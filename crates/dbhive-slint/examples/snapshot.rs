//! Render a Slint component to a PNG-compatible image with no display.
//!
//! Uses Slint's software renderer behind a minimal platform, so screens can
//! be inspected from CI or while the machine's display is asleep, and never
//! captures anything else on the screen.
//!
//!     cargo run -p dbhive-slint --example snapshot -- <component> <out.ppm> [width height scale]

use std::rc::Rc;

use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter, WindowEvent};
use slint::ComponentHandle;

use dbhive_slint::*;

const SAMPLE_SQL: &str = "-- Top customers by revenue\nSELECT c.id, c.name, SUM(o.total) AS revenue\n  FROM customers c\n  JOIN orders o ON o.customer_id = c.id\n\n WHERE o.status = 'paid' AND o.note <> 'it''s a test'\n   AND o.created_at >= '2026-01-01'\n/* block comment\n   across lines */\n GROUP BY c.id, c.name\nHAVING SUM(o.total) > 1000.50\n ORDER BY revenue DESC\n LIMIT 25;";

struct SnapshotPlatform {
    window: Rc<MinimalSoftwareWindow>,
    start: std::time::Instant,
}

impl Platform for SnapshotPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }
    fn duration_since_start(&self) -> core::time::Duration {
        self.start.elapsed()
    }
}

/// Everything a snapshot can show. Each entry creates, configures and keeps
/// its component alive for the duration of the render.
fn instantiate(name: &str) -> Box<dyn std::any::Any> {
    match name {
        "app" => {
            let c = AppWindow::new().expect("AppWindow");
            c.show().unwrap();
            Box::new(c)
        }
        // workspace, workspace-light, -long, -empty, -loading, -error, plus the
        // query-tab states: -query, -query-light, -query-error, -query-running,
        // -query-dml, -query-blank, -query-confirm.
        _ if name.starts_with("workspace") => {
            let c = AppWindow::new().expect("AppWindow");
            if name == "workspace-light" || name.ends_with("-light") {
                c.global::<Theme>().set_dark(false);
            }
            c.set_screen(Screen::Workspace);
            c.set_workspace_connection_name("Production Postgres".into());
            c.set_workspace_location_label("app · public".into());

            let mut names = vec!["customers", "orders", "order_items", "products", "refunds"];
            if name == "workspace-long" {
                names.push("a_table_with_a_very_long_name_that_must_be_elided_not_overflow_0001");
            }
            let tables: Vec<TableItem> = names
                .iter()
                .enumerate()
                .map(|(i, name)| TableItem {
                    name: (*name).into(),
                    row_count: format!("~{}", (i + 1) * 1240).into(),
                })
                .collect();
            if name == "workspace-error" {
                c.set_workspace_tables_error(
                    "connection to server at \"db.internal\" failed: timeout expired".into(),
                );
            } else if name != "workspace-empty" {
                c.set_workspace_tables(slint::ModelRc::new(slint::VecModel::from(tables)));
            }

            if name == "workspace-loading" {
                c.set_workspace_loading_tables(true);
            } else if name.starts_with("workspace-query") {
                let dark = c.global::<Theme>().get_dark();
                let tab = |title: &str, query: bool| TabInfo { title: title.into(), query, busy: name == "workspace-query-running" && query };
                c.set_workspace_tabs(slint::ModelRc::new(slint::VecModel::from(vec![
                    tab("Query 1", true),
                    tab("customers", false),
                ])));
                c.set_workspace_active_tab(0);
                c.set_workspace_active_is_query(true);
                if name != "workspace-query-blank" {
                    c.set_workspace_editor_text(SAMPLE_SQL.into());
                    c.set_workspace_editor_lines(dbhive_slint::sql_highlight::to_model_for(SAMPLE_SQL, dark));
                }
                c.set_workspace_result_title("Results".into());
                match name {
                    "workspace-query-running" => c.set_workspace_running(true),
                    "workspace-query-error" => c.set_workspace_rows_error(
                        "syntax error at or near \"FORM\" (position 31). Did you mean FROM?".into(),
                    ),
                    "workspace-query-dml" => {
                        c.set_workspace_has_result(true);
                        c.set_workspace_result_summary("3 rows affected · 4ms".into());
                        c.set_workspace_result_message("3 rows affected · 4ms".into());
                    }
                    "workspace-query-blank" => {}
                    // Autocomplete: columns after SELECT, popup under the caret.
                    "workspace-query-complete" | "workspace-query-complete-light" => {
                        let sql = "SELECT c.na, count(*)\n  FROM customers c\n  JOIN orders o ON o.customer_id = c.id\n GROUP BY c.na";
                        c.set_workspace_editor_text(sql.into());
                        c.set_workspace_editor_lines(dbhive_slint::sql_highlight::to_model_for(sql, dark));
                        let item = |label: &str, detail: &str, kind: &str| dbhive_slint::CompletionItem {
                            label: label.into(),
                            detail: detail.into(),
                            kind: kind.into(),
                        };
                        c.set_workspace_completions(slint::ModelRc::new(slint::VecModel::from(vec![
                            item("name", "text", "column"),
                            item("national_id", "varchar(32)", "column"),
                            item("count", "function", "function"),
                            item("natural", "keyword", "keyword"),
                            item("notes", "public", "table"),
                        ])));
                        c.set_workspace_completion_selected(0);
                        // Puts the caret after `c.na` on the first line so the
                        // popup draws beneath it.
                        c.set_workspace_editor_pending_cursor("SELECT c.na".len() as i32);
                    }
                    _ => {
                        let cols = vec!["id".to_string(), "name".to_string(), "revenue".to_string()];
                        let widths = vec![80.0f32, 240.0, 160.0];
                        let (columns, width) = dbhive_slint::grid_model::layout_columns(&cols, &widths);
                        c.set_workspace_columns(columns);
                        c.set_workspace_grid_width(width);
                        let rows: Vec<Vec<Option<String>>> = (1..=6)
                            .map(|i| vec![Some(i.to_string()), Some(format!("Customer {i}")), Some(format!("{}.50", 4200 - i * 310))])
                            .collect();
                        c.set_workspace_rows(slint::ModelRc::new(dbhive_slint::grid_model::LazyRows::new(std::rc::Rc::new(rows))));
                        c.set_workspace_row_count(6);
                        c.set_workspace_visible_row_count(6);
                        c.set_workspace_has_result(true);
                        c.set_workspace_result_summary("6 rows · 12ms".into());
                    }
                }
                if name == "workspace-query-confirm" {
                    c.set_confirm_title("Run this statement?".into());
                    c.set_confirm_message("• DELETE without WHERE removes every row of orders".into());
                    c.set_confirm_label("Run anyway".into());
                    c.set_show_confirm(true);
                }
            } else if name == "workspace" || name == "workspace-light" || name == "workspace-long" {
                c.set_workspace_tabs(slint::ModelRc::new(slint::VecModel::from(vec![
                    TabInfo { title: "customers".into(), query: false, busy: false },
                    TabInfo { title: "Query 1".into(), query: true, busy: false },
                ])));
                c.set_workspace_active_tab(0);
                c.set_workspace_result_title("customers".into());
                c.set_workspace_has_result(true);
                c.set_workspace_result_summary("8 rows · 12ms".into());
                c.set_workspace_selected_table("customers".into());
                let cols = vec!["id".to_string(), "name".to_string(), "email".to_string(), "plan".to_string()];
                let widths = vec![80.0f32, 220.0, 260.0, 140.0];
                let (columns, width) = dbhive_slint::grid_model::layout_columns(&cols, &widths);
                c.set_workspace_columns(columns);
                c.set_workspace_grid_width(width);
                let rows: Vec<Vec<Option<String>>> = (1..=8)
                    .map(|i| {
                        vec![
                            Some(i.to_string()),
                            Some(format!("Customer {i}")),
                            if i == 3 { None } else { Some(format!("user{i}@example.com")) },
                            Some(if i % 2 == 0 { "pro" } else { "free" }.to_string()),
                        ]
                    })
                    .collect();
                c.set_workspace_rows(slint::ModelRc::new(dbhive_slint::grid_model::LazyRows::new(
                    std::rc::Rc::new(rows),
                )));
                c.set_workspace_row_count(8);
                c.set_workspace_visible_row_count(8);
            }
            c.show().unwrap();
            Box::new(c)
        }
        _ if name.starts_with("inspector") => inspector(name),
        // The ER diagram and the schema designer, over the workspace.
        _ if name == "er" || name.starts_with("er-") => er_window(name),
        _ if name.starts_with("designer") => designer_window(name),
        // The command palette and shortcuts help over a real screen, drawn by
        // the same code the app uses (palette_view). Variants:
        //   palette, palette-workspace, palette-filtered, palette-theme,
        //   palette-window, palette-empty, palette-bottom, palette-light,
        //   palette-light-filtered, shortcuts, shortcuts-filtered,
        //   shortcuts-empty, shortcuts-light.
        _ if name.starts_with("palette") || name.starts_with("shortcuts") => {
            command_overlay(name)
        }
        "connections" | "connections-light" => {
            let c = AppWindow::new().expect("AppWindow");
            if name == "connections-light" {
                c.global::<Theme>().set_dark(false);
            }
            c.set_app_version("0.21.0".into());
            let item = |id: &str, n: &str, host: &str, ini: &str, rgb: (u8, u8, u8), env: &str, tone: Tone, fav: bool, connected: bool| ConnectionItem {
                id: id.into(),
                name: n.into(),
                host: host.into(),
                initials: ini.into(),
                tint: slint::Color::from_rgb_u8(rgb.0, rgb.1, rgb.2),
                env_label: env.into(),
                env_tone: tone,
                favorite: fav,
                connected,
                connecting: false,
            };
            let items = vec![
                item("1", "Production Postgres", "PostgreSQL • db.prod.internal:5432", "PG", (56, 118, 171), "Production", Tone::Danger, true, true),
                item("2", "Local MySQL", "MySQL • localhost:3306", "MY", (0, 117, 143), "Local", Tone::Neutral, false, false),
                item("3", "Analytics SQLite", "SQLite • /data/analytics.db", "SL", (0, 121, 168), "", Tone::Neutral, false, false),
                item("4", "Staging Mongo", "MongoDB • staging.example.com:27017", "MG", (77, 179, 61), "Staging", Tone::Warning, false, false),
            ];
            c.set_connection_total(items.len() as i32);
            c.set_connections(slint::ModelRc::new(slint::VecModel::from(items)));
            c.show().unwrap();
            Box::new(c)
        }
        "about" => {
            let c = AppWindow::new().expect("AppWindow");
            c.set_app_version("0.21.0".into());
            c.set_show_about(true);
            c.show().unwrap();
            Box::new(c)
        }
        "confirm" => {
            let c = AppWindow::new().expect("AppWindow");
            c.set_confirm_title("Delete connection?".into());
            c.set_confirm_message(
                "\"Production Postgres\" will be removed. This cannot be undone.".into(),
            );
            c.set_confirm_label("Delete".into());
            c.set_show_confirm(true);
            c.show().unwrap();
            Box::new(c)
        }
        "connection-editor" | "connection-editor-sqlite" | "connection-editor-ssh" => {
            let c = AppWindow::new().expect("AppWindow");
            let driver = if name == "connection-editor-sqlite" {
                dbhive_ui_model::connection_editor::ALL_DRIVERS[2].clone() // Sqlite
            } else {
                dbhive_ui_model::connection_editor::ALL_DRIVERS[0].clone() // Postgres
            };
            let mut form = dbhive_ui_model::connection_editor::ConnectionForm::new_for(driver);
            let mut data = connection_editor::form_data(&form);
            if name == "connection-editor-ssh" {
                form.ssh_enabled = true;
                form.ssh_host = "bastion.example.com".into();
                form.ssh_username = "deploy".into();
                form.color = dbhive_ui_model::connection_editor::COLOR_PRESETS[3].into();
                data = connection_editor::form_data(&form);
                data.connection_string = "mysql://u:p@h:3306/d".into();
                data.connection_string_error =
                    "Not a connection string for this driver. Expected e.g. postgresql://user:password@host:5432/database".into();
            }
            c.set_editor_is_new(true);
            c.set_editor(data);
            c.set_show_connection_editor(true);
            c.show().unwrap();
            Box::new(c)
        }
        // settings[-appearance|-query|-shortcuts][-light] plus a state:
        // -dirty, -accent, -errors, -recording, -duplicate, -unbound,
        // -loading, -error, -confirm.
        _ if name.starts_with("settings") => {
            use dbhive_core::models::settings::AppSettings;
            use dbhive_ui_model::settings::{KeyModifiers, Section, ShortcutField};
            use dbhive_ui_model::settings_editor::SettingsEditor;
            use dbhive_slint::settings_view::{self, Banner, Load, View};

            let c = AppWindow::new().expect("AppWindow");
            c.set_app_version("0.21.0".into());
            let has = |t: &str| name.split('-').any(|p| p == t);
            let mut ed = SettingsEditor::new(AppSettings::default());
            let mut view = View::default();
            if has("appearance") {
                ed.set_section(Section::Appearance);
            } else if has("query") {
                ed.set_section(Section::Query);
            } else if has("shortcuts") {
                ed.set_section(Section::Shortcuts);
            }
            if has("dirty") {
                ed.edit(|s| {
                    s.general.auto_save_connections = false;
                    s.query.confirm_destructive = false;
                });
            }
            if has("accent") {
                ed.pick_accent_preset(1);
            }
            if has("errors") {
                ed.set_number_text(dbhive_ui_model::settings_editor::NumberField::EditorFontSize, "40");
                ed.set_number_text(dbhive_ui_model::settings_editor::NumberField::QueryTimeout, "soon");
                ed.set_number_text(dbhive_ui_model::settings_editor::NumberField::MaxRows, "5");
                ed.set_accent_text("#12");
                ed.set_auto_check(true);
                ed.set_number_text(dbhive_ui_model::settings_editor::NumberField::UpdateInterval, "0");
            }
            if has("recording") {
                ed.start_recording_index(1);
            }
            if has("duplicate") {
                let ctrl = KeyModifiers { ctrl: true, command: true, ..Default::default() };
                ed.start_recording(ShortcutField::NewTab);
                if let dbhive_ui_model::settings_editor::RecordOutcome::Bound { warning: Some(message), .. } =
                    ed.record(ctrl, "W", false)
                {
                    view.banner = Some(Banner { message, tone: Tone::Warning });
                }
            }
            if has("unbound") {
                for f in dbhive_ui_model::settings_editor::shortcut_fields() {
                    ed.clear_shortcut(f);
                }
            }
            if has("loading") {
                view.load = Load::Loading;
            }
            if has("error") {
                view.load = Load::Failed(
                    "settings.json is not valid JSON: expected value at line 1 column 1. Refusing to overwrite it.".into(),
                );
            }
            if has("saving") {
                view.saving = true;
            }

            // The same live effects the controller applies.
            let a = ed.appearance(Some(!has("light")));
            let theme = c.global::<Theme>();
            theme.set_dark(a.mode == dbhive_ui_model::settings::ResolvedMode::Dark);
            theme.set_accent_custom(a.accent.is_some());
            if let Some((r, g, b)) = a.accent {
                theme.set_accent_custom_color(slint::Color::from_rgb_u8(r, g, b));
                let (fr, fg, fb) = a.accent_foreground;
                theme.set_accent_custom_foreground(slint::Color::from_rgb_u8(fr, fg, fb));
            }
            theme.set_editor_font_size(a.editor_font_size as f32);

            c.set_settings(settings_view::data(&ed, &view));
            c.set_show_settings(true);
            if has("confirm") {
                c.set_confirm_title("Reset all settings to defaults?".into());
                c.set_confirm_message(
                    "This restores every setting, not only shortcuts, to its default value. It cannot be undone.".into(),
                );
                c.set_confirm_label("Reset".into());
                c.set_show_confirm(true);
            }
            c.show().unwrap();
            Box::new(c)
        }
        "kit-dark" | "kit-light" => {
            let c = KitGallery::new().expect("KitGallery");
            c.global::<Theme>().set_dark(name == "kit-dark");
            c.show().unwrap();
            Box::new(c)
        }
        "sql-selection" => {
            let c = SqlEditorSpike::new().expect("SqlEditorSpike");
            c.set_text(SAMPLE_SQL.into());
            c.set_lines(dbhive_slint::sql_highlight::to_model(SAMPLE_SQL));
            c.show().unwrap();
            // Select "customers c" on line 3 so selection and caret both show.
            let start = SAMPLE_SQL.find("customers c").unwrap() as i32;
            c.invoke_select_range(start, start + "customers c".len() as i32);
            Box::new(c)
        }
        "sql" | "sql-debug" => {
            let c = SqlEditorSpike::new().expect("SqlEditorSpike");
            let sql = SAMPLE_SQL;
            c.set_text(sql.into());
            c.set_lines(dbhive_slint::sql_highlight::to_model(sql));
            c.set_debug_alignment(name == "sql-debug");
            c.show().unwrap();
            Box::new(c)
        }
        _ if name.starts_with("history") || name.starts_with("snippets") || name.starts_with("templates") => {
            history_panel_window(name)
        }
        _ if name.starts_with("import") || name.starts_with("export") => import_export_window(name),
        _ if name.starts_with("backup") => backup_window(name),
        _ if name.starts_with("activity") => activity_window(name),
        _ if name.starts_with("migrations") => migrations_window(name),
        _ if name.starts_with("builder") => builder_window(name),
        _ if name.starts_with("tabledialog") => table_dialog_window(name),
        _ if name.starts_with("plugins") => plugins_window(name),
        _ if name.starts_with("ai-") => ai_window(name),
        other => {
            eprintln!("unknown component {:?}", other);
            std::process::exit(2);
        }
    }
}

