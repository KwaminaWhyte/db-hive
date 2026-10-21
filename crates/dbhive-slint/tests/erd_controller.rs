//! The ER diagram driven end to end: the real window, the real controller, a
//! real SQLite file with foreign keys.
//!
//! Open it from the palette command and see the tables laid out with their
//! key badges, the column cap, the junction table and many-to-many links, and
//! the whole diagram fitted into the canvas. Drag a table and watch the
//! arrangement land in `erd-layouts.json` in the web diagram's format; reload
//! and see it come back. Toggle columns, filter, auto layout (which forgets
//! the saved arrangement), then export an SVG: a folder is refused, an
//! existing file needs a second confirming save.
//!
//! Every store is redirected to a temp directory first, so this never touches
//! the developer's own connections or saved diagram layouts.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{DiagramNode, ErdState, Screen};
use slint::{ComponentHandle, Model, Timer, TimerMode};

fn node(w: &dbhive_slint::AppWindow, title: &str) -> Option<(usize, DiagramNode)> {
    w.global::<ErdState>()
        .get_nodes()
        .iter()
        .enumerate()
        .find(|(_, n)| n.title == title)
}

fn layouts(dir: &std::path::Path) -> serde_json::Value {
    std::fs::read_to_string(dir.join("erd-layouts.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null)
}

#[test]
fn open_the_diagram_arrange_it_keep_the_arrangement_and_export_it() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-erd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let db = dir.join("shop.sqlite");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        let wide: String = (1..=13).map(|i| format!(", c{} TEXT", i)).collect();
        conn.execute_batch(&format!(
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT NOT NULL);
             CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers(id), total REAL);
             CREATE TABLE tags (id INTEGER PRIMARY KEY, label TEXT);
             CREATE TABLE order_tags (order_id INTEGER NOT NULL REFERENCES orders(id), tag_id INTEGER NOT NULL REFERENCES tags(id), PRIMARY KEY (order_id, tag_id));
             CREATE TABLE audit (id INTEGER PRIMARY KEY{wide});"
        ))
        .unwrap();
    }
    let db_name = db.to_string_lossy().to_string();

    {
        let mut profile = ConnectionProfile::new(
            "shop-db".to_string(),
            "Shop".to_string(),
            DbDriver::Sqlite,
            String::new(),
            0,
            String::new(),
        );
        profile.database = Some(db_name.clone());
        let mut state = AppState::new();
        state.add_profile(profile);
        state.save_profiles_to_store().unwrap();
    }
    // SQLite's "schema" is the database file; the saved layout is keyed by it.
    let layout_key = format!("shop-db-{}", db_name);

    let w = dbhive_slint::app::build().unwrap();
    // The headless window has no size until it is given one; the canvas
    // reports its own size from the layout, which the fit depends on.
    w.window().set_size(slint::LogicalSize::new(1280.0, 800.0));
    let failure: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let step = Rc::new(Cell::new(0u32));
    let saved_pos: Rc<Cell<(f32, f32)>> = Rc::new(Cell::new((0.0, 0.0)));
    let started = Instant::now();
    let timer = Timer::default();

    let svg_path = dir.join("diagram.svg");

    let (weak, fail, step2, dir2, svg2, saved2, key2) = (
        w.as_weak(),
        failure.clone(),
        step.clone(),
        dir.clone(),
        svg_path.clone(),
        saved_pos.clone(),
        layout_key.clone(),
    );
    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        let w = weak.upgrade().unwrap();
        let st = w.global::<ErdState>();
        // A headless window never lays out or renders, and `if` blocks are
        // only instantiated by a layout or input pass; a pointer event makes
        // that pass happen, as it does in the real window every frame.
        w.window().dispatch_event(slint::platform::WindowEvent::PointerMoved {
            position: slint::LogicalPosition::new(5.0, 5.0),
        });
        let bail = |msg: String| {
            *fail.borrow_mut() = Some(msg);
            slint::quit_event_loop().unwrap();
        };
        if started.elapsed() > Duration::from_secs(40) {
            bail(format!("timed out at step {} (window {:?}, viewport {}x{}, zoom {}, visible {}, loading {})", step2.get(), w.window().size(), st.get_viewport_width(), st.get_viewport_height(), st.get_zoom(), st.get_visible(), st.get_loading()));
            return;
        }
        match step2.get() {
            // Connect, then open the diagram from the palette command.
            0 => {
                w.invoke_connect("shop-db".into());
                step2.set(1);
            }
            1 if w.get_screen() == Screen::Workspace && w.get_workspace_tables().row_count() == 5 => {
                w.invoke_run_command("go-er-diagram".into());
                if !st.get_visible() {
                    return bail("the palette command did not open the diagram".into());
                }
                step2.set(2);
            }

            // Loaded: every table, badges, the column cap, the junction table.
            2 if !st.get_loading() && st.get_nodes().row_count() == 5 => {
                if !st.get_error().is_empty() {
                    return bail(format!("load error: {}", st.get_error()));
                }
                if !st.get_subtitle().contains("5 tables, 3 relationships") {
                    return bail(format!("subtitle was {:?}", st.get_subtitle()));
                }
                let (_, orders) = node(&w, "orders").unwrap();
                let badges: Vec<String> = orders.rows.iter().map(|r| r.badge.to_string()).collect();
                if badges != ["PK", "FK", ""] {
                    return bail(format!("orders badges were {:?}", badges));
                }
                let (_, audit) = node(&w, "audit").unwrap();
                if audit.rows.row_count() != 10 || audit.more != "+4 more columns" {
                    return bail(format!(
                        "audit should show 10 rows and '+4 more columns', got {} / {:?}",
                        audit.rows.row_count(),
                        audit.more
                    ));
                }
                let (_, junction) = node(&w, "order_tags").unwrap();
                if junction.tag != "M:N" || !junction.junction {
                    return bail("order_tags should be marked as a junction".into());
                }
                if node(&w, "orders").unwrap().1.junction {
                    return bail("orders must not be marked as a junction".into());
                }
                let edges = st.get_edges();
                if edges.row_count() != 3 {
                    return bail(format!("expected 3 relationships, got {}", edges.row_count()));
                }
                let dashed = edges.iter().filter(|e| e.muted).count();
                if dashed != 2 {
                    return bail(format!("the two links through the junction should be dashed, got {}", dashed));
                }
                if !edges.iter().all(|e| e.label.contains('\u{2192}')) {
                    return bail("every relationship should carry a label".into());
                }
                step2.set(3);
            }

            // The canvas reported its size, so the diagram was fitted: every
            // table lies inside the viewport at the chosen zoom.
            3 if st.get_viewport_width() > 1.0 => {
                let (z, px, py) = (st.get_zoom(), st.get_pan_x(), st.get_pan_y());
                let (vw, vh) = (st.get_viewport_width(), st.get_viewport_height());
                if z > 1.0 + 1e-4 {
                    return bail(format!("fit must not zoom past 100%, got {}", z));
                }
                for n in st.get_nodes().iter() {
                    let (l, t) = (px + n.x * z, py + n.y * z);
                    let (r, b) = (l + n.width * z, t + n.height * z);
                    if l < -0.5 || t < -0.5 || r > vw + 0.5 || b > vh + 0.5 {
                        return bail(format!(
                            "{} is outside the fitted viewport: ({l:.0},{t:.0})-({r:.0},{b:.0}) in {vw:.0}x{vh:.0}",
                            n.title
                        ));
                    }
                }
                if st.get_zoom_label().is_empty() {
                    return bail("the zoom label is empty".into());
                }
                step2.set(4);
            }

            // Drag a table by a screen delta; it snaps to the 16px grid and the
            // arrangement is saved in the web diagram's format.
            4 => {
                let (i, orders) = node(&w, "orders").unwrap();
                let z = st.get_zoom();
                let (x0, y0) = (orders.x, orders.y);
                st.invoke_node_dragged(i as i32, 100.0, 60.0);
                st.invoke_node_released(i as i32);
                let moved = node(&w, "orders").unwrap().1;
                let snap = |v: f32| (v / 16.0).round() * 16.0;
                let (ex, ey) = (snap(x0 + 100.0 / z), snap(y0 + 60.0 / z));
                if (moved.x - ex).abs() > 0.01 || (moved.y - ey).abs() > 0.01 {
                    return bail(format!(
                        "orders moved to ({}, {}), expected ({}, {})",
                        moved.x, moved.y, ex, ey
                    ));
                }
                if moved.x % 16.0 != 0.0 || moved.y % 16.0 != 0.0 {
                    return bail("the dragged table is off the grid".into());
                }
                saved2.set((moved.x, moved.y));
                step2.set(5);
            }
            5 => {
                let saved = layouts(&dir2);
                let entry = &saved[key2.as_str()]["orders"];
                if entry.is_null() {
                    if started.elapsed() > Duration::from_secs(30) {
                        bail("erd-layouts.json never received the arrangement".into());
                    }
                    return;
                }
                let (sx, sy) = saved2.get();
                if (entry["x"].as_f64().unwrap() as f32 - sx).abs() > 0.01
                    || (entry["y"].as_f64().unwrap() as f32 - sy).abs() > 0.01
                {
                    return bail(format!("saved position {entry} does not match ({sx}, {sy})"));
                }
                if saved[key2.as_str()].as_object().unwrap().len() != 1 {
                    return bail("only the dragged table should be saved".into());
                }
                // Reload from the database: the arrangement comes back.
                st.invoke_refresh();
                step2.set(6);
            }
            6 if !st.get_loading() && st.get_nodes().row_count() == 5 => {
                let o = node(&w, "orders").unwrap().1;
                let (sx, sy) = saved2.get();
                if (o.x - sx).abs() > 0.01 || (o.y - sy).abs() > 0.01 {
                    return bail(format!("reload lost the arrangement: ({}, {}) vs ({sx}, {sy})", o.x, o.y));
                }
                // Hide the columns: boxes collapse to their headers.
                st.invoke_toggle_columns();
                step2.set(7);
            }
            7 if !st.get_show_columns() => {
                let o = node(&w, "orders").unwrap().1;
                if o.rows.row_count() != 0 || o.height > 40.0 {
                    return bail(format!("columns hidden but orders has {} rows, height {}", o.rows.row_count(), o.height));
                }
                st.invoke_toggle_columns();
                step2.set(8);
            }
            8 if st.get_show_columns() => {
                // Highlight filter.
                st.set_filter("order".into());
                st.invoke_filter_edited("order".into());
                let by = |t: &str| node(&w, t).unwrap().1;
                if !by("orders").emphasis || !by("order_tags").emphasis {
                    return bail("orders and order_tags should match the filter".into());
                }
                if by("customers").emphasis || !by("customers").dimmed {
                    return bail("customers should be dimmed while filtering".into());
                }
                st.set_filter("".into());
                st.invoke_filter_edited("".into());
                if by("customers").dimmed {
                    return bail("clearing the filter should undim".into());
                }
                // Relationships off, then on.
                st.invoke_toggle_relationships();
                if st.get_edges().row_count() != 0 {
                    return bail("hiding relationships should empty the edges".into());
                }
                st.invoke_toggle_relationships();
                if st.get_edges().row_count() != 3 {
                    return bail("showing relationships should bring the edges back".into());
                }
                // Auto layout forgets the saved arrangement.
                st.invoke_auto_layout();
                step2.set(9);
            }
            9 => {
                let o = node(&w, "orders").unwrap().1;
                let (sx, sy) = saved2.get();
                if (o.x - sx).abs() < 0.01 && (o.y - sy).abs() < 0.01 {
                    return bail("auto layout left the table where it was dragged".into());
                }
                if !layouts(&dir2)[key2.as_str()].is_null() {
                    if started.elapsed() > Duration::from_secs(30) {
                        return bail("auto layout never cleared the saved arrangement".into());
                    }
                    return;
                }
                // Export: a folder is refused.
                st.invoke_export_open_requested();
                if !st.get_export_open() || !st.get_export_path().ends_with("-erd.svg") {
                    return bail(format!("export panel: open {} path {:?}", st.get_export_open(), st.get_export_path()));
                }
                st.set_export_path(dir2.to_string_lossy().to_string().into());
                st.invoke_export_save(dir2.to_string_lossy().to_string().into());
                if !st.get_export_message().contains("folder") || st.get_export_ok() {
                    return bail(format!("a folder should be refused, got {:?}", st.get_export_message()));
                }
                let target = svg2.to_string_lossy().to_string();
                st.set_export_path(target.clone().into());
                st.invoke_export_save(target.into());
                step2.set(10);
            }
            10 if st.get_export_ok() && !st.get_exporting() => {
                let svg = std::fs::read_to_string(&svg2).unwrap_or_default();
                if !svg.starts_with("<svg ") || !svg.contains(">orders</text>") || !svg.contains("stroke-dasharray") {
                    return bail(format!("exported file is not the diagram: {:.200}", svg));
                }
                // An existing file needs a second, confirming save.
                std::fs::write(&svg2, "keep me").unwrap();
                let target = svg2.to_string_lossy().to_string();
                st.invoke_export_save(target.clone().into());
                if !st.get_export_message().contains("already exists") {
                    return bail(format!("overwrite was not refused: {:?}", st.get_export_message()));
                }
                if std::fs::read_to_string(&svg2).unwrap() != "keep me" {
                    return bail("the existing file was overwritten without confirmation".into());
                }
                st.invoke_export_save(target.into());
                step2.set(11);
            }
            11 if st.get_export_ok() && !st.get_exporting() && st.get_export_message().starts_with("Saved") => {
                if !std::fs::read_to_string(&svg2).unwrap_or_default().starts_with("<svg ") {
                    return bail("the confirmed save did not replace the file".into());
                }
                st.invoke_close();
                if st.get_visible() {
                    return bail("close did not hide the diagram".into());
                }
                // And it opens again from the button path without reloading.
                st.invoke_open_requested();
                if !st.get_visible() || st.get_nodes().row_count() != 5 {
                    return bail("reopening should show the loaded diagram at once".into());
                }
                step2.set(99);
                slint::quit_event_loop().unwrap();
            }
            _ => {}
        }
    });

    w.run().unwrap();
    drop(timer);
    let _ = std::fs::remove_dir_all(&dir);

    if let Some(msg) = failure.borrow().as_ref() {
        panic!("{msg}");
    }
    assert_eq!(step.get(), 99, "the scenario did not run to its end");
}
