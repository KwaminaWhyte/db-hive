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
        "workspace" | "workspace-light" | "workspace-long" | "workspace-empty" | "workspace-loading"
        | "workspace-error" => {
            let c = AppWindow::new().expect("AppWindow");
            if name == "workspace-light" {
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
            } else if name == "workspace" || name == "workspace-light" || name == "workspace-long" {
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