// -- History / snippets / templates panel ------------------------------------
//
// Variants: history, history-light, history-empty, history-loading,
// history-error, history-long; snippets, snippets-light, snippets-empty,
// snippets-loading, snippets-error, snippets-form; templates,
// templates-light, templates-empty, templates-loading, templates-error,
// templates-run, templates-run-errors, templates-form, templates-import.
// The workspace sits behind the panel, as it does in the app.

fn history_panel_window(name: &str) -> Box<dyn std::any::Any> {
    // "-light" only changes the theme; every other check uses the base name.
    let light = name.ends_with("-light");
    let name = name.trim_end_matches("-light");
    use dbhive_core::models::{QueryHistory, QuerySnippet};
    use dbhive_slint::history_view::{form_params, rows_model, run_params, strings_model};
    use dbhive_ui_model::history::{sync_parameters, QueryTemplate};
    use dbhive_ui_model::history_panel as hp;

    let c = AppWindow::new().expect("AppWindow");
    if light {
        c.global::<Theme>().set_dark(false);
    }
    // The workspace behind the dimmed panel.
    c.set_screen(Screen::Workspace);
    c.set_workspace_connection_name("Production Postgres".into());
    c.set_workspace_location_label("app · public".into());
    let tables: Vec<TableItem> = ["customers", "orders", "order_items", "products"]
        .iter()
        .map(|n| TableItem { name: (*n).into(), row_count: "~1240".into() })
        .collect();
    c.set_workspace_tables(slint::ModelRc::new(slint::VecModel::from(tables)));
    c.set_workspace_tabs(slint::ModelRc::new(slint::VecModel::from(vec![TabInfo {
        title: "Query 1".into(),
        query: true,
        busy: false,
    }])));
    c.set_workspace_active_tab(0);
    c.set_workspace_active_is_query(true);

    let h = c.global::<HistoryState>();
    h.set_visible(true);
    let now = chrono::Utc::now();
    let ago = |mins: i64| (now - chrono::Duration::minutes(mins)).to_rfc3339();

    let tab = if name.starts_with("snippets") { 1 } else if name.starts_with("templates") { 2 } else { 0 };
    h.set_tab(tab);
    h.set_scope_label("Production Postgres / app".into());
    h.set_tab_labels(strings_model(&[
        "History".into(),
        if name == "snippets-empty" { "Snippets".into() } else { "Snippets (4)".into() },
        if name == "templates-empty" { "Templates".into() } else { "Templates (3)".into() },
    ]));

    // History.
    let q = |sql: &str, mins: i64, ms: u64, rows: u64| {
        QueryHistory::new("c1".into(), "Production Postgres".into(), "app".into(), sql.into(), ago(mins))
            .with_success(ms, Some(rows))
    };
    let mut entries = vec![
        q("SELECT c.id, c.name, SUM(o.total) AS revenue\n  FROM customers c JOIN orders o ON o.customer_id = c.id", 2, 38, 25),
        q("UPDATE orders SET status = 'shipped' WHERE id = 1042", 14, 4, 1),
        QueryHistory::new("c1".into(), "Production Postgres".into(), "app".into(),
            "SELECT * FROM order_itemz WHERE order_id = 7".into(), ago(55))
            .with_error("relation \"order_itemz\" does not exist".into(), Some(3)),
        q("SELECT COUNT(*) FROM events WHERE created_at > now() - interval '1 day'", 60 * 5, 1840, 1),
        q("SELECT * FROM customers ORDER BY created_at DESC LIMIT 100", 60 * 27, 12, 100),
        q("SELECT a_column_with_a_very_long_name_that_must_be_elided_rather_than_overflow_the_row, another_long_column_name_here, and_one_more FROM a_table_with_a_very_long_name WHERE x = 1", 60 * 24 * 3, 9, 1204),
    ];
    if name == "history-long" {
        entries[2].error_message = Some("ERROR: relation \"order_itemz\" does not exist at character 15; HINT: Perhaps you meant to reference the table \"order_items\". The statement was rolled back and no rows were changed.".into());
    }
    if name != "history-empty" {
        h.set_history(rows_model(hp::history_rows(&entries, now)));
        h.set_history_summary(format!("{} queries, newest first", entries.len()).into());
    }
    h.set_scoped(true);
    if name == "history-loading" {
        h.set_loading(true);
    }
    if name == "history-error" {
        h.set_error("Cleared, but the change could not be saved: Permission denied (os error 13)".into());
    }

    // Snippets.
    let snippets = vec![
        QuerySnippet::new("Active users this week".into(), "SELECT id, email FROM users WHERE last_seen > now() - interval '7 days'".into(), Some("Users seen in the last seven days".into()), Some(vec!["reports".into(), "weekly".into()])),
        QuerySnippet::new("Table sizes".into(), "SELECT relname, pg_size_pretty(pg_total_relation_size(relid)) FROM pg_catalog.pg_statio_user_tables ORDER BY 2 DESC".into(), None, Some(vec!["maintenance".into()])),
        QuerySnippet::new("Blocked queries".into(), "SELECT pid, query FROM pg_stat_activity WHERE wait_event_type = 'Lock'".into(), Some("Who is waiting on a lock".into()), Some(vec!["maintenance".into()])),
        QuerySnippet::new("Top customers by revenue".into(), "SELECT c.name, SUM(o.total) FROM customers c JOIN orders o ON o.customer_id = c.id GROUP BY 1 ORDER BY 2 DESC LIMIT 10".into(), None, None),
    ];
    let tags = dbhive_ui_model::history::snippet_tags(&snippets);
    if name != "snippets-empty" {
        let picked = (name == "snippets").then_some("reports");
        let rows = hp::snippet_rows(&snippets, "", picked);
        h.set_snippet_summary(format!("{} of 4 snippets", rows.len()).into());
        h.set_snippets(rows_model(rows));
        h.set_snippet_tags(strings_model(&tags));
        if picked.is_some() {
            h.set_snippet_tag(1);
        }
    }
    if name == "snippets-loading" {
        h.set_loading(true);
    }
    if name == "snippets-error" {
        h.set_error("Could not save snippets: No space left on device (os error 28)".into());
    }

    // Templates.
    let mk = |n: &str, d: Option<&str>, sql: &str, cat: Option<&str>, tags: &[&str]| {
        QueryTemplate::new(n.into(), d.map(Into::into), sql.into(), sync_parameters(sql, &[]), cat.map(Into::into),
            (!tags.is_empty()).then(|| tags.iter().map(|t| t.to_string()).collect()))
    };
    let templates = vec![
        mk("Orders by status and date", Some("Filter orders for a day"), "SELECT * FROM orders WHERE status = {{status}} AND created_at >= {{since:date}} AND total > {{min_total:decimal}}", Some("Reports"), &["orders"]),
        mk("Find user by email", None, "SELECT * FROM users WHERE email = {{email}}", Some("Support"), &[]),
        mk("Recent signups", Some("New users in the last N days"), "SELECT * FROM users WHERE created_at > now() - ({{days:integer}} * interval '1 day') AND active = {{active:boolean}}", Some("Reports"), &["users", "growth"]),
    ];
    let cats = dbhive_ui_model::history::template_categories(&templates);
    if name != "templates-empty" && name != "templates-error" {
        h.set_templates(rows_model(hp::template_rows(&templates, "", None)));
        h.set_template_categories(strings_model(&cats));
        h.set_template_summary("3 of 3 templates".into());
    }
    if name == "templates-loading" {
        h.set_loading(true);
    }
    if name == "templates-error" {
        h.set_error("templates.json could not be read: expected value at line 1 column 1".into());
        h.set_error_retry(true);
    }

    // Forms.
    let dark = c.global::<Theme>().get_dark();
    if name == "snippets-form" {
        h.set_mode(PanelMode::SnippetForm);
        h.set_form_is_new(false);
        h.set_form_name("Active users this week".into());
        h.set_form_description("Users seen in the last seven days".into());
        h.set_form_tags("reports, weekly".into());
        let sql = "SELECT id, email\n  FROM users\n WHERE last_seen > now() - interval '7 days';";
        h.set_form_sql(sql.into());
        h.set_form_lines(dbhive_slint::sql_highlight::to_model_for(sql, dark));
    }
    if name == "templates-form" {
        h.set_mode(PanelMode::TemplateForm);
        h.set_form_is_new(false);
        h.set_form_name("Orders by status and date".into());
        h.set_form_description("Filter orders for a day".into());
        h.set_form_category("Reports".into());
        h.set_form_tags("orders".into());
        let sql = &templates[0].sql;
        h.set_form_sql(sql.as_str().into());
        h.set_form_lines(dbhive_slint::sql_highlight::to_model_for(sql, dark));
        let mut params = templates[0].parameters.clone();
        params[1].default_value = Some("2025-01-01".into());
        params[2].required = false;
        h.set_form_params(form_params(&params));
        if name == "templates-form" {
            h.set_form_error("".into());
        }
    }
    if name == "templates-run" || name == "templates-run-errors" {
        h.set_mode(PanelMode::TemplateRun);
        let mut run = hp::TemplateRun::new(&templates[0]);
        run.set_value(0, "paid");
        if name == "templates-run-errors" {
            run.set_value(1, "last tuesday");
            let _ = run.submit();
            h.set_form_error("Fix the highlighted fields first.".into());
        } else {
            run.set_value(1, "2025-01-01");
            run.set_value(2, "99.50");
        }
        h.set_run_title("Run: Orders by status and date".into());
        h.set_run_params(run_params(run.fields()));
        h.set_run_errors(strings_model(&run.errors()));
        h.set_run_preview(run.preview().into());
    }
    if name == "templates-import" {
        h.set_mode(PanelMode::TemplateImport);
        h.set_import_path("/Users/me/Downloads/query-templates.json".into());
        h.set_import_error("Could not read /Users/me/Downloads/query-templates.json: No such file or directory (os error 2)".into());
    }
    c.show().unwrap();
    Box::new(c)
}

