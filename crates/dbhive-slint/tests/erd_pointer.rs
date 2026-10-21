//! The ER diagram canvas driven with real pointer events: pan by dragging
//! empty space, move a table by dragging it (which selects it, snaps it to the
//! grid and saves the arrangement), zoom with the scroll wheel about the
//! pointer, and clear the selection by clicking the background.
//!
//! `erd_controller.rs` drives the same flows through the state callbacks; this
//! proves the canvas itself raises them from pointer input.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{ErdState, Screen};
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, Model, Timer, TimerMode};

const WINDOW_H: f32 = 800.0;

#[test]
fn pan_drag_a_table_and_zoom_with_real_pointer_events() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-erd-pointer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let db = dir.join("shop.sqlite");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT);
             CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id));
             CREATE TABLE items (id INTEGER PRIMARY KEY, order_id INTEGER REFERENCES orders(id));",
        )
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
    let key = format!("shop-db-{}", db_name);

    let w = dbhive_slint::app::build().unwrap();
    w.window()
        .set_size(slint::LogicalSize::new(1280.0, WINDOW_H));
    let failure: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let step = Rc::new(Cell::new(0u32));
    // (pan_x, pan_y) before a gesture, and the zoom before the wheel.
    let before: Rc<Cell<(f32, f32, f32)>> = Rc::new(Cell::new((0.0, 0.0, 1.0)));
    let started = Instant::now();
    let timer = Timer::default();

    let (weak, fail, step2, before2, dir2, key2) = (
        w.as_weak(),
        failure.clone(),
        step.clone(),
        before.clone(),
        dir.clone(),
        key.clone(),
    );
    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        let w = weak.upgrade().unwrap();
        let st = w.global::<ErdState>();
        let win = w.window();
        let bail = |msg: String| {
            *fail.borrow_mut() = Some(msg);
            slint::quit_event_loop().unwrap();
        };
        if started.elapsed() > Duration::from_secs(40) {
            bail(format!("timed out at step {}", step2.get()));
            return;
        }
        // The canvas's top-left in window coordinates: below the title bar and
        // toolbar (and the in-window menu bar), above the 30px legend strip.
        // Read lazily: the canvas only reports its size once it exists.
        let at = |x: f32, y: f32| LogicalPosition::new(x, WINDOW_H - 30.0 - st.get_viewport_height() + y);
        let press = |p: LogicalPosition| win.dispatch_event(WindowEvent::PointerPressed { position: p, button: PointerEventButton::Left });
        let release = |p: LogicalPosition| win.dispatch_event(WindowEvent::PointerReleased { position: p, button: PointerEventButton::Left });
        let moved = |p: LogicalPosition| win.dispatch_event(WindowEvent::PointerMoved { position: p });
        // Keep layout alive: a pointer move makes the headless window
        // instantiate `if` blocks (see erd_controller.rs).
        if step2.get() < 3 {
            moved(LogicalPosition::new(5.0, 5.0));
        }

        match step2.get() {
            0 => {
                w.invoke_connect("shop-db".into());
                step2.set(1);
            }
            1 if w.get_screen() == Screen::Workspace && w.get_workspace_tables().row_count() == 3 => {
                w.invoke_run_command("go-er-diagram".into());
                step2.set(2);
            }
            2 if !st.get_loading()
                && st.get_nodes().row_count() == 3
                && st.get_viewport_width() > 1.0
                && st.get_zoom_label() != "" =>
            {
                // Pan: drag empty space (the top-right corner, clear after a fit).
                before2.set((st.get_pan_x(), st.get_pan_y(), st.get_zoom()));
                let start = at(st.get_viewport_width() - 6.0, 6.0);
                press(start);
                for k in 1..=6 {
                    moved(LogicalPosition::new(start.x + 5.0 * k as f32, start.y + 3.0 * k as f32));
                }
                release(LogicalPosition::new(start.x + 30.0, start.y + 18.0));
                step2.set(3);
            }
            3 => {
                let (px, py, _) = before2.get();
                let (dx, dy) = (st.get_pan_x() - px, st.get_pan_y() - py);
                if (dx - 30.0).abs() > 0.6 || (dy - 18.0).abs() > 0.6 {
                    return bail(format!("dragging empty space should pan by (30, 18), moved ({dx}, {dy})"));
                }
                // A pan is not a click: nothing was deselected or selected by it.
                if !st.get_selection_label().is_empty() {
                    return bail(format!("panning selected something: {:?}", st.get_selection_label()));
                }
                // Drag the orders table by its header with the pointer.
                let (i, n) = st
                    .get_nodes()
                    .iter()
                    .enumerate()
                    .find(|(_, n)| n.title == "orders")
                    .unwrap();
                let z = st.get_zoom();
                let (cx, cy) = (
                    st.get_pan_x() + (n.x + n.width / 2.0) * z,
                    st.get_pan_y() + (n.y + 10.0) * z,
                );
                let start = at(cx, cy);
                before2.set((n.x, n.y, i as f32));
                press(start);
                for k in 1..=8 {
                    moved(LogicalPosition::new(start.x + 10.0 * k as f32, start.y + 5.0 * k as f32));
                }
                release(LogicalPosition::new(start.x + 80.0, start.y + 40.0));
                step2.set(4);
            }
            4 => {
                let (x0, y0, i) = before2.get();
                let n = st.get_nodes().row_data(i as usize).unwrap();
                if n.title != "orders" {
                    return bail("the wrong table moved".into());
                }
                let z = st.get_zoom();
                let snap = |v: f32| (v / 16.0).round() * 16.0;
                // Eight pointer events of (10, 5) screen px each add up in world
                // units; the table lands on the grid near that total.
                let (ex, ey) = (snap(x0 + 80.0 / z), snap(y0 + 40.0 / z));
                if (n.x - ex).abs() > 16.5 || (n.y - ey).abs() > 16.5 || n.x % 16.0 != 0.0 || n.y % 16.0 != 0.0 {
                    return bail(format!("orders went from ({x0}, {y0}) to ({}, {}), expected about ({ex}, {ey}) on the grid", n.x, n.y));
                }
                if (n.x - x0).abs() < 1.0 && (n.y - y0).abs() < 1.0 {
                    return bail("the table did not move at all".into());
                }
                if !n.selected || !st.get_selection_label().starts_with("orders") {
                    return bail(format!("dragging a table should select it, label {:?}", st.get_selection_label()));
                }
                // Its arrangement is saved.
                let saved: serde_json::Value = std::fs::read_to_string(dir2.join("erd-layouts.json"))
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or(serde_json::Value::Null);
                if saved[key2.as_str()]["orders"].is_null() {
                    if started.elapsed() > Duration::from_secs(30) {
                        bail("the dragged position was never saved".into());
                    }
                    return;
                }
                // Wheel up over the middle of the canvas zooms in about that point.
                before2.set((st.get_pan_x(), st.get_pan_y(), st.get_zoom()));
                let p = at(st.get_viewport_width() / 2.0, st.get_viewport_height() / 2.0);
                moved(p);
                win.dispatch_event(WindowEvent::PointerScrolled { position: p, delta_x: 0.0, delta_y: 120.0 });
                step2.set(5);
            }
            5 => {
                let (px, py, z0) = before2.get();
                let z1 = st.get_zoom();
                if z1 <= z0 {
                    return bail(format!("wheel up should zoom in: {z0} -> {z1}"));
                }
                // The world point under the pointer stays put.
                let (ax, ay) = (st.get_viewport_width() / 2.0, st.get_viewport_height() / 2.0);
                let (wx0, wy0) = ((ax - px) / z0, (ay - py) / z0);
                let (wx1, wy1) = ((ax - st.get_pan_x()) / z1, (ay - st.get_pan_y()) / z1);
                if (wx0 - wx1).abs() > 0.5 || (wy0 - wy1).abs() > 0.5 {
                    return bail(format!("zoom drifted: ({wx0}, {wy0}) -> ({wx1}, {wy1})"));
                }
                if st.get_zoom_label() == "" {
                    return bail("zoom label empty".into());
                }
                // A click on empty canvas clears the selection.
                let start = at(st.get_viewport_width() - 6.0, 6.0);
                moved(start);
                press(start);
                release(start);
                step2.set(6);
            }
            6 => {
                if !st.get_selection_label().is_empty() {
                    return bail(format!("clicking the background should deselect, label {:?}", st.get_selection_label()));
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
