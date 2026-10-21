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
            c.set_workspace_rows(slint::ModelRc::new(dbhive_slint::grid_model::LazyRows::new(std::rc::Rc::new(data))));
            c.set_workspace_row_count(n as i32);
            c.set_workspace_visible_row_count(n as i32);
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