/// The table inspector (a table tab). Variants, each with an optional `-light`:
/// inspector, -filters, -panel, -empty, -loading, -error, -deep, -long,
/// -structure, -structure-loading, -structure-error, -structure-empty.
fn inspector(name: &str) -> Box<dyn std::any::Any> {
    use dbhive_slint::{ColumnRowItem, FilterRowItem, IndexRowItem, InspectorBridge, RelationRowItem};

    let c = AppWindow::new().expect("AppWindow");
    if name.ends_with("-light") {
        c.global::<Theme>().set_dark(false);
    }
    let v = name.trim_end_matches("-light");
    let structure = v.starts_with("inspector-structure");

    c.set_screen(Screen::Workspace);
    c.set_workspace_connection_name("Production Postgres".into());
    c.set_workspace_location_label("app · public".into());
    let tables: Vec<TableItem> = ["customers", "orders", "order_items", "products", "refunds"]
        .iter()
        .enumerate()
        .map(|(i, n)| TableItem { name: (*n).into(), row_count: format!("~{}", (i + 1) * 1240).into() })
        .collect();
    c.set_workspace_tables(slint::ModelRc::new(slint::VecModel::from(tables)));
    c.set_workspace_selected_table("orders".into());
    c.set_workspace_tabs(slint::ModelRc::new(slint::VecModel::from(vec![
        TabInfo { title: "Query 1".into(), query: true, busy: false },
        TabInfo { title: "orders".into(), query: false, busy: v == "inspector-loading" },
    ])));
    c.set_workspace_active_tab(1);
    c.set_workspace_active_is_query(false);

    let b = c.global::<InspectorBridge>();
    let s = |x: &str| -> slint::SharedString { x.into() };
    let strs = |xs: &[&str]| slint::ModelRc::new(slint::VecModel::from(xs.iter().map(|x| s(x)).collect::<Vec<_>>()));

    // Pager and toolbar, shared by every data variant.
    b.set_view(if structure { 1 } else { 0 });
    b.set_total_label(s("112 rows"));
    b.set_elapsed_label(s("14ms"));
    b.set_page_label(s("Page 2 of 5"));
    b.set_range_label(s("26-50 of 112 rows"));
    b.set_page_size_label(s("25 / page"));
    b.set_page_size_options(strs(&["25 / page", "50 / page", "100 / page", "250 / page"]));
    b.set_can_first(true);
    b.set_can_prev(true);
    b.set_can_next(true);
    b.set_filter_columns(strs(&["id", "customer_id", "note", "total", "created_at"]));
    b.set_filter_ops(strs(&["contains", "equals", "starts with", "is null"]));

    // The page of rows.
    let cols: Vec<String> = ["id", "customer_id", "note", "total", "created_at"].iter().map(|x| x.to_string()).collect();
    let widths = vec![80.0f32, 140.0, 240.0, 120.0, 200.0];
    let (columns, width) = dbhive_slint::grid_model::layout_columns(&cols, &widths);
    let data: Vec<Vec<Option<String>>> = (26..=50)
        .map(|i| {
            vec![
                Some(i.to_string()),
                Some(((i % 5) + 1).to_string()),
                if i == 28 { None } else if i % 10 == 0 { Some(format!("gift-{i}")) } else { Some(format!("order-{i}")) },
                Some(format!("{}.50", i * 10)),
                Some(format!("2026-10-{:02} 09:{:02}:00", (i % 28) + 1, i % 60)),
            ]
        })
        .collect();
    let n = data.len();
    let long_note = "A very long note that has to be elided and not run over into the next column of the grid at all";

    match v {
        "inspector-empty" => {
            c.set_workspace_has_result(true);
            c.set_workspace_columns(columns);
            c.set_workspace_grid_width(width);
            b.set_total_label(s("0 rows"));
            b.set_page_label(s("Page 1 of 1"));
            b.set_range_label(s("0 of 0 rows"));
            b.set_can_first(false);
            b.set_can_prev(false);
            b.set_can_next(false);
        }
        "inspector-loading" => {
            c.set_workspace_running(true);
            b.set_total_label(s("Counting rows…"));
            b.set_page_label(s("Page 1"));
            b.set_range_label(s("No rows"));
            b.set_elapsed_label(s(""));
            b.set_can_first(false);
            b.set_can_prev(false);
            b.set_can_next(false);
        }
        "inspector-error" => {
            c.set_workspace_rows_error(
                "Query failed: relation \"public.orders\" does not exist (position 15)".into(),
            );
            b.set_total_label(s(""));
            b.set_page_label(s("Page 1"));
            b.set_range_label(s("No rows"));
            b.set_can_first(false);
            b.set_can_prev(false);
            b.set_can_next(false);
        }
        _ if !structure => {
            let mut data = data;
            if v == "inspector-long" {
                data[2][2] = Some(long_note.to_string());
            }
            c.set_workspace_columns(columns);
            c.set_workspace_grid_width(width);
            let editing = v.starts_with("inspector-edit") || v.starts_with("inspector-review");
            if editing {
                // Pending changes: an edited note, a row marked for deletion
                // and a new row at the bottom.
                use dbhive_slint::grid_model::RowMark;
                data[1][2] = Some("order-27 (rush)".to_string());
                data.push(vec![
                    Some("auto".into()),
                    Some("3".into()),
                    Some("Gift wrap".into()),
                    Some("default".into()),
                    Some("default".into()),
                ]);
                let mut marks: Vec<RowMark> = data
                    .iter()
                    .map(|r| RowMark {
                        deleted: false,
                        added: false,
                        modified: vec![false; r.len()],
                        placeholder: vec![false; r.len()],
                    })
                    .collect();
                marks[1].modified[2] = true;
                marks[4].deleted = true;
                let last = marks.len() - 1;
                marks[last].added = true;
                marks[last].placeholder = vec![true, false, false, true, true];
                let rows = data.len() as i32;
                c.set_workspace_rows(slint::ModelRc::new(dbhive_slint::grid_model::LazyRows::with_marks(
                    std::rc::Rc::new(data),
                    std::rc::Rc::new(marks),
                )));
                c.set_workspace_row_count(rows);
                c.set_workspace_visible_row_count(rows);
                b.set_editable(true);
                b.set_pending_count(3);
                b.set_selected_row(4);
                b.set_selected_col(1);
                b.set_selected_deleted(true);
                if v == "inspector-edit-editing" {
                    b.set_editing_row(1);
                    b.set_editing_col(2);
                    b.set_edit_initial(s("order-27 (rush)"));
                }
                if v == "inspector-edit-error" {
                    b.set_edit_message(s("Not saved: total must be a number, got \"ten\""));
                }
                if v.starts_with("inspector-review") {
                    b.set_review_open(true);
                    if v == "inspector-review-failed" {
                        b.set_edit_message(s("Nothing was changed. Change 3 of 3 failed (insert into orders): UNIQUE constraint failed: orders.code"));
                    }
                    b.set_review_items(slint::ModelRc::new(slint::VecModel::from(vec![
                        dbhive_slint::ReviewItem {
                            summary: s("Change note on order 27"),
                            sql: s("UPDATE \"orders\" SET \"note\" = 'order-27 (rush)' WHERE \"id\" = 27"),
                            kind: s("update"),
                        },
                        dbhive_slint::ReviewItem {
                            summary: s("Delete order 30"),
                            sql: s("DELETE FROM \"orders\" WHERE \"id\" = 30"),
                            kind: s("delete"),
                        },
                        dbhive_slint::ReviewItem {
                            summary: s("Add a row to orders"),
                            sql: s("INSERT INTO \"orders\" (\"customer_id\", \"note\") VALUES (3, 'Gift wrap')"),
                            kind: s("insert"),
                        },
                    ])));
                }
            } else {
                c.set_workspace_rows(slint::ModelRc::new(dbhive_slint::grid_model::LazyRows::new(std::rc::Rc::new(data))));
                c.set_workspace_row_count(n as i32);
                c.set_workspace_visible_row_count(n as i32);
                b.set_editable(v != "inspector-readonly");
                if v == "inspector-readonly" {
                    b.set_edit_note(s("This kind of connection cannot save edits safely, so the grid is read-only."));
                }
            }
            c.set_workspace_has_result(true);
            b.set_sort_column(3);
            b.set_sort_desc(true);
            if v == "inspector-deep" {
                b.set_page_label(s("Page 4,001 of 40,000"));
                b.set_range_label(s("100,001-100,025 of 1,000,000 rows"));
                b.set_total_label(s("1,000,000 rows"));
                b.set_can_next(false);
                b.set_show_jump(true);
                b.set_deep_blocked(true);
            }
            if v == "inspector-filters" || v == "inspector-panel" {
                b.set_filters_open(v == "inspector-filters");
                b.set_applied_filter_count(1);
                b.set_total_label(s("13 rows match"));
                b.set_filters_dirty(v == "inspector-filters");
                b.set_filter_rows(slint::ModelRc::new(slint::VecModel::from(vec![
                    FilterRowItem { column: s("note"), op: s("contains"), value: s("gift"), takes_value: true },
                    FilterRowItem { column: s("created_at"), op: s("is null"), value: s(""), takes_value: false },
                ])));
            }
            if v == "inspector-panel" {
                b.set_selected_row(2);
                b.set_selected_col(2);
                b.set_panel_open(true);
                b.set_panel_title(s("Row 3 · note"));
                b.set_panel_json(s("{\n  \"id\": 28,\n  \"customer_id\": 4,\n  \"note\": null,\n  \"total\": 280.5,\n  \"created_at\": \"2026-10-01 09:28:00\",\n  \"meta\": {\n    \"source\": \"web\",\n    \"tags\": [\n      \"gift\",\n      \"rush\"\n    ]\n  }\n}"));
            }
        }
        _ => {
            // Structure variants.
            match v {
                "inspector-structure-loading" => b.set_meta_loading(true),
                "inspector-structure-error" => b.set_meta_error(s("Could not read the schema: permission denied for table orders")),
                "inspector-structure-empty" => {}
                _ => {
                    let col = |name: &str, ty: &str, nullable: bool, def: &str, pk: bool, auto: bool, fk: &str, fk_table: &str| ColumnRowItem {
                        name: s(name),
                        data_type: s(ty),
                        nullable,
                        default_value: s(def),
                        primary: pk,
                        auto,
                        foreign: s(fk),
                        foreign_table: s(fk_table),
                    };
                    b.set_struct_columns(slint::ModelRc::new(slint::VecModel::from(vec![
                        col("id", "integer", false, "nextval('orders_id_seq')", true, true, "", ""),
                        col("customer_id", "integer", false, "(none)", false, false, "customers(id)", "customers"),
                        col("note", "text", true, "(none)", false, false, "", ""),
                        col("total", "numeric(10,2)", false, "0", false, false, "", ""),
                        col("created_at", "timestamp with time zone", true, "now()", false, false, "", ""),
                    ])));
                    b.set_struct_indexes(slint::ModelRc::new(slint::VecModel::from(vec![
                        IndexRowItem { name: s("orders_pkey"), columns: s("id"), kind: s("PRIMARY") },
                        IndexRowItem { name: s("ix_orders_customer"), columns: s("customer_id, created_at"), kind: s("INDEX") },
                        IndexRowItem { name: s("uq_orders_ref"), columns: s("note"), kind: s("UNIQUE") },
                    ])));
                    b.set_struct_relations(slint::ModelRc::new(slint::VecModel::from(vec![
                        RelationRowItem { constraint: s("orders_customer_id_fkey"), outgoing: true, from: s("orders(customer_id)"), to: s("customers(id)"), actions: s("CASCADE / NO ACTION"), other_table: s("customers") },
                        RelationRowItem { constraint: s("order_items_order_id_fkey"), outgoing: false, from: s("order_items(order_id)"), to: s("orders(id)"), actions: s("CASCADE / NO ACTION"), other_table: s("order_items") },
                    ])));
                }
            }
        }
    }
    c.show().unwrap();
    Box::new(c)
}


/// `palette*` and `shortcuts*` variants. The screen behind is the populated
/// workspace unless the name says otherwise (`palette` alone is the connection
/// list), so the dimmed backdrop and the contextual commands are both real.
fn command_overlay(name: &str) -> Box<dyn std::any::Any> {
    use dbhive_ui_model::command_palette::{Scope, IS_MAC};
    use dbhive_ui_model::palette_session::{CommandContext, PaletteState, ShortcutsState};

    let light = name.contains("light");
    let on_connections = name == "palette" || name == "shortcuts" || name == "shortcuts-empty";
    let backdrop = match (on_connections, light) {
        (true, false) => "connections",
        (true, true) => "connections-light",
        (false, false) => "workspace-query",
        (false, true) => "workspace-query-light",
    };
    let c = *instantiate(backdrop).downcast::<AppWindow>().expect("AppWindow");

    let mut ctx = if on_connections {
        CommandContext::connections()
    } else {
        CommandContext::workspace()
    };
    ctx.dark = !light;
    ctx.tab_count = 2;
    ctx.editor_empty = false;
    debug_assert!(ctx.screen == Scope::Connections || ctx.screen == Scope::Workspace);

    if name.starts_with("shortcuts") {
        let mut s = ShortcutsState::default();
        s.open();
        match name {
            "shortcuts-filtered" => s.set_query("tab"),
            "shortcuts-empty" => s.set_query("zzzz"),
            _ => {}
        }
        dbhive_slint::palette_view::apply_shortcuts(&c, true, s.query(), &s.view(IS_MAC));
        return Box::new(c);
    }

    let mut p = PaletteState::default();
    p.open();
    match name {
        "palette-filtered" | "palette-light-filtered" => {
            p.set_query("tab");
            p.move_selection(1, &ctx);
        }
        "palette-theme" => p.set_query("theme"),
        "palette-window" => p.set_query("window"),
        "palette-empty" => p.set_query("zzzz"),
        "palette-bottom" => p.move_selection(-1, &ctx),
        _ => {}
    }
    let view = p.view(&ctx, IS_MAC);
    dbhive_slint::palette_view::apply_palette(&c, true, p.query(), &view);
    c.set_palette_scroll(view.reveal_offset(0.0));
    Box::new(c)
}

// -- Activity monitor and schema migrations ------------------------------------
//
// activity: queries tab populated with the details of a failed row open;
// activity-filters, activity-empty, activity-empty-filtered, activity-error,
// activity-long; activity-stats, activity-stats-empty; activity-server,
// activity-server-loading, -error, -none, -unsupported, -stale.
// migrations: migrations-select, -select-empty, -select-error,
// -select-warning, -select-busy, -diff, -diff-collapsed, -diff-empty, -sql,
// -sql-destructive, -sql-applying, -sql-success, -sql-failed,
// -sql-failed-partial. Any of them with a `-light` suffix.
// The workspace sits behind the panel, as it does in the app.

fn panel_backdrop(c: &AppWindow) {
    c.set_screen(Screen::Workspace);
    c.set_workspace_connection_name("Production Postgres".into());
    c.set_workspace_location_label("app · public".into());
    let tables: Vec<TableItem> = ["customers", "orders", "order_items", "products"]
        .iter()
        .map(|n| TableItem { name: (*n).into(), row_count: "~1240".into() })
        .collect();
    c.set_workspace_tables(slint::ModelRc::new(slint::VecModel::from(tables)));
    c.set_workspace_tabs(slint::ModelRc::new(slint::VecModel::from(vec![TabInfo {
        title: "Query 1".into(),
        query: true,
        busy: false,
    }])));
    c.set_workspace_active_tab(0);
    c.set_workspace_active_is_query(true);
}

