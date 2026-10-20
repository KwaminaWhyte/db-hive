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
        other => {
            eprintln!("unknown component {:?}", other);
            std::process::exit(2);
        }
    }
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