fn activity_window(name: &str) -> Box<dyn std::any::Any> {
    use dbhive_core::models::QueryLog;
    use dbhive_core::service_monitoring as mon;
    use dbhive_slint::activity_view as view;
    use dbhive_ui_model::activity_monitor::FilterState;
    use dbhive_ui_model::activity_panel as ap;

    let light = name.ends_with("-light");
    let name = name.trim_end_matches("-light");

    let c = AppWindow::new().expect("AppWindow");
    if light {
        c.global::<Theme>().set_dark(false);
    }
    panel_backdrop(&c);
    let a = c.global::<ActivityState>();
    a.set_visible(true);
    a.set_tab(if name.starts_with("activity-stats") {
        1
    } else if name.starts_with("activity-server") {
        2
    } else {
        0
    });

    // A morning's worth of queries.
    let now = chrono::Utc::now();
    let long_sql = "SELECT c.id, c.name, c.email, SUM(o.total) AS revenue, COUNT(o.id) AS orders, MAX(o.created_at) AS last_order FROM customers c JOIN orders o ON o.customer_id = c.id WHERE o.status = 'paid' GROUP BY c.id, c.name, c.email HAVING SUM(o.total) > 1000 ORDER BY revenue DESC LIMIT 100";
    let mk = |i: usize, conn: &str, db: &str, sql: &str, mins: i64, outcome: Result<(u64, u64), (u64, &str)>| {
        let mut l = QueryLog::new(format!("log-{i}"), conn.to_lowercase(), conn.into(), Some(db.into()), sql.into());
        l.started_at = now - chrono::Duration::minutes(mins);
        match outcome {
            Ok((ms, rows)) => l.complete(ms, Some(rows)),
            Err((ms, e)) => l.fail(ms, e.to_string()),
        }
        l
    };
    let mut logs = vec![
        mk(1, "Production Postgres", "app", "SELECT * FROM customers WHERE id = 4271", 2, Ok((4, 1))),
        mk(2, "Production Postgres", "app", long_sql, 5, Ok((1840, 100))),
        mk(3, "Production Postgres", "app", "UPDATE orders SET status = 'shipped' WHERE id IN (8812, 8813)", 9, Ok((12, 2))),
        mk(4, "Production Postgres", "app", "SELECT * FROM missing_table", 14, Err((2, "relation \"missing_table\" does not exist"))),
        mk(5, "Local MySQL", "shop", "INSERT INTO carts (user_id, created_at) VALUES (17, NOW())", 22, Ok((7, 1))),
        mk(6, "Local MySQL", "shop", "SELECT COUNT(*) FROM carts", 31, Ok((3, 1))),
        mk(7, "Production Postgres", "app", "DELETE FROM sessions WHERE expires_at < now()", 47, Ok((95, 1204))),
        mk(8, "Production Postgres", "app", "CREATE INDEX idx_orders_customer ON orders (customer_id)", 63, Ok((5120, 0))),
        mk(9, "Local MySQL", "shop", "SELECT p.name, SUM(i.qty) FROM items i JOIN products p ON p.id = i.product_id GROUP BY p.name", 88, Ok((260, 48))),
    ];
    if name == "activity-long" {
        logs[0].connection_name = "A connection with a very long display name that cannot fit its column".into();
    }
    if name == "activity-empty" || name == "activity-stats-empty" || name == "activity-empty-filtered" {
        logs.clear();
    }

    let filtered = name == "activity-empty-filtered" || name == "activity-filters";
    let mut filters = FilterState::default();
    if filtered {
        filters.search_text = "orders".into();
        filters.types = vec![dbhive_core::models::QueryType::Select];
        filters.time_range = mon::TimeRange::LastHour;
    }
    a.set_filters_open(name == "activity-filters" || name == "activity-empty-filtered");
    a.set_filters_active(filtered);
    a.set_filter_connections(view_strings(&["All connections", "Production Postgres", "Local MySQL"]));
    a.set_filter_databases(view_strings(&["All databases", "app", "shop"]));
    a.set_filter_ranges(view_strings(&mon::TimeRange::ALL.map(|r| r.label())));
    a.set_filter_range(if filtered { 2 } else { 0 });
    a.set_type_chips(view::chips(
        &mon::QUERY_TYPES
            .iter()
            .map(|t| (mon::query_type_key(*t), filters.types.contains(t)))
            .collect::<Vec<_>>(),
    ));
    a.set_status_chips(view::chips(
        &mon::QUERY_STATUSES
            .iter()
            .map(|s| (ap::status_label(*s), false))
            .collect::<Vec<_>>(),
    ));
    if name == "activity-filters" {
        a.set_search("orders".into());
        a.set_min_duration("10".into());
    }

    let shown: Vec<QueryLog> = if name == "activity-empty-filtered" { Vec::new() } else { logs.clone() };
    let (columns, rows, width) = view::grid(&ap::log_grid(&shown), &ap::LOG_COLUMN_WIDTHS);
    a.set_columns(columns);
    a.set_rows(rows);
    a.set_grid_width(width);
    a.set_log_total(if name == "activity-empty-filtered" { 9 } else { logs.len() as i32 });
    a.set_summary(ap::log_summary(shown.len(), a.get_log_total() as usize, filtered).into());
    a.set_page_label(ap::page_label(0, 1, shown.len()).into());
    a.set_sort_column(0);
    a.set_sort_desc(true);
    if name == "activity-error" {
        a.set_error("Export failed: Failed to write /nonexistent/query-logs.csv: permission denied".into());
    }
    // Expand the failed query, which shows the error line too.
    if name == "activity" || name == "activity-long" {
        a.set_selected_row(3);
        let d = ap::log_detail(&logs[3]);
        a.set_has_detail(true);
        a.set_detail_status(d.status.as_str().into());
        a.set_detail_tone(view::tone(d.tone));
        a.set_detail_facts(view::facts(&d));
        a.set_detail_sql(d.sql.as_str().into());
        a.set_detail_error(d.error.as_str().into());
        a.set_can_open_in_editor(true);
    }

    // Statistics.
    let stats = mon::compute_stats(&logs);
    a.set_tiles(view::tiles(&ap::stat_tiles(&stats)));
    a.set_type_bars(view::bars(&ap::type_breakdown(&stats)));
    a.set_status_bars(view::bars(&ap::status_breakdown(&stats)));

    // Server.
    a.set_server_connections(view_strings(&["Production Postgres (PostgreSQL)", "Local MySQL (MySQL)"]));
    a.set_server_connection(0);
    let mut series = mon::MetricSeries::default();
    for i in 0..90i64 {
        let wave = ((i as f64) / 7.0).sin();
        series.push(
            1_700_000_000_000 + i * 5_000,
            &mon::ServerStats {
                numeric_connections: 24 + (wave * 6.0) as i64 + (i / 12),
                active_connections: 3 + ((wave + 1.0) * 3.0) as i64,
                cache_hit_ratio: Some(0.991),
                transactions_per_sec: Some(i as f64 * 5_000.0 / 1000.0 * (40.0 + wave * 15.0)),
                deadlocks: Some(0),
            },
        );
    }
    let latest = mon::ServerStats {
        numeric_connections: 31,
        active_connections: 5,
        cache_hit_ratio: Some(0.991),
        transactions_per_sec: Some(0.0),
        deadlocks: Some(0),
    };
    let window = series.window(Some(300_000));
    a.set_live_tiles(view::tiles(&ap::live_tiles(Some(&latest), series.latest().map(|s| s.tx_per_sec))));
    a.set_sessions_chart(view::chart(ap::sessions_chart(window).as_ref()));
    a.set_tx_chart(view::chart(ap::transactions_chart(window).as_ref()));
    a.set_updated_label("Updated 09:41:12".into());
    let p = |pid: i64, user: &str, state: &str, ms: i64, q: &str| mon::ActiveQuery {
        pid,
        user: Some(user.into()),
        database: Some("app".into()),
        client_addr: None,
        query_start: None,
        state: Some(state.into()),
        query_text: Some(q.into()),
        duration_ms: Some(ms),
    };
    let processes = vec![
        p(8812, "app", "active", 14_200, "SELECT o.id, SUM(i.qty) FROM orders o JOIN order_items i ON i.order_id = o.id GROUP BY o.id"),
        p(8830, "app", "idle in transaction", 61_400, "UPDATE carts SET updated_at = now() WHERE id = 17"),
        p(8841, "report", "active", 820, "SELECT count(*) FROM events"),
    ];
    let (pc, pr, pw) = view::grid(&ap::process_grid(&processes), &ap::PROCESS_COLUMN_WIDTHS);
    a.set_process_columns(pc);
    a.set_process_rows(pr);
    a.set_process_width(pw);
    a.set_process_summary("3 active sessions".into());
    a.set_process_selected(0);
    a.set_can_cancel(true);
    a.set_cancel_label("Cancel query 8812".into());
    a.set_server_state(match name {
        "activity-server-loading" => MonitorServerState::Loading,
        "activity-server-error" => MonitorServerState::Error,
        "activity-server-none" => MonitorServerState::None,
        "activity-server-unsupported" => MonitorServerState::Unsupported,
        _ => MonitorServerState::Ready,
    });
    a.set_server_message(match name {
        "activity-server-error" => "connection to server at \"db.internal\" (10.0.4.12), port 5432 failed: timeout expired".into(),
        "activity-server-unsupported" => "Not supported for this driver (Sqlite)".into(),
        _ => "".into(),
    });
    if name == "activity-server-stale" {
        a.set_updated_label("Last refresh failed: connection reset by peer".into());
    }
    if name == "activity-server-loading" || name == "activity-server-error" {
        a.set_updated_label("".into());
    }
    a.set_auto_refresh(name == "activity" || name.starts_with("activity-server"));
    c.show().unwrap();
    Box::new(c)
}

fn view_strings(items: &[&str]) -> slint::ModelRc<slint::SharedString> {
    slint::ModelRc::new(slint::VecModel::from(
        items.iter().map(|s| slint::SharedString::from(*s)).collect::<Vec<_>>(),
    ))
}

fn migrations_window(name: &str) -> Box<dyn std::any::Any> {
    use dbhive_core::migrations::diff::TableWithFks;
    use dbhive_core::migrations::{compute_diff, generate_migration_sql};
    use dbhive_core::models::metadata::{ColumnInfo, IndexInfo, TableInfo, TableSchema};
    use dbhive_core::models::DbDriver;
    use dbhive_core::service_migrations::ApplyResult;
    use dbhive_slint::migrations_view as view;
    use dbhive_ui_model::migrations as mg;
    use dbhive_ui_model::migrations_panel as mp;

    let light = name.ends_with("-light");
    let name = name.trim_end_matches("-light");

    let c = AppWindow::new().expect("AppWindow");
    if light {
        c.global::<Theme>().set_dark(false);
    }
    panel_backdrop(&c);
    let dark = !light;
    let m = c.global::<MigrationsState>();
    m.set_visible(true);

    // Two schemas that differ in every way the tree can show.
    let col = |n: &str, t: &str, nullable: bool| ColumnInfo::new(n.into(), t.into(), nullable);
    let table = |n: &str, cols: Vec<ColumnInfo>, idx: Vec<IndexInfo>| TableWithFks {
        schema: TableSchema::new(TableInfo::new(n.into(), "public".into(), "TABLE".into()), cols, idx),
        foreign_keys: vec![],
    };
    let source = vec![
        table("customers", vec![col("id", "integer", false), col("email", "varchar(320)", false), col("phone", "text", true)], vec![]),
        table("orders", vec![col("id", "integer", false), col("customer_id", "integer", false), col("total", "numeric(12,2)", false)], vec![]),
        table("invoices", vec![col("id", "integer", false), col("order_id", "integer", false)], vec![]),
    ];
    let target = vec![
        table("customers", vec![col("id", "integer", false), col("email", "varchar(100)", true), col("fax", "text", true)], vec![]),
        table("orders", vec![col("id", "integer", false), col("customer_id", "integer", false), col("total", "numeric(12,2)", false)], vec![]),
        table("legacy_exports", vec![col("id", "integer", false)], vec![]),
    ];
    let diff = if name == "migrations-diff-empty" { compute_diff(&source, &source) } else { compute_diff(&source, &target) };

    m.set_connections(slint::ModelRc::new(slint::VecModel::from(vec![
        slint::SharedString::from("● Staging Postgres (PostgreSQL)"),
        slint::SharedString::from("● Production Postgres (PostgreSQL)"),
        slint::SharedString::from("○ Local MySQL (MySQL)"),
    ])));

    let step = if name.starts_with("migrations-sql") {
        2
    } else if name.starts_with("migrations-diff") {
        1
    } else {
        0
    };
    m.set_step(step);

    if name == "migrations-select-empty" {
        m.set_connections(slint::ModelRc::new(slint::VecModel::from(Vec::<slint::SharedString>::new())));
    } else {
        m.set_source_index(1);
        m.set_target_index(if name == "migrations-select" || name == "migrations-select-busy" { 0 } else { 0 });
    }
    if name == "migrations-select-warning" {
        m.set_target_index(2);
        m.set_select_warning("Not connected: Local MySQL. Open it from the connections screen first.".into());
        m.set_blocked_reason("Connect first: Local MySQL".into());
    }
    if name == "migrations-select-error" {
        m.set_error("Could not compare the schemas: relation \"information_schema.tables\" is not accessible: permission denied".into());
    }
    if name == "migrations-select-busy" {
        m.set_busy(true);
        m.set_busy_label("Comparing schemas…".into());
    }

    // Diff.
    m.set_diff_empty(mg::is_empty_diff(&diff));
    m.set_diff_summary(mp::diff_summary(&diff).into());
    let expanded = if name == "migrations-diff-collapsed" { Default::default() } else { mp::default_expanded(&diff) };
    m.set_diff_rows(view::diff_rows(&mp::diff_tree(&diff, &expanded)));

    // SQL.
    let mut statements = generate_migration_sql(&diff, &DbDriver::Postgres).unwrap();
    if name == "migrations-sql" {
        statements.retain(|s| !mp::is_destructive_statement(s));
    }
    let sql = mg::statements_to_text(&statements);
    m.set_sql(sql.as_str().into());
    m.set_sql_lines(dbhive_slint::sql_highlight::to_model_for(&sql, dark));
    m.set_target_name("Staging Postgres".into());
    m.set_sql_summary(mp::sql_summary(&statements, "Staging Postgres", false).into());
    m.set_acknowledged(name != "migrations-sql");
    m.set_use_transaction(name != "migrations-sql-failed-partial");

    let outcome = match name {
        "migrations-sql-success" => Some(ApplyResult { executed: statements.len() as u32, succeeded: statements.len() as u32, ..Default::default() }),
        "migrations-sql-failed" => Some(ApplyResult {
            executed: 3,
            succeeded: 2,
            failed_statement: Some("ALTER TABLE \"public\".\"customers\" ALTER COLUMN \"email\" SET NOT NULL".into()),
            error: Some("column \"email\" of relation \"customers\" contains null values".into()),
        }),
        "migrations-sql-failed-partial" => Some(ApplyResult {
            executed: 3,
            succeeded: 2,
            failed_statement: Some("DROP TABLE \"public\".\"legacy_exports\"".into()),
            error: Some("cannot drop table legacy_exports because other objects depend on it".into()),
        }),
        _ => None,
    };
    if let Some(r) = &outcome {
        let o = mp::outcome_view(r, m.get_use_transaction());
        m.set_has_outcome(true);
        m.set_outcome_ok(o.ok);
        m.set_outcome_headline(o.headline.into());
        m.set_outcome_detail(o.detail.into());
        m.set_outcome_statement(o.failed_statement.into());
    }
    if name == "migrations-sql-applying" {
        m.set_busy(true);
        m.set_busy_label("Applying the migration…".into());
    }
    if name.starts_with("migrations-sql") && name != "migrations-sql-applying" && m.get_blocked_reason().is_empty() && !m.get_acknowledged() {
        m.set_blocked_reason("Tick the box to enable Apply".into());
    }
    if name == "migrations-diff-empty" {
        m.set_blocked_reason("Nothing to generate".into());
    }
    c.show().unwrap();
    Box::new(c)
}

// -- Import / export and backup panels ---------------------------------------
//
// import, import-loading, import-error, import-preview, import-preview-sheet,
// import-empty, import-mapping, import-mapping-new, import-mapping-empty,
// import-options, import-confirm, import-confirm-destructive, import-running,
// import-done, import-done-errors, import-sql, import-sql-running,
// import-sql-result, import-sql-error;
// export-table, export-done, export-error, export-empty, export-loading,
// export-dump, export-dump-done; each with an optional `-light`.
// backup, backup-empty, backup-loading, backup-error, backup-create,
// backup-create-error, backup-restore, backup-restore-drop, backup-progress,
// backup-done, backup-failed, backup-blocked; each with an optional `-light`.
// The workspace sits behind the panel, as it does in the app.

fn workspace_behind(c: &AppWindow, light: bool, connection: &str, location: &str) {
    if light {
        c.global::<Theme>().set_dark(false);
    }
    c.set_screen(Screen::Workspace);
    c.set_workspace_connection_name(connection.into());
    c.set_workspace_location_label(location.into());
    let tables: Vec<TableItem> = ["customers", "orders", "order_items", "products"]
        .iter()
        .map(|n| TableItem { name: (*n).into(), row_count: "~1240".into() })
        .collect();
    c.set_workspace_tables(slint::ModelRc::new(slint::VecModel::from(tables)));
    c.set_workspace_tabs(slint::ModelRc::new(slint::VecModel::from(vec![TabInfo {
        title: "Query 1".into(),
        query: true,
        busy: false,
    }])));
    c.set_workspace_active_tab(0);
    c.set_workspace_active_is_query(true);
}

fn sample_preview(rows: usize, sheets: bool) -> dbhive_core::service_import_export::ImportPreview {
    use dbhive_core::service_import_export::ImportPreview;
    let columns: Vec<String> = ["id", "full_name", "email", "plan", "signup_date", "monthly_spend"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let names = ["Ada Lovelace", "Grace Hopper", "Alan Turing", "Edsger Dijkstra", "Barbara Liskov", "Donald Knuth", "Margaret Hamilton", "Ken Thompson"];
    let data: Vec<Vec<String>> = (0..rows)
        .map(|i| {
            let n = names[i % names.len()];
            vec![
                (i + 1).to_string(),
                n.to_string(),
                format!("{}@example.com", n.to_lowercase().replace(' ', ".")),
                ["free", "pro", "team"][i % 3].to_string(),
                format!("2026-0{}-{:02}", 1 + i % 9, 1 + (i * 3) % 27),
                format!("{}.{:02}", 10 + i * 7, (i * 13) % 100),
            ]
        })
        .collect();
    ImportPreview {
        detected_types: ["INTEGER", "TEXT", "TEXT", "TEXT", "DATE", "DECIMAL"].iter().map(|s| s.to_string()).collect(),
        total_rows: sheets.then_some(940),
        file_type: if sheets { "xlsx" } else { "csv" }.to_string(),
        sheet_names: sheets.then(|| vec!["Customers".to_string(), "Churned".to_string(), "Notes".to_string()]),
        columns,
        rows: data,
    }
}

fn target_columns() -> Vec<dbhive_core::service_import_export::TableColumnInfo> {
    use dbhive_core::service_import_export::TableColumnInfo;
    let col = |n: &str, t: &str, pk: bool| TableColumnInfo {
        name: n.into(),
        data_type: t.into(),
        nullable: !pk,
        default_value: None,
        is_primary_key: pk,
    };
    vec![
        col("id", "integer", true),
        col("full_name", "text", false),
        col("email", "text", false),
        col("plan", "varchar(20)", false),
        col("signup_date", "date", false),
    ]
}

/// A wizard at the given step, filled the way a user would have filled it.
fn wizard_at(step: &str) -> dbhive_ui_model::import_wizard::ImportWizard {
    use dbhive_core::service_import_export as svc;
    use dbhive_ui_model::import_export::Step;
    use dbhive_ui_model::import_wizard::ImportWizard;

    let mut w = ImportWizard::new("public");
    w.set_file_path("/Users/me/Downloads/customers.csv");
    w.set_tables(vec!["customers".into(), "order_items".into(), "orders".into(), "products".into()]);
    if step == "file" {
        return w;
    }
    let sheets = step == "preview-sheet";
    if step == "empty" {
        let mut p = sample_preview(0, false);
        p.detected_types.clear();
        w.apply_preview(p);
        return w;
    }
    if sheets {
        w.set_file_path("/Users/me/Downloads/customers-2026.xlsx");
    }
    w.apply_preview(sample_preview(8, sheets));
    if step == "preview" || step == "preview-sheet" {
        return w;
    }
    if step == "mapping-empty" {
        w.set_tables(vec![]);
        w.step = Step::Mapping;
        return w;
    }
    if step == "mapping-new" {
        w.set_typed_table("customers_2026");
        w.step = Step::Mapping;
        return w;
    }
    w.set_target_table("customers");
    w.apply_target_columns(target_columns());
    // The user skipped one column and gave another a default.
    w.toggle_skip(0);
    w.set_mapping_default(3, "free");
    w.step = Step::Mapping;
    if step == "mapping" {
        return w;
    }
    w.step = Step::Options;
    if step == "options" || step == "options-truncate" {
        w.set_truncate(step == "options-truncate");
        return w;
    }
    // Confirm and later: a real plan from a real source.
    w.set_truncate(step == "confirm-destructive");
    w.commit_options().unwrap();
    let source = svc::ImportSource {
        columns: w.preview.as_ref().unwrap().columns.clone(),
        rows: {
            let p = sample_preview(8, false);
            let mut rows = Vec::new();
            for _ in 0..30 {
                rows.extend(p.rows.clone());
            }
            rows
        },
    };
    w.options.batch_size = 100;
    let plan = svc::build_import_plan(&dbhive_core::models::DbDriver::Postgres, &source, &w.options).unwrap();
    w.apply_plan(source, plan);
    w
}

fn import_export_window(name: &str) -> Box<dyn std::any::Any> {
    use dbhive_core::service_import_export::{ImportResult, SqlImportResult};
    use dbhive_slint::import_export_view::{project, set_preview, DataSession};

    let light = name.ends_with("-light");
    let name = name.trim_end_matches("-light");
    let c = AppWindow::new().expect("AppWindow");
    workspace_behind(&c, light, "Production Postgres", "app · public");
    let g = c.global::<ImportExportState>();
    g.set_visible(true);

    let mut d = DataSession::default();
    d.wizard = wizard_at("file");
    d.sql.path = String::new();
    let mut dump_note = true;

    let step = name.strip_prefix("import-");
    match step {
        None => {}
        Some("loading") => d.busy = "Reading the file…".into(),
        Some("error") => {
            d.import_path_error = String::new();
            d.wizard.error = "Could not read the file: Failed to open CSV: No such file or directory (os error 2)".into();
        }
        Some("preview") => d.wizard = wizard_at("preview"),
        Some("preview-sheet") => d.wizard = wizard_at("preview-sheet"),
        Some("empty") => d.wizard = wizard_at("empty"),
        Some("mapping") => d.wizard = wizard_at("mapping"),
        Some("mapping-new") => d.wizard = wizard_at("mapping-new"),
        Some("mapping-empty") => d.wizard = wizard_at("mapping-empty"),
        Some("mapping-loading") => {
            d.wizard = wizard_at("mapping");
            d.columns_loading = true;
        }
        Some("options") => d.wizard = wizard_at("options"),
        Some("options-truncate") => d.wizard = wizard_at("options-truncate"),
        Some("confirm") => d.wizard = wizard_at("confirm"),
        Some("confirm-destructive") => {
            d.wizard = wizard_at("confirm-destructive");
            d.wizard.confirm_input = "custom".into();
        }
        Some("running") => {
            d.wizard = wizard_at("confirm");
            d.wizard.import_started();
        }
        Some("done") | Some("done-errors") => {
            d.wizard = wizard_at("confirm");
            d.wizard.import_started();
            let r = if step == Some("done") {
                ImportResult { rows_imported: 240, rows_failed: 0, errors: vec![], success: true }
            } else {
                ImportResult {
                    rows_imported: 232,
                    rows_failed: 8,
                    errors: (0..8)
                        .map(|i| format!("Row {}: duplicate key value violates unique constraint \"customers_email_key\" (email)=(ada.lovelace@example.com)", 17 + i * 30))
                        .collect(),
                    success: false,
                }
            };
            d.wizard.import_finished(r);
        }
        Some("sql") | Some("sql-running") | Some("sql-result") | Some("sql-error") | Some("sql-partial") => {
            d.tab = 1;
            d.sql.path = "/Users/me/Downloads/shop_2026-10-21.sql".into();
            match step {
                Some("sql-running") => d.sql_running = true,
                Some("sql-result") => {
                    d.sql_result = Some(SqlImportResult { executed: 1840, errors_count: 0, skipped: 12, first_error: None, cancelled: false, log_file: None });
                }
                Some("sql-partial") => {
                    d.sql.continue_on_error = true;
                    d.sql_result = Some(SqlImportResult {
                        executed: 1790,
                        errors_count: 3,
                        skipped: 12,
                        first_error: Some("ERROR: relation \"order_items\" already exists (statement 214)".into()),
                        cancelled: false,
                        log_file: Some("/Users/me/Downloads/shop_2026-10-21_import_errors.log".into()),
                    });
                }
                Some("sql-error") => {
                    d.sql_path_error = "No file exists at that path.".into();
                    d.sql_error = "The SQL file could not be run: Failed to open SQL file: No such file or directory (os error 2)".into();
                }
                _ => {}
            }
        }
        _ => {}
    }

    // Export and dump tabs.
    if name.starts_with("export") {
        d.wizard = wizard_at("file");
        d.tab = if name.starts_with("export-dump") { 3 } else { 2 };
        d.export.table = "customers".into();
        d.export.path = "/Users/me/Downloads/customers_2026-10-21.csv".into();
        d.dump.path = "/Users/me/Downloads/app_2026-10-21.sql".into();
        match name {
            "export-done" => d.export_result = "Exported 1,240 rows to /Users/me/Downloads/customers_2026-10-21.csv".into(),
            "export-error" => d.export_error = "Export failed: permission denied for table customers".into(),
            "export-empty" => d.wizard.set_tables(vec![]),
            "export-loading" => {
                d.wizard.set_tables(vec![]);
                d.tables_loading = true;
            }
            "export-running" => d.export_running = true,
            "export-dump-done" => d.dump_result = "Dumped 4 tables to /Users/me/Downloads/app_2026-10-21.sql".into(),
            "export-dump-error" => d.dump_error = "The folder does not exist.".into(),
            "export-dump" => {
                d.dump.include_drop = true;
                d.dump.tables_text = "customers, orders".into();
            }
            _ => {}
        }
        if name.starts_with("export-dump") {
            dump_note = false;
        }
    }
    let _ = dump_note;

    let driver = dbhive_core::models::DbDriver::Postgres;
    project(&g, &d, "Production Postgres · public", Some(&driver));
    set_preview(&g, &d.wizard);
    g.set_type_options(dbhive_slint::import_export_view::type_options());
    g.set_import_path(d.wizard.file_path.clone().into());
    g.set_sql_path(d.sql.path.clone().into());
    g.set_export_path(d.export.path.clone().into());
    g.set_dump_path(d.dump.path.clone().into());
    g.set_dump_tables(d.dump.tables_text.clone().into());
    if name == "import-mapping-new" {
        g.set_new_table_text("customers_2026".into());
    }
    if name.starts_with("import-confirm-destructive") {
        g.set_confirm_text(d.wizard.confirm_input.clone().into());
    }
    c.show().unwrap();
    Box::new(c)
}

fn backup_entries() -> Vec<dbhive_core::models::backup::BackupEntry> {
    use dbhive_core::models::backup::{BackupEntry, BackupStatus};
    let now = chrono::Utc::now().timestamp();
    let e = |file: &str, size: u64, mins: i64, note: Option<&str>, status: BackupStatus| BackupEntry {
        id: file.into(),
        connection_id: "c1".into(),
        connection_name: "Production Postgres".into(),
        driver: "Postgres".into(),
        file_path: format!("/Users/me/Library/Application Support/com.kwamina.db-hive/backups/{file}"),
        file_name: file.into(),
        size_bytes: size,
        created_at: now - mins * 60,
        status,
        note: note.map(str::to_string),
    };
    vec![
        e("Production_Postgres_1761043800.sql", 48_234_112, 12, Some("before the orders migration"), BackupStatus::Completed),
        e("Production_Postgres_1760957400.sql", 47_901_888, 60 * 24, None, BackupStatus::Completed),
        e("Production_Postgres_1760871000.sql", 0, 60 * 48, None, BackupStatus::Failed("pg_dump: error: connection to server at \"db.internal\" (10.0.4.12), port 5432 failed: Connection refused".into())),
        e("Production_Postgres_1760266200.sql", 46_120_448, 60 * 24 * 9, Some("quarterly"), BackupStatus::Completed),
        e("a_connection_with_a_very_long_name_that_must_be_elided_rather_than_overflow_the_row_1759000000.sql", 1_024, 60 * 24 * 30, None, BackupStatus::Completed),
    ]
}

fn backup_window(name: &str) -> Box<dyn std::any::Any> {
    use dbhive_core::models::backup::BackupProgress;
    use dbhive_slint::backup_view::{project, Context};
    use dbhive_ui_model::backup_manager::{BackupManager, OpKind, View};

    let light = name.ends_with("-light");
    let name = name.trim_end_matches("-light");
    let c = AppWindow::new().expect("AppWindow");
    workspace_behind(&c, light, "Production Postgres", "app · public");
    let g = c.global::<BackupState>();
    g.set_visible(true);

    let mut m = BackupManager::default();
    m.directory = "/Users/me/Library/Application Support/com.kwamina.db-hive/backups".into();
    let mut ctx = Context {
        location: "Production Postgres · Postgres".into(),
        driver: Some(dbhive_core::models::DbDriver::Postgres),
        target_name: "Production Postgres".into(),
        database_label: "app".into(),
        blocker: String::new(),
    };
    if name != "backup-empty" && name != "backup-loading" && name != "backup-error" {
        m.entries = backup_entries();
    }
    let progress = |stage: &str, msg: &str, done: bool, error: Option<&str>| BackupProgress {
        stage: stage.into(),
        message: msg.into(),
        done,
        error: error.map(str::to_string),
    };
    match name {
        "backup-loading" => m.loading = true,
        "backup-error" => m.list_error = "Could not read the backup folder: Cannot read backup dir: Permission denied (os error 13)".into(),
        "backup-filtered" => m.filter = "quarterly".into(),
        "backup-blocked" => {
            ctx.driver = Some(dbhive_core::models::DbDriver::Redis);
            ctx.location = "Cache · Redis".into();
            ctx.blocker = "Backups are not supported for Redis connections.".into();
        }
        "backup-create" | "backup-create-error" | "backup-create-sqlite" => {
            m.view = View::Create;
            m.form.note = "before the orders migration".into();
            if name == "backup-create-error" {
                m.form_error = "Select at least schema or data; a backup with neither is empty".into();
                m.form.include_schema = false;
                m.form.include_data = false;
            }
            if name == "backup-create-sqlite" {
                ctx.driver = Some(dbhive_core::models::DbDriver::Sqlite);
                ctx.location = "Widgets · Sqlite".into();
            }
        }
        "backup-restore" | "backup-restore-drop" | "backup-restore-sqlite" => {
            // A restore needs the file to exist; make one next to the output.
            let mut entry = m.entries[0].clone();
            if let Some(dir) = std::env::args().nth(2).and_then(|o| std::path::Path::new(&o).parent().map(|p| p.to_path_buf())) {
                let file = dir.join(&entry.file_name);
                std::fs::write(&file, b"-- snapshot placeholder").unwrap();
                entry.file_path = file.to_string_lossy().to_string();
            }
            m.open_restore(entry);
            if name == "backup-restore-drop" {
                m.drop_existing = true;
            }
            if name == "backup-restore-sqlite" {
                ctx.driver = Some(dbhive_core::models::DbDriver::Sqlite);
                ctx.location = "Widgets · Sqlite".into();
                ctx.database_label = "widgets.sqlite".into();
            }
        }
        "backup-progress" | "backup-restore-progress" => {
            let kind = if name == "backup-progress" { OpKind::Backup } else { OpKind::Restore };
            m.begin(kind);
            let op = m.op.as_mut().unwrap();
            op.apply(&progress("dumping", "Running pg_dump…", false, None));
            op.elapsed = Some(std::time::Duration::from_millis(7400));
        }
        "backup-done" => {
            m.begin(OpKind::Backup);
            let op = m.op.as_mut().unwrap();
            op.apply(&progress("done", "Wrote Production_Postgres_1761043800.sql", true, None));
            op.message = "Backup written to /Users/me/Library/Application Support/com.kwamina.db-hive/backups/Production_Postgres_1761043800.sql".into();
            op.elapsed = Some(std::time::Duration::from_millis(12_800));
        }
        "backup-failed" => {
            m.begin(OpKind::Restore);
            let op = m.op.as_mut().unwrap();
            op.apply(&progress("done", "psql failed", true, Some("psql: error: connection to server at \"db.internal\" (10.0.4.12), port 5432 failed: Connection refused")));
            op.elapsed = Some(std::time::Duration::from_millis(900));
        }
        _ => {}
    }

    project(&g, &m, &ctx);
    g.set_note(m.form.note.clone().into());
    g.set_filter(m.filter.clone().into());
    c.show().unwrap();
    Box::new(c)
}

// -- Visual query builder and table dialogs ------------------------------------
//
// Builder variants: builder, builder-light, builder-empty, builder-loading,
// builder-error, builder-open (every section open), builder-open-light.
// Table dialog variants (each also with -light): tabledialog-basics,
// tabledialog-basics-empty, tabledialog-columns, tabledialog-constraints,
// tabledialog-preview, tabledialog-preview-error, tabledialog-edit,
// tabledialog-edit-preview, tabledialog-edit-loading, tabledialog-edit-error,
// tabledialog-edit-failed, tabledialog-edit-done.
// The workspace sits behind the panel, as it does in the app.

fn builder_workspace_behind(light: bool, loading: bool) -> AppWindow {
    let c = AppWindow::new().expect("AppWindow");
    if light {
        c.global::<Theme>().set_dark(false);
    }
    c.set_screen(Screen::Workspace);
    c.set_workspace_connection_name("Production Postgres".into());
    c.set_workspace_location_label("app · public".into());
    let tables: Vec<TableItem> = ["customers", "orders", "order_items", "products"]
        .iter()
        .map(|n| TableItem { name: (*n).into(), row_count: "~1240".into() })
        .collect();
    c.set_workspace_loading_tables(loading);
    if !loading {
        c.set_workspace_tables(slint::ModelRc::new(slint::VecModel::from(tables)));
    }
    c.set_workspace_tabs(slint::ModelRc::new(slint::VecModel::from(vec![TabInfo {
        title: "Query 1".into(),
        query: true,
        busy: false,
    }])));
    c.set_workspace_active_tab(0);
    c.set_workspace_active_is_query(true);
    c
}

fn builder_window(name: &str) -> Box<dyn std::any::Any> {
    use dbhive_core::models::ForeignKeyInfo;
    use dbhive_ui_model::builder_editor::{BuilderEditor, Change};
    use dbhive_ui_model::query_builder::rules_for;

    let light = name.ends_with("-light");
    let name = name.trim_end_matches("-light");
    let c = builder_workspace_behind(light, name == "builder-loading");
    let dark = !light;

    let q = |x: &str| format!("\"{}\"", x.replace('"', "\"\""));
    let esc = |x: &str| x.replace('\'', "''");
    let pg = rules_for(&dbhive_core::models::DbDriver::Postgres);
    let cols = |l: &[(&str, &str)]| l.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
    let catalog: Vec<String> =
        ["customers", "orders", "order_items", "products", "refunds"].iter().map(|s| s.to_string()).collect();
    let keys = vec![ForeignKeyInfo {
        name: "orders_customer_fk".into(),
        table: "orders".into(),
        schema: "public".into(),
        columns: vec!["customer_id".into()],
        referenced_table: "customers".into(),
        referenced_schema: "public".into(),
        referenced_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
    }];

    let mut e = BuilderEditor::new();
    if name != "builder-empty" && name != "builder-loading" {
        e.add_table("public", "customers", cols(&[("id", "integer"), ("name", "text"), ("plan", "text"), ("active", "boolean")]));
        e.add_table("public", "orders", cols(&[("id", "integer"), ("customer_id", "integer"), ("total", "numeric"), ("status", "text")]));
        let k = &keys;
        if name != "builder-error" {
            e.apply("join-suggest", 0, 0, "", k);
            e.apply("col-add", 0, 2, "", k); // customers.name
            e.apply("col-add", 1, 3, "", k); // orders.total
            e.apply("col-agg", 1, 2, "", k); // SUM
            e.apply("col-alias", 1, 0, "revenue", k);
            e.apply("f-cond-add", 0, 0, "", k);
            e.apply("f-col", 1, 2, "", k); // plan
            e.apply("f-val", 1, 0, "pro", k);
            e.apply("f-group-add", 0, 0, "", k);
            e.apply("f-cond-add", 2, 0, "", k);
            e.apply("f-alias", 3, 1, "", k); // orders
            e.apply("f-col", 3, 2, "", k); // total
            e.apply("f-val", 3, 0, "100", k);
            e.apply("f-cond-add", 2, 0, "", k);
            e.apply("f-alias", 4, 1, "", k);
            e.apply("f-col", 4, 3, "", k); // status
            e.apply("f-val", 4, 0, "paid", k);
            e.apply("f-op", 2, 0, "", k); // OR
            e.apply("gb-add", 0, 1, "", k);
            e.apply("ob-add", 0, 1, "", k);
            e.apply("limit", 0, 0, "25", k);
        } else {
            // A half-built query with problems to warn about.
            e.apply("join-add", 0, 0, "", k);
            e.apply("col-add", 0, 1, "", k);
            e.apply("col-add", 0, 2, "", k);
            e.apply("gb-add", 0, 0, "", k);
            e.apply("limit", 0, 0, "lots", k);
        }
        if name == "builder-open" {
            // The upper sections fold away so the lower ones fit on screen.
            for sec in [0, 1, 2, 4, 5, 6] {
                e.apply("section", sec, 0, "", k);
            }
            e.apply("hv-add", 0, 0, "", k);
            e.apply("hv-val", 0, 0, "1000", k);
        }
    }

    let view = e.view(&catalog, &keys, &q, &esc, pg);
    let b = c.global::<BuilderState>();
    b.set_visible(true);
    b.set_scope_label("Production Postgres / app".into());
    b.set_loading(name == "builder-loading");
    if name == "builder-loading" {
        b.set_pending_note("Reading the columns of orders…".into());
    }
    if name == "builder-error" {
        b.set_error("Could not read the columns of refunds: relation \"public.refunds\" does not exist".into());
        b.set_error_retry(true);
    }
    let can_use = !view.sql.is_empty() && view.limit_error.is_empty() && view.offset_error.is_empty();
    dbhive_slint::builder_view::apply(&c, &view, Change::Structure, can_use, dark);
    dbhive_slint::builder_view::reset_fields(&c, &view);
    c.show().unwrap();
    Box::new(c)
}

fn table_dialog_window(name: &str) -> Box<dyn std::any::Any> {
    use dbhive_core::models::{ColumnInfo, DbDriver, TableInfo, TableSchema};
    use dbhive_ui_model::builder_editor::Change;
    use dbhive_ui_model::table_dialog::{report, AlterEditor, CreateWizard, RefColumns};

    let light = name.ends_with("-light");
    let name = name.trim_end_matches("-light");
    let c = builder_workspace_behind(light, false);
    let dark = !light;
    let pg = DbDriver::Postgres;
    let tables: Vec<String> = ["customers", "orders", "order_items", "products"].iter().map(|s| s.to_string()).collect();

    let d = c.global::<TableDialogState>();
    d.set_visible(true);

    if name.starts_with("tabledialog-edit") {
        let mut id = ColumnInfo::new("id".into(), "integer".into(), false);
        id.is_primary_key = true;
        id.is_auto_increment = true;
        let schema = TableSchema::new(
            TableInfo::new("customers".into(), "public".into(), "TABLE".into()),
            vec![
                id,
                ColumnInfo::new("name".into(), "character varying(120)".into(), false),
                ColumnInfo::new("email".into(), "text".into(), true),
                ColumnInfo::new("plan".into(), "text".into(), true),
                ColumnInfo::new("legacy_code".into(), "integer".into(), true),
            ],
            vec![],
        );
        let mut e = AlterEditor::from_schema("public", "customers", &schema);
        match name {
            "tabledialog-edit-loading" => {
                d.set_loading(true);
                d.set_mode(1);
                d.set_title("Edit table".into());
                d.set_subtitle("public.customers".into());
            }
            "tabledialog-edit-error" => {
                d.set_load_error("Could not read customers: permission denied for table customers".into());
                d.set_mode(1);
                d.set_title("Edit table".into());
                d.set_subtitle("public.customers".into());
            }
            _ => {
                if name != "tabledialog-edit-clean" {
                    e.apply("col-name", 2, 0, "contact_email");
                    e.apply("col-null", 3, 0, "");
                    e.apply("col-remove", 4, 0, "");
                    e.apply("col-add", 0, 0, "");
                    e.apply("col-name", 5, 0, "signup_date");
                    e.apply("col-type", 5, 5, "");
                    // The key cannot be dropped; this is refused and shows.
                    e.apply("col-remove", 0, 0, "");
                }
                let mut rep = Vec::new();
                if name == "tabledialog-edit-preview" || name == "tabledialog-edit-failed" || name == "tabledialog-edit-done" {
                    e.step = 1;
                }
                let stmts = e.statements(&pg).unwrap_or_default();
                if name == "tabledialog-edit-failed" {
                    rep = report(&stmts, &[Ok(()), Err("column \"signup_date\" of relation \"customers\" already exists".into())]);
                }
                if name == "tabledialog-edit-done" {
                    let ok: Vec<Result<(), String>> = stmts.iter().map(|_| Ok(())).collect();
                    rep = report(&stmts, &ok);
                }
                let v = e.view(&pg);
                dbhive_slint::table_dialog_view::apply(&c, &v, Change::Structure, &rep, dark);
            }
        }
    } else {
        let mut w = CreateWizard::new("public");
        if name != "tabledialog-basics-empty" {
            w.apply("name", 0, 0, "invoices", &tables);
            w.apply("desc", 0, 0, "One row per invoice sent to a customer", &tables);
        }
        if name == "tabledialog-basics" || name == "tabledialog-basics-empty" {
            // Step one as it opens.
        } else {
            w.apply("col-add", 0, 0, "", &tables);
            w.apply("col-name", 1, 0, "customer_id", &tables);
            w.apply("col-type", 1, 1, "", &tables); // BIGINT
            w.apply("col-null", 1, 0, "", &tables);
            w.apply("col-add", 0, 0, "", &tables);
            w.apply("col-name", 2, 0, "reference", &tables);
            w.apply("col-type", 2, 2, "", &tables); // VARCHAR
            w.apply("col-param", 2, 0, "40", &tables);
            w.apply("col-add", 0, 0, "", &tables);
            w.apply("col-name", 3, 0, "amount", &tables);
            w.apply("col-type", 3, 7, "", &tables); // DECIMAL
            w.apply("col-add", 0, 0, "", &tables);
            w.apply("col-name", 4, 0, "issued_at", &tables);
            w.apply("col-type", 4, 6, "", &tables); // TIMESTAMP
            w.step = 1;
        }
        if name == "tabledialog-constraints" || name.starts_with("tabledialog-preview") {
            w.apply("uq-add", 0, 0, "", &tables);
            w.apply("uq-name", 0, 0, "uq_invoice_reference", &tables);
            w.apply("uq-col", 0, 2, "", &tables); // reference
            w.apply("fk-add", 0, 0, "", &tables);
            w.apply("fk-col", 0, 1, "", &tables); // customer_id
            w.apply("fk-table", 0, 0, "", &tables); // customers
            w.ref_columns.insert("customers".into(), RefColumns::Loaded(vec!["id".into(), "name".into(), "plan".into()]));
            w.apply("fk-rcol", 0, 0, "", &tables);
            w.apply("fk-del", 0, 2, "", &tables);
            w.step = 2;
        }
        if name.starts_with("tabledialog-preview") {
            if name == "tabledialog-preview-error" {
                w.apply("name", 0, 0, "", &tables);
            }
            w.step = 3;
        }
        let v = w.view(&pg, &tables);
        dbhive_slint::table_dialog_view::apply(&c, &v, Change::Structure, &[], dark);
    }
    c.show().unwrap();
    Box::new(c)
}



// -- ER diagram and schema designer ---------------------------------------------
//
// Variants: er, er-light, er-selected, er-zoomed, er-filter, er-collapsed,
// er-no-relationships, er-loading, er-error, er-empty, er-export,
// er-export-exists; designer, designer-light, designer-empty,
// designer-applying, designer-problems, designer-varchar, designer-nosel,
// designer-preview, designer-preview-light, designer-result.
//
// Both build their pictures with the same projection functions the app uses
// (`erd_view::project`, `designer_view::project`), so a snapshot is what the
// controller would show. The canvas size is the window less the title bar,
// toolbar and legend strip.

fn canvas_size() -> (f32, f32) {
    let args: Vec<String> = std::env::args().collect();
    let w: f32 = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(1200.0);
    let h: f32 = args.get(4).and_then(|v| v.parse().ok()).unwrap_or(760.0);
    // The software renderer draws the menu bar inside the window (about 40px);
    // the native macOS menu bar takes no window space.
    (w, h - 40.0 - 48.0 - 44.0 - 30.0)
}

fn er_col(name: &str, ty: &str, pk: bool) -> dbhive_core::models::ColumnInfo {
    dbhive_core::models::ColumnInfo {
        name: name.into(),
        data_type: ty.into(),
        nullable: !pk,
        default_value: None,
        is_primary_key: pk,
        is_auto_increment: false,
    }
}

fn er_fk(table: &str, col: &str, parent: &str) -> dbhive_core::models::ForeignKeyInfo {
    dbhive_core::models::ForeignKeyInfo {
        name: format!("fk_{}_{}", table, col),
        table: table.into(),
        schema: "public".into(),
        columns: vec![col.into()],
        referenced_table: parent.into(),
        referenced_schema: "public".into(),
        referenced_columns: vec!["id".into()],
        on_delete: None,
        on_update: None,
    }
}

fn sample_er_scene() -> dbhive_ui_model::erd_scene::ErdScene {
    use dbhive_ui_model::er_diagram::DiagramTable;
    let t = |name: &str, cols: Vec<dbhive_core::models::ColumnInfo>| DiagramTable { name: name.into(), columns: cols };
    let tables = vec![
        t("customers", vec![er_col("id", "integer", true), er_col("email", "varchar(255)", false), er_col("name", "text", false), er_col("created_at", "timestamp", false)]),
        t("addresses", vec![er_col("id", "integer", true), er_col("customer_id", "integer", false), er_col("line1", "varchar(120)", false), er_col("city", "varchar(80)", false), er_col("country", "char(2)", false)]),
        t("products", vec![er_col("id", "integer", true), er_col("sku", "varchar(32)", false), er_col("name", "varchar(120)", false), er_col("price", "numeric(10,2)", false), er_col("active", "boolean", false)]),
        t("categories", vec![er_col("id", "integer", true), er_col("name", "text", false), er_col("parent_id", "integer", false)]),
        t("product_categories", vec![er_col("product_id", "integer", true), er_col("category_id", "integer", true)]),
        t("orders", vec![er_col("id", "integer", true), er_col("customer_id", "integer", false), er_col("address_id", "integer", false), er_col("status", "varchar(20)", false), er_col("total", "numeric(12,2)", false), er_col("placed_at", "timestamp", false)]),
        t("order_items", vec![er_col("id", "integer", true), er_col("order_id", "integer", false), er_col("product_id", "integer", false), er_col("quantity", "integer", false), er_col("unit_price", "numeric(10,2)", false)]),
        t("payments", vec![er_col("id", "integer", true), er_col("order_id", "integer", false), er_col("provider", "varchar(40)", false), er_col("amount", "numeric(12,2)", false), er_col("paid_at", "timestamp", false)]),
        t("refunds", vec![er_col("id", "integer", true), er_col("payment_id", "integer", false), er_col("amount", "numeric(12,2)", false), er_col("reason", "text", false)]),
        t("audit_log", (0..14).map(|i| er_col(if i == 0 { "id" } else { ["actor_id", "action", "entity", "entity_id", "before", "after", "ip", "user_agent", "session_id", "request_id", "created_at", "retention_until", "checksum"][i - 1] }, if i == 0 { "bigint" } else { "text" }, i == 0)).collect()),
        t("a_table_with_a_very_long_name_that_must_be_elided_not_overflow_0001", vec![er_col("id", "integer", true), er_col("a_column_with_a_very_long_name_that_must_be_elided_too", "text", false)]),
    ];
    let fks = vec![
        er_fk("addresses", "customer_id", "customers"),
        er_fk("categories", "parent_id", "categories"),
        er_fk("product_categories", "product_id", "products"),
        er_fk("product_categories", "category_id", "categories"),
        er_fk("orders", "customer_id", "customers"),
        er_fk("orders", "address_id", "addresses"),
        er_fk("order_items", "order_id", "orders"),
        er_fk("order_items", "product_id", "products"),
        er_fk("payments", "order_id", "orders"),
        er_fk("refunds", "payment_id", "payments"),
    ];
    dbhive_ui_model::erd_scene::ErdScene::new(tables, fks, &std::collections::HashMap::new())
}

/// The workspace behind an overlay, the way the other panels snapshot it.
fn erd_workspace_behind(c: &AppWindow, light: bool) {
    if light {
        c.global::<Theme>().set_dark(false);
    }
    c.set_screen(Screen::Workspace);
    c.set_workspace_connection_name("Production Postgres".into());
    c.set_workspace_location_label("app \u{b7} public".into());
}

fn er_window(name: &str) -> Box<dyn std::any::Any> {
    use dbhive_slint::diagram_model::Diagram;
    use dbhive_ui_model::erd_scene::{fit, Flow, View};

    let light = name.ends_with("-light");
    let name = name.trim_end_matches("-light");
    let c = AppWindow::new().expect("AppWindow");
    erd_workspace_behind(&c, light);

    let mut scene = sample_er_scene();
    let orders = scene.tables.iter().position(|t| t.name == "orders").unwrap();
    match name {
        "er-selected" | "er-zoomed" => scene.selected = Some(orders),
        "er-filter" => scene.filter = "order".into(),
        "er-collapsed" => scene.set_show_columns(false),
        "er-no-relationships" => scene.options.show_relationships = false,
        _ => {}
    }
    let mut diagram = Diagram::new(Vec::new(), Vec::new()).with_flow(Flow::Vertical, true);
    diagram.hide_edges = !scene.options.show_relationships;
    diagram.selected = scene.selected;
    diagram.replace(dbhive_slint::erd_view::nodes(&scene), dbhive_slint::erd_view::links(&scene));

    let (vw, vh) = canvas_size();
    let view = if name == "er-zoomed" {
        let r = scene.rect(orders);
        View { zoom: 1.0, pan_x: 0.0, pan_y: 0.0 }.center_on(r.cx(), r.cy(), vw, vh)
    } else {
        fit(scene.bounds().unwrap(), vw, vh)
    };

    let st = c.global::<ErdState>();
    st.set_visible(true);
    let (loading, error) = match name {
        "er-loading" => (true, "Could not read the schema: x"),
        "er-error" => (false, "Could not read the schema: permission denied for schema public"),
        _ => (false, ""),
    };
    let error = if name == "er-error" { error } else { "" };
    let scene_ref = if name == "er-loading" || name == "er-error" || name == "er-empty" {
        None
    } else {
        Some(&scene)
    };
    let empty_scene = dbhive_ui_model::erd_scene::ErdScene::new(vec![], vec![], &std::collections::HashMap::new());
    let scene_ref = if name == "er-empty" { Some(&empty_scene) } else { scene_ref };
    dbhive_slint::erd_view::project(&st, scene_ref, &diagram, view.zoom, loading, error, "public");
    st.set_pan_x(view.pan_x);
    st.set_pan_y(view.pan_y);
    st.set_viewport_width(vw);
    st.set_viewport_height(vh);
    if name.starts_with("er-export") {
        st.set_export_open(true);
        st.set_export_path("/Users/me/Downloads/public-erd.svg".into());
        if name == "er-export-exists" {
            st.set_export_ok(false);
            st.set_export_message("That file already exists. Save again to replace it.".into());
        }
    }
    c.show().unwrap();
    Box::new((c, diagram))
}

fn designer_window(name: &str) -> Box<dyn std::any::Any> {
    use dbhive_slint::diagram_model::Diagram;
    use dbhive_ui_model::designer_session::{action_index, type_index, Designer, Outcome, TableResult};
    use dbhive_ui_model::erd_scene::{fit, Flow};
    use dbhive_ui_model::table_editor::TypeKind;

    let light = name.ends_with("-light");
    let name = name.trim_end_matches("-light");
    let c = AppWindow::new().expect("AppWindow");
    erd_workspace_behind(&c, light);

    let mut d = Designer::new();
    if name != "designer-empty" {
        let users = d.add_table();
        d.rename_table(users, "users");
        let email = d.add_column(users).unwrap();
        d.rename_column(users, email, "email");
        d.set_column_type(users, email, type_index(TypeKind::Varchar));
        d.toggle_nullable(users, email);
        d.toggle_unique(users, email);

        let products = d.add_table();
        d.rename_table(products, "products");
        let title = d.add_column(products).unwrap();
        d.rename_column(products, title, "title");
        d.set_column_type(products, title, type_index(TypeKind::Text));
        let price = d.add_column(products).unwrap();
        d.rename_column(products, price, "price");
        d.set_column_type(products, price, type_index(TypeKind::Decimal));

        let orders = d.add_table();
        d.rename_table(orders, "orders");
        let user_id = d.add_column(orders).unwrap();
        d.rename_column(orders, user_id, "user_id");
        d.set_column_type(orders, user_id, type_index(TypeKind::BigInt));
        d.set_reference_table(orders, user_id, "users");
        d.set_reference_column(orders, user_id, "id");
        d.set_on_delete(orders, user_id, action_index(&dbhive_core::models::ForeignKeyAction::Cascade));
        let placed = d.add_column(orders).unwrap();
        d.rename_column(orders, placed, "placed_at");
        d.set_column_type(orders, placed, type_index(TypeKind::Timestamp));

        let items = d.add_table();
        d.rename_table(items, "order_items");
        let order_id = d.add_column(items).unwrap();
        d.rename_column(items, order_id, "order_id");
        d.set_column_type(items, order_id, type_index(TypeKind::BigInt));
        d.set_reference_table(items, order_id, "orders");
        let product_id = d.add_column(items).unwrap();
        d.rename_column(items, product_id, "product_id");
        d.set_column_type(items, product_id, type_index(TypeKind::BigInt));
        d.set_reference_table(items, product_id, "products");
        d.auto_layout();

        match name {
            "designer-nosel" => d.select_index(None),
            "designer-varchar" => {
                d.select_index(Some(0));
                d.select_column(Some(email_index(&d)));
            }
            "designer-problems" => {
                let extra = d.add_table(); // unnamed
                let _ = extra;
                d.select_index(Some(3));
                d.select_column(Some(1));
                d.set_reference_column(items, order_id, "missing");
            }
            _ => {
                d.select_index(Some(2));
                d.select_column(Some(1));
            }
        }
    }

    let mut diagram = Diagram::new(Vec::new(), Vec::new()).with_flow(Flow::Horizontal, true);
    diagram.selected = d.selected_index();
    diagram.replace(dbhive_slint::designer_view::nodes(&d), dbhive_slint::designer_view::links(&d));
    let (vw, vh) = canvas_size();
    let view = match d.bounds() {
        Some(b) => fit(b, vw - 340.0, vh),
        None => Default::default(),
    };

    let st = c.global::<DesignerState>();
    st.set_visible(true);
    dbhive_slint::designer_view::project(&st, &d, &diagram, view.zoom, name == "designer-applying", "Sqlite");
    st.set_pan_x(view.pan_x);
    st.set_pan_y(view.pan_y);
    st.set_viewport_width(vw - 340.0);
    st.set_viewport_height(vh);

    if name.starts_with("designer-preview") {
        let plan = d.plan(&dbhive_core::models::DbDriver::Sqlite, "").unwrap();
        st.set_preview_sql(dbhive_ui_model::designer_session::script(&plan).into());
        st.set_preview_open(true);
    }
    if name == "designer-result" {
        let results = vec![
            TableResult { name: "users".into(), outcome: Outcome::Created },
            TableResult { name: "products".into(), outcome: Outcome::Created },
            TableResult { name: "orders".into(), outcome: Outcome::Failed("table \"orders\" already exists".into()) },
            TableResult { name: "order_items".into(), outcome: Outcome::Skipped("orders was not created".into()) },
        ];
        st.set_results(dbhive_slint::designer_view::result_rows(&results));
        st.set_results_summary(dbhive_ui_model::designer_session::results_summary(&results).into());
        st.set_preview_open(true);
    }
    c.show().unwrap();
    Box::new((c, diagram))
}

fn email_index(d: &dbhive_ui_model::designer_session::Designer) -> usize {
    d.tables[0].columns.iter().position(|c| c.base.name == "email").unwrap_or(0)
}

// -- Plugin manager ---------------------------------------------------------
//
// plugins (installed, one selected), plugins-disabled, plugins-interface (a
// plugin's own interface open), plugins-interface-error, plugins-settings,
// plugins-settings-error, plugins-busy, plugins-empty, plugins-loading,
// plugins-error, plugins-problems, plugins-long; plugins-market,
// plugins-market-filtered, plugins-market-empty, plugins-market-loading,
// plugins-market-error; plugins-toolbar (panel closed, buttons in the
// workspace header), plugins-toolbar-connections. Any of them with a `-light`
// suffix. The workspace sits behind the panel, as it does in the app.

const PLUGINS_MANIFEST: &str = r#"{
  "id": "com.example.alpha", "name": "Advanced CSV Exporter", "version": "1.2.0",
  "description": "Export query results to CSV with custom delimiters, headers and encodings.",
  "author": { "name": "DB Hive Team", "email": "plugins@dbhive.com" },
  "category": "export", "main": "index.js", "pluginType": "javascript",
  "permissions": ["showNotification", "writeFiles", "modifyUI"],
  "minVersion": "0.1.0", "license": "MIT",
  "homepage": "https://github.com/dbhive/csv-exporter",
  "repository": "https://github.com/dbhive/csv-exporter",
  "keywords": ["csv", "export", "data", "excel"],
  "configSchema": { "type": "object", "properties": {
    "delimiter": { "type": "string", "enum": [",", ";", "\t", "|"], "default": ",", "title": "Field delimiter",
                   "description": "Character that separates fields." },
    "includeHeaders": { "type": "boolean", "default": true, "title": "Include headers" },
    "maxRows": { "type": "integer", "default": 1000, "minimum": 1, "maximum": 50000, "title": "Row limit" },
    "nullValue": { "type": "string", "default": "", "title": "NULL representation" }
  } }
}"#;

fn plugins_sample(id: &str, name: &str, version: &str, enabled: bool) -> dbhive_core::plugins::Plugin {
    use dbhive_core::plugins::{Plugin, PluginManifest, PluginStats};
    let mut manifest = PluginManifest::parse(PLUGINS_MANIFEST).expect("sample manifest");
    manifest.id = id.into();
    manifest.name = name.into();
    manifest.version = version.into();
    let mut stats = PluginStats::new("2026-09-14T10:00:00Z".into());
    stats.execution_count = 12;
    stats.last_used = Some("2026-10-18T08:30:00Z".into());
    Plugin {
        manifest,
        path: std::path::PathBuf::from("/plugins"),
        enabled,
        loaded: false,
        config: None,
        stats,
    }
}

fn plugins_sample_list() -> Vec<dbhive_core::plugins::Plugin> {
    let mut formatter = plugins_sample("com.example.formatter", "SQL Formatter Pro", "1.5.2", true);
    formatter.manifest.category = dbhive_core::plugins::PluginCategory::Formatter;
    formatter.manifest.permissions = vec![dbhive_core::plugins::PluginPermission::ModifyUi];
    formatter.manifest.author.name = "SQL Tools Inc".into();
    let mut theme = plugins_sample("com.example.theme", "Dark Theme Plus", "2.1.0", false);
    theme.manifest.category = dbhive_core::plugins::PluginCategory::Theme;
    theme.manifest.config_schema = None;
    theme.manifest.permissions = vec![];
    theme.manifest.author.name = "Theme Designer".into();
    vec![
        plugins_sample("com.example.alpha", "Advanced CSV Exporter", "1.2.0", true),
        theme,
        formatter,
    ]
}

/// The interface a plugin described: a form, a button and a table.
fn plugins_sample_interface() -> dbhive_core::plugins::PluginUi {
    dbhive_core::plugins::PluginUi::parse(
        r#"{
      "title": "Export to CSV",
      "toolbar": [{ "id": "quick-export", "label": "Export CSV", "icon": "export", "tooltip": "Export the last result" }],
      "panels": [
        { "title": "Options", "items": [
          { "type": "text", "text": "Choose how the file is written.", "style": "muted" },
          { "type": "input", "id": "name", "label": "File name", "value": "export.csv" },
          { "type": "number", "id": "rows", "label": "Row limit", "value": 1000, "min": 1, "max": 50000 },
          { "type": "select", "id": "enc", "label": "Encoding", "value": "utf-8",
            "options": [{ "value": "utf-8", "label": "UTF-8" }, { "value": "utf-16", "label": "UTF-16" }] },
          { "type": "checkbox", "id": "headers", "label": "Include a header row", "value": true },
          { "type": "button", "id": "export", "label": "Export now", "style": "primary" }
        ] },
        { "title": "Recent exports", "collapsible": true, "items": [
          { "type": "table", "columns": ["File", "Rows", "When"], "rows": [
            ["orders-2026-10-18.csv", "1204", "2 days ago"],
            ["customers.csv", "310", "last week"],
            ["a_file_with_a_very_long_name_that_must_be_elided_instead_of_overflowing.csv", "52", "last month"] ] }
        ] }
      ]
    }"#,
    )
    .expect("sample interface")
}

fn plugins_window(name: &str) -> Box<dyn std::any::Any> {
    use dbhive_ui_model::plugins::Tab;
    use dbhive_ui_model::plugins_panel::{Busy, Load, PluginsPanel};

    let light = name.ends_with("-light");
    let name = name.trim_end_matches("-light");

    let c = AppWindow::new().expect("AppWindow");
    if light {
        c.global::<Theme>().set_dark(false);
    }
    if name == "plugins-toolbar-connections" {
        c.set_screen(Screen::Connections);
    } else {
        panel_backdrop(&c);
    }
    let s = c.global::<PluginsState>();

    let mut p = PluginsPanel::new();
    p.apply_scan(Ok((plugins_sample_list(), vec![])));
    p.select("com.example.alpha");
    if name.starts_with("plugins-market") {
        p.tab = Tab::Marketplace;
    }

    match name {
        "plugins" | "plugins-toolbar" | "plugins-toolbar-connections" => {}
        "plugins-disabled" => p.select("com.example.theme"),
        "plugins-busy" => {
            p.busy.insert("com.example.theme".into(), Busy::Toggling);
            p.busy.insert("com.example.alpha".into(), Busy::Running);
        }
        "plugins-interface" | "plugins-interface-error" => {
            p.set_interface("com.example.alpha", plugins_sample_interface());
            p.open_view = Some("com.example.alpha".into());
            if name == "plugins-interface-error" {
                p.set_interface_error(
                    "com.example.alpha",
                    "Plugin execution error: Permission denied: writeFiles".into(),
                );
            }
        }
        "plugins-settings" | "plugins-settings-error" => {
            p.open_settings("com.example.alpha");
            if name == "plugins-settings-error" {
                p.set_number(dbhive_ui_model::plugins_panel::Scope::Form, "maxRows", "lots");
            }
        }
        "plugins-empty" => p.apply_scan(Ok((vec![], vec![]))),
        "plugins-loading" | "plugins-market-loading" => p.load = Load::Loading,
        "plugins-error" | "plugins-market-error" => {
            p.apply_scan(Err("Could not read the plugin folder: permission denied (os error 13)".into()))
        }
        "plugins-problems" => p.apply_scan(Ok((
            plugins_sample_list(),
            vec![
                "broken-theme: Invalid manifest: missing field `pluginType`".into(),
                "old-exporter: Incompatible version: requires 9.0.0, current is 0.21.0".into(),
            ],
        ))),
        "plugins-long" => {
            let mut long = plugins_sample(
                "com.example.long",
                "A plugin with a name that is much too long to fit in the installed list without eliding",
                "10.20.30",
                true,
            );
            long.manifest.description = "This description keeps going to prove that long copy wraps inside the detail pane instead of running off the edge of the panel, and stays readable. ".repeat(3);
            long.manifest.author.name = "An author with a very long name, Incorporated and Associates".into();
            let mut list = plugins_sample_list();
            list.insert(0, long);
            p.apply_scan(Ok((list, vec![])));
            p.select("com.example.long");
        }
        "plugins-market" => {
            p.upsert(plugins_sample("com.dbhive.csv-exporter", "Advanced CSV Exporter", "1.0.0", true));
            p.busy.insert("com.dbhive.dark-theme-plus".into(), Busy::Installing);
            p.install_errors.insert(
                "com.dbhive.sql-formatter".into(),
                "SQL Formatter Pro is not bundled with this build, and downloading plugins is not supported yet.".into(),
            );
        }
        "plugins-market-filtered" => {
            p.filters.search = "theme".into();
            s.set_search("theme".into());
        }
        "plugins-market-empty" => {
            p.filters.search = "zzzz".into();
            s.set_search("zzzz".into());
        }
        other => {
            eprintln!("unknown plugins variant {:?}", other);
            std::process::exit(2);
        }
    }

    if name.starts_with("plugins-toolbar") {
        // The panel stays closed; the buttons sit in the screen's header.
        p.set_interface("com.example.alpha", plugins_sample_interface());
    } else {
        s.set_visible(true);
    }
    dbhive_slint::plugins_view::apply(&s, &p, dbhive_slint::plugins_view::Items::Rebuild);
    c.show().unwrap();
    Box::new(c)
}

// -- AI assistant -----------------------------------------------------------------
//
// Variants: ai-answers (a conversation: generate, fix, explain), ai-empty (ready, no
// conversation yet), ai-no-sql (empty editor), ai-busy, ai-failed (a provider
// error in the conversation), ai-context (the schema viewer open),
// ai-context-off (the schema switched off), ai-context-error, ai-models-error,
// ai-models-loading, ai-key (no API key, settings open), ai-key-saved (settings
// open with a key saved), ai-checking, ai-ollama (not running),
// ai-unavailable, ai-long (long strings); each with an optional `-light`.
// The panel is docked in the workspace, so the workspace is the backdrop.

const AI_EDITOR_SQL: &str = "SELECT c.name, SUM(o.total) AS revenue\n  FORM orders o\n  JOIN customers c ON c.id = o.customer_id\n GROUP BY c.name\n ORDER BY revenue DESC;";
const AI_EDITOR_ERROR: &str = "syntax error at or near \"FORM\" (position 41)";

fn ai_models() -> Vec<dbhive_core::ai::AiModel> {
    use dbhive_core::ai::{AiModel, AiProviderType};
    ["gpt-4o", "gpt-4o-mini", "o1-mini"]
        .iter()
        .map(|id| AiModel {
            id: (*id).into(),
            name: id.to_uppercase().replace("GPT-4O", "GPT-4o").replace("-MINI", " Mini"),
            provider: AiProviderType::OpenAI,
            description: Some("128K context".into()),
            context_window: Some(128_000),
        })
        .collect()
}

fn ai_sample_context() -> String {
    use dbhive_core::service_ai::{build_schema_context, SchemaColumn, SchemaTable};
    let col = |n: &str, t: &str| SchemaColumn { name: n.into(), data_type: t.into() };
    let tables = vec![
        SchemaTable {
            schema: "public".into(),
            table: "customers".into(),
            columns: vec![col("id", "integer"), col("name", "text"), col("email", "text"), col("created_at", "timestamp")],
        },
        SchemaTable {
            schema: "public".into(),
            table: "orders".into(),
            columns: vec![col("id", "integer"), col("customer_id", "integer"), col("total", "numeric"), col("status", "text")],
        },
    ];
    build_schema_context("app", &tables)
}

fn ai_response(content: &str, ms: u64) -> dbhive_core::service_ai::AiResponse {
    dbhive_core::service_ai::AiResponse {
        content: content.into(),
        model: "gpt-4o".into(),
        provider: dbhive_core::ai::AiProviderType::OpenAI,
        duration_ms: ms,
    }
}

/// A panel on OpenAI that is connected, with models and the schema loaded.
fn ai_ready_panel() -> dbhive_ui_model::ai_panel::AiPanel {
    use dbhive_core::ai::AiProviderType::OpenAI;
    use dbhive_core::service_ai::{AiSettings, ProviderStatus};
    let mut p = dbhive_ui_model::ai_panel::AiPanel::default();
    let mut settings = AiSettings::default();
    settings.active_provider = OpenAI;
    settings.openai.default_model = "gpt-4o".into();
    p.settings_result(Ok(settings));
    p.status_result(OpenAI, ProviderStatus { provider: OpenAI, available: true, configured: true });
    p.begin_models();
    p.models_result(OpenAI, Ok(ai_models()));
    let key = ("conn".to_string(), "app".to_string());
    p.begin_context(key.clone());
    p.context_result(&key, Ok(ai_sample_context()));
    p
}

fn ai_window(name: &str) -> Box<dyn std::any::Any> {
    use dbhive_core::ai::AiProviderType::{self, OpenAI};
    use dbhive_core::service_ai::ProviderStatus;
    use dbhive_ui_model::ai_assistant::Kind;
    use dbhive_ui_model::ai_panel::Inputs;

    let light = name.ends_with("-light");
    let base = name.strip_suffix("-light").unwrap_or(name);

    let c = AppWindow::new().expect("AppWindow");
    if light {
        c.global::<Theme>().set_dark(false);
    }
    c.set_screen(Screen::Workspace);
    c.set_workspace_connection_name("Production Postgres".into());
    c.set_workspace_location_label("app · public".into());
    let tables: Vec<TableItem> = ["customers", "orders", "order_items", "products"]
        .iter()
        .map(|n| TableItem { name: (*n).into(), row_count: "~1240".into() })
        .collect();
    c.set_workspace_tables(slint::ModelRc::new(slint::VecModel::from(tables)));
    c.set_workspace_tabs(slint::ModelRc::new(slint::VecModel::from(vec![TabInfo {
        title: "Query 1".into(),
        query: true,
        busy: false,
    }])));
    c.set_workspace_active_tab(0);
    c.set_workspace_active_is_query(true);
    c.set_workspace_result_title("Results".into());

    // The editor holds a query with a typo, which failed, unless a variant
    // says otherwise.
    let blank_editor = base == "ai-no-sql";
    let (sql, error): (&str, Option<&str>) = if blank_editor {
        ("", None)
    } else if base == "ai-long" {
        (
            "SELECT a_really_long_column_name_that_keeps_going_and_going_and_going_without_a_single_break_0001 FROM a_table_with_a_very_long_name_that_must_wrap_or_clip_not_overflow_the_panel",
            Some("column \"a_really_long_column_name_that_keeps_going_and_going_and_going_without_a_single_break_0001\" does not exist and this message is long enough that it must be clipped to four lines at most in the panel so it never pushes the controls off the screen at all"),
        )
    } else {
        (AI_EDITOR_SQL, Some(AI_EDITOR_ERROR))
    };
    if !sql.is_empty() {
        let dark = c.global::<Theme>().get_dark();
        c.set_workspace_editor_text(sql.into());
        c.set_workspace_editor_lines(dbhive_slint::sql_highlight::to_model_for(sql, dark));
    }
    if let Some(e) = error {
        c.set_workspace_rows_error(e.into());
    }

    let mut p = ai_ready_panel();
    let mut prompt: &'static str = "";
    let inputs = |prompt: &'static str| Inputs { current_sql: sql, last_error: error, prompt };

    match base {
        "ai-answers" | "ai-long" => {
            // Generate, then fix the failed query, then explain it.
            p.start(Kind::Generate, inputs("top customers by revenue"));
            p.finish(
                OpenAI,
                Kind::Generate,
                Ok(ai_response("```sql\nSELECT c.name, SUM(o.total) AS revenue\n  FROM orders o\n  JOIN customers c ON c.id = o.customer_id\n GROUP BY c.name\n ORDER BY revenue DESC\n LIMIT 10;\n```", 1240)),
            );
            if base == "ai-answers" {
                p.start(Kind::Fix, inputs(""));
                p.finish(
                    OpenAI,
                    Kind::Fix,
                    Ok(ai_response("```sql\nSELECT c.name, SUM(o.total) AS revenue\n  FROM orders o\n  JOIN customers c ON c.id = o.customer_id\n GROUP BY c.name\n ORDER BY revenue DESC;\n```\nThe keyword is FROM, not FORM.", 880)),
                );
                p.start(Kind::Explain, inputs(""));
                p.finish(
                    OpenAI,
                    Kind::Explain,
                    Ok(ai_response("Totals every customer's paid orders and lists the biggest spenders first.\nThe join keeps only customers who have at least one order.", 2410)),
                );
            }
            prompt = "top customers by revenue";
        }
        "ai-empty" | "ai-no-sql" => {}
        "ai-busy" => {
            p.start(Kind::Fix, inputs(""));
        }
        "ai-failed" => {
            p.start(Kind::Optimize, inputs(""));
            p.finish(OpenAI, Kind::Optimize, Err("OpenAI API error: Rate limit reached for gpt-4o. Please try again in 20s.".into()));
        }
        "ai-context" | "ai-context-off" => {
            p.toggle_context_viewer();
            if base == "ai-context-off" {
                p.set_include_schema(false);
            }
        }
        "ai-context-error" => {
            let key = ("conn".to_string(), "other".to_string());
            p.begin_context(key.clone());
            p.context_result(&key, Err("permission denied for schema public".into()));
            p.toggle_context_viewer();
        }
        "ai-models-error" => {
            p.refresh_models();
            p.begin_models();
            p.models_result(OpenAI, Err("OpenAI API error: Incorrect API key provided".into()));
        }
        "ai-models-loading" => {
            p.refresh_models();
            p.begin_models();
        }
        "ai-key" | "ai-key-saved" => {
            p.show_settings(true);
            if base == "ai-key" {
                p.status_result(OpenAI, ProviderStatus { provider: OpenAI, available: false, configured: false });
            }
        }
        "ai-checking" => p.begin_status_check(),
        "ai-ollama" => {
            p.change_provider(AiProviderType::Ollama);
            p.status_result(
                AiProviderType::Ollama,
                ProviderStatus { provider: AiProviderType::Ollama, available: false, configured: true },
            );
        }
        "ai-unavailable" => {
            p.status_result(OpenAI, ProviderStatus { provider: OpenAI, available: false, configured: true });
        }
        other => {
            eprintln!("unknown AI variant {:?}", other);
            std::process::exit(2);
        }
    }

    let s = c.global::<AiState>();
    s.set_visible(true);
    s.set_prompt(prompt.into());
    let view = p.view(inputs(prompt));
    dbhive_slint::ai_view::apply(
        &c,
        &view,
        &dbhive_slint::ai_view::Extras { sync_base_url: true, ..Default::default() },
    );
    c.show().unwrap();
    Box::new(c)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let name = args.get(1).map(String::as_str).unwrap_or("app");
    let out = args.get(2).map(String::as_str).unwrap_or("snapshot.ppm");
    let width: u32 = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(1200);
    let height: u32 = args.get(4).and_then(|v| v.parse().ok()).unwrap_or(760);
    let scale: f32 = args.get(5).and_then(|v| v.parse().ok()).unwrap_or(2.0);

    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(SnapshotPlatform {
        window: window.clone(),
        start: std::time::Instant::now(),
    }))
    .expect("platform");

    let (pw, ph) = ((width as f32 * scale) as u32, (height as f32 * scale) as u32);
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: scale });
    window.set_size(slint::PhysicalSize::new(pw, ph));

    let _keep = instantiate(name);

    // Let bindings settle and any zero-duration timers fire before drawing.
    for _ in 0..3 {
        slint::platform::update_timers_and_animations();
    }

    let mut buffer = vec![PremultipliedRgbaColor::default(); (pw * ph) as usize];
    window.request_redraw();
    window.draw_if_needed(|renderer| {
        renderer.render(&mut buffer, pw as usize);
    });

    let mut ppm = format!("P6\n{} {}\n255\n", pw, ph).into_bytes();
    // Opaque window background, so premultiplied equals straight colour.
    for px in &buffer {
        ppm.extend_from_slice(&[px.red, px.green, px.blue]);
    }
    std::fs::write(out, ppm).expect("write snapshot");
    println!("wrote {} ({}x{})", out, pw, ph);
}
