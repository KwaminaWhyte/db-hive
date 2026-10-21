//! The schema browser driven end to end: the real window, the real
//! controller, real mouse and key events, a real SQLite file.
//!
//! The logic (rows, filter, keyboard rules, menus, the drop confirmation) is
//! unit tested in `dbhive_ui_model::schema_tree`. What this proves is the
//! wiring: that a click only highlights while a double click opens a table
//! tab, that the arrow keys and Enter reach the tree and move its cursor, that
//! a right click raises the right menu, that Generate SELECT and INSERT put
//! quoted SQL in an editor and run nothing, that Drop Table warns about the
//! tables that point at it and stays closed until the name is typed exactly,
//! and that a refresh picks up a table created behind the app's back.
//!
//! SQLite has one schema, so the multi-schema tree (a database holding
//! several schemas) is covered by the unit tests and the snapshots only.
//!
//! Every store is redirected to a temp directory, so this never touches the
//! developer's own saved connections or history.

use std::cell::Cell;
use std::future::poll_fn;
use std::path::PathBuf;
use std::rc::Rc;
use std::task::Poll;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{AppWindow, Screen, TableDialogState, TreeRowKind, TreeState, TreeStatus};
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, Model, SharedString, Timer, TimerMode};

// -- helpers ---------------------------------------------------------------

async fn sleep(d: Duration) {
    let fired = Rc::new(Cell::new(false));
    let timer = Timer::default();
    let mut started = false;
    poll_fn(move |cx| {
        if fired.get() {
            return Poll::Ready(());
        }
        if !started {
            started = true;
            let (flag, waker) = (fired.clone(), cx.waker().clone());
            timer.start(TimerMode::SingleShot, d, move || {
                flag.set(true);
                waker.wake_by_ref();
            });
        }
        Poll::Pending
    })
    .await
}

/// Wait for `cond`, panicking with `what` if it never holds. The panic is the
/// test failure: it unwinds out of the event loop into `w.run()`.
async fn until(what: &str, cond: impl Fn() -> bool) {
    let started = Instant::now();
    while !cond() {
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "timed out waiting for: {what}"
        );
        sleep(Duration::from_millis(10)).await;
    }
}

/// Give the window a few turns of the event loop, for asserting that
/// something did NOT happen.
async fn settle() {
    for _ in 0..8 {
        sleep(Duration::from_millis(10)).await;
    }
}

fn press_key(w: &AppWindow, k: Key) {
    let text: SharedString = k.into();
    w.window().dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    w.window().dispatch_event(WindowEvent::KeyReleased { text });
}

fn labels(w: &AppWindow) -> Vec<String> {
    w.global::<TreeState>().get_rows().iter().map(|r| r.label.to_string()).collect()
}

fn cursor(w: &AppWindow) -> i32 {
    w.global::<TreeState>().get_cursor()
}

fn tab_titles(w: &AppWindow) -> Vec<String> {
    w.get_workspace_tabs().iter().map(|t| t.title.to_string()).collect()
}

fn toast_texts(w: &AppWindow) -> Vec<String> {
    w.get_toasts().iter().map(|t| t.text.to_string()).collect()
}

fn menu_labels(w: &AppWindow) -> Vec<String> {
    w.global::<TreeState>().get_menu_items().iter().map(|m| m.label.to_string()).collect()
}

/// Where the tree's rows sit. The sidebar is a fixed width and its rows a
/// fixed pitch, so a row is found by arithmetic: 28px rows, 2px apart, the
/// first centred 233px down once the fallback menu bar, title bar, workspace
/// header, tree header and filter box above it are counted. Each click is
/// checked against the cursor it should have set, so a layout change shows up
/// as a clear failure here rather than as a click on the wrong row.
fn row_pos(row: usize) -> LogicalPosition {
    LogicalPosition::new(120.0, 233.0 + 30.0 * row as f32)
}

fn pointer(w: &AppWindow, at: LogicalPosition, button: PointerEventButton) {
    w.window().dispatch_event(WindowEvent::PointerMoved { position: at });
    w.window().dispatch_event(WindowEvent::PointerPressed { position: at, button });
    w.window().dispatch_event(WindowEvent::PointerReleased { position: at, button });
}

fn click_row(w: &AppWindow, row: usize) {
    pointer(w, row_pos(row), PointerEventButton::Left);
}

fn double_click_row(w: &AppWindow, row: usize) {
    click_row(w, row);
    click_row(w, row);
}

fn right_click_row(w: &AppWindow, row: usize) {
    pointer(w, row_pos(row), PointerEventButton::Right);
}

/// Pick a menu entry by its label and close the menu the way the overlay does.
fn choose(w: &AppWindow, label_starts: &str) {
    let t = w.global::<TreeState>();
    let items: Vec<String> = menu_labels(w);
    let i = items
        .iter()
        .position(|l| l.starts_with(label_starts))
        .unwrap_or_else(|| panic!("no menu entry starting {label_starts:?} in {items:?}"));
    t.set_menu_visible(false);
    t.invoke_menu_chosen(i as i32);
}

fn tables_in(db: &PathBuf) -> Vec<String> {
    let conn = rusqlite::Connection::open(db).unwrap();
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0)).unwrap().map(|r| r.unwrap()).collect()
}

// -- the scenario ------------------------------------------------------------

async fn scenario(w: AppWindow, db: PathBuf, finished: Rc<Cell<bool>>) {
    let t = w.global::<TreeState>();

    // ---- connect: a database row, then tables and a view, with counts ------
    w.invoke_connect("shop-db".into());
    until("the tree to fill", || {
        w.get_screen() == Screen::Workspace
            && t.get_status() == TreeStatus::Populated
            && t.get_rows().row_count() == 5
    })
    .await;
    assert_eq!(labels(&w), ["shop.sqlite", "customers", "order_notes", "orders", "scratch"]);
    let rows: Vec<_> = t.get_rows().iter().collect();
    assert_eq!(rows[0].kind, TreeRowKind::Database);
    assert!(rows[0].active, "the connection's database is marked");
    assert_eq!(rows[2].kind, TreeRowKind::View, "a view is told from a table");
    assert_eq!(rows[1].kind, TreeRowKind::Table);
    assert_eq!(rows[1].detail, "~3");
    assert_eq!(rows[3].detail, "~2");
    assert_eq!(rows[2].detail, "", "a view has no row count");
    assert_eq!(t.get_table_count(), 4);

    // ---- live filter, no matches, clear -----------------------------------
    t.set_filter("ord".into());
    t.invoke_filter_edited("ord".into());
    until("the filter to narrow the tree", || t.get_rows().row_count() == 3).await;
    assert_eq!(labels(&w), ["shop.sqlite", "order_notes", "orders"]);
    t.set_filter("zzzz".into());
    t.invoke_filter_edited("zzzz".into());
    until("no matches", || t.get_status() == TreeStatus::NoMatches).await;
    assert_eq!(t.get_rows().row_count(), 0);
    t.invoke_clear_filter();
    until("the cleared tree", || t.get_rows().row_count() == 5).await;
    assert_eq!(t.get_filter(), "", "Clear search empties the box too");
    assert_eq!(t.get_status(), TreeStatus::Populated);

    // ---- a click highlights; a double click opens --------------------------
    assert_eq!(w.get_workspace_tabs().row_count(), 1, "just the starting query tab");
    click_row(&w, 3);
    until("the click to set the cursor", || cursor(&w) == 3).await;
    settle().await;
    assert_eq!(w.get_workspace_tabs().row_count(), 1, "a single click must not open a tab");
    // Slint's double-click window is about half a second; let the single
    // click age out so the next gesture is a fresh double click.
    sleep(Duration::from_millis(700)).await;
    double_click_row(&w, 3);
    until("a double click to open the table", || w.get_workspace_tabs().row_count() == 2).await;
    assert_eq!(tab_titles(&w), ["Query 1", "orders"]);

    // ---- keyboard: real key events, with the tree focused by the click ------
    press_key(&w, Key::Home);
    until("Home", || cursor(&w) == 0).await;
    press_key(&w, Key::DownArrow);
    until("Down", || cursor(&w) == 1).await;
    press_key(&w, Key::End);
    until("End", || cursor(&w) == 4).await;
    press_key(&w, Key::UpArrow);
    until("Up", || cursor(&w) == 3).await;
    // Left on a table goes to its parent; Left again closes the database.
    press_key(&w, Key::LeftArrow);
    until("Left to the parent", || cursor(&w) == 0).await;
    press_key(&w, Key::LeftArrow);
    until("Left to collapse", || t.get_rows().row_count() == 1).await;
    // Right opens it, and again steps into the first child.
    press_key(&w, Key::RightArrow);
    until("Right to expand", || t.get_rows().row_count() == 5).await;
    press_key(&w, Key::RightArrow);
    until("Right into the first child", || cursor(&w) == 1).await;
    // Enter opens the table under the cursor.
    press_key(&w, Key::Return);
    until("Enter to open a table", || tab_titles(&w).contains(&"customers".to_string())).await;
    assert_eq!(tab_titles(&w), ["Query 1", "orders", "customers"]);
    // Enter on a table that is already open only focuses it.
    press_key(&w, Key::Return);
    settle().await;
    assert_eq!(w.get_workspace_tabs().row_count(), 3);

    // ---- right click: the right menu on the right row ------------------------
    right_click_row(&w, 3);
    until("the table menu", || t.get_menu_visible()).await;
    assert_eq!(cursor(&w), 3, "the right-clicked row is highlighted");
    assert_eq!(
        menu_labels(&w),
        [
            "View Data",
            "Generate SELECT",
            "Generate INSERT",
            "Copy Name",
            "Edit Table…",
            "Refresh",
            "Drop Table…"
        ]
    );
    let items: Vec<_> = t.get_menu_items().iter().collect();
    assert!(items.last().unwrap().danger, "Drop Table is drawn as destructive");
    t.set_menu_visible(false);
    t.invoke_menu_dismissed();

    right_click_row(&w, 2);
    until("the view menu", || t.get_menu_visible()).await;
    assert_eq!(menu_labels(&w), ["View Data", "Generate SELECT", "Copy Name", "Refresh"]);
    t.set_menu_visible(false);
    t.invoke_menu_dismissed();

    right_click_row(&w, 0);
    until("the database menu", || t.get_menu_visible()).await;
    assert_eq!(menu_labels(&w), ["Refresh"], "SQLite cannot switch or create databases");
    t.set_menu_visible(false);
    t.invoke_menu_dismissed();

    // ---- Generate SELECT / INSERT: into a new editor, never run ---------------
    right_click_row(&w, 3);
    until("the menu", || t.get_menu_visible()).await;
    choose(&w, "Generate SELECT");
    until("the SELECT in an editor", || {
        w.get_workspace_active_is_query() && w.get_workspace_editor_text().starts_with("SELECT")
    })
    .await;
    assert_eq!(w.get_workspace_editor_text(), "SELECT *\nFROM \"orders\"\nLIMIT 100;");
    settle().await;
    assert!(!w.get_workspace_has_result() && !w.get_workspace_running(), "generated SQL must not run");
    assert_eq!(w.get_workspace_tabs().row_count(), 4);

    right_click_row(&w, 3);
    until("the menu", || t.get_menu_visible()).await;
    choose(&w, "Generate INSERT");
    until("the INSERT in an editor", || w.get_workspace_editor_text().starts_with("INSERT")).await;
    assert_eq!(
        w.get_workspace_editor_text(),
        "INSERT INTO \"orders\"\n  (\"customer_id\", \"note\")\nVALUES\n  (NULL, NULL);",
        "the auto-numbered id is left out and every name is quoted"
    );
    assert!(!w.get_workspace_has_result(), "generated SQL must not run");
    let orders_rows: i64 = rusqlite::Connection::open(&db)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM orders", [], |r| r.get(0))
        .unwrap();
    assert_eq!(orders_rows, 2, "Generate INSERT inserted nothing");

    right_click_row(&w, 3);
    until("the menu", || t.get_menu_visible()).await;
    choose(&w, "Copy Name");
    until("the copy toast", || toast_texts(&w).iter().any(|x| x == "Copied \"orders\"")).await;

    // ---- the menu overlay itself, with real events -----------------------------
    // A press outside closes it and does nothing else.
    let tabs_before = w.get_workspace_tabs().row_count();
    right_click_row(&w, 3);
    until("the menu", || t.get_menu_visible()).await;
    pointer(&w, LogicalPosition::new(700.0, 600.0), PointerEventButton::Left);
    until("a press outside to close the menu", || !t.get_menu_visible()).await;
    assert_eq!(w.get_workspace_tabs().row_count(), tabs_before, "closing opens nothing");
    // Escape closes it too.
    right_click_row(&w, 3);
    until("the menu", || t.get_menu_visible()).await;
    press_key(&w, Key::Escape);
    until("Escape to close the menu", || !t.get_menu_visible()).await;
    // The keyboard goes back to the tree, not to nowhere.
    let before = cursor(&w);
    settle().await;
    press_key(&w, Key::UpArrow);
    until("the tree to take the arrow keys again", || cursor(&w) == before - 1).await;
    press_key(&w, Key::DownArrow);
    until("and again", || cursor(&w) == before).await;
    // The arrows and Enter pick an entry: the highlight starts on the first,
    // so two Downs reach Generate INSERT.
    right_click_row(&w, 3);
    until("the menu", || t.get_menu_visible()).await;
    press_key(&w, Key::DownArrow);
    press_key(&w, Key::DownArrow);
    press_key(&w, Key::Return);
    until("Enter to pick Generate INSERT", || {
        !t.get_menu_visible() && w.get_workspace_editor_text().starts_with("INSERT INTO \"orders\"")
    })
    .await;
    // A click on an entry picks it. The menu opens at the pointer: 4px of
    // padding, then 30px entries, so Generate SELECT (the second) is centred
    // 4 + 30 + 15 below the row's click point.
    right_click_row(&w, 3);
    until("the menu", || t.get_menu_visible()).await;
    let at = row_pos(3);
    pointer(&w, LogicalPosition::new(at.x + 100.0, at.y + 49.0), PointerEventButton::Left);
    until("a click to pick Generate SELECT", || {
        !t.get_menu_visible() && w.get_workspace_editor_text().starts_with("SELECT *\nFROM \"orders\"")
    })
    .await;

    // ---- Edit Table and the header's new-table button use the table dialog ----
    right_click_row(&w, 3);
    until("the menu", || t.get_menu_visible()).await;
    choose(&w, "Edit Table");
    let d = w.global::<TableDialogState>();
    until("the edit dialog", || d.get_visible() && !d.get_loading() && d.get_mode() == 1).await;
    assert_eq!(d.get_title(), "Edit table");
    d.invoke_close();
    until("the dialog to close", || !d.get_visible()).await;

    t.invoke_create_table();
    until("the create wizard", || d.get_visible() && d.get_mode() == 0).await;
    assert_eq!(d.get_title(), "New table");
    d.invoke_close();
    until("the wizard to close", || !d.get_visible()).await;

    // ---- Drop Table: warns about dependents, needs the exact name -------------
    right_click_row(&w, 1);
    until("the customers menu", || t.get_menu_visible()).await;
    choose(&w, "Drop Table");
    until("the dependency check", || t.get_drop_visible() && !t.get_drop_checking()).await;
    assert_eq!(
        t.get_drop_warning(),
        "1 table has foreign keys to customers: orders.",
        "the warning names what points at the table"
    );
    assert!(!t.get_drop_offers_cascade(), "CASCADE is a Postgres option");
    assert!(!t.get_drop_can_drop(), "nothing typed yet");
    let sql = t.get_drop_sql().to_string();
    assert!(sql.contains("DROP TABLE") && sql.contains("\"customers\""), "{sql}");
    t.set_drop_typed("custom".into());
    t.invoke_drop_typed_edited("custom".into());
    assert!(!t.get_drop_can_drop(), "a partial name is not enough");
    t.invoke_drop_confirm();
    settle().await;
    assert!(tables_in(&db).contains(&"customers".to_string()), "a refused confirm must not drop");
    t.set_drop_typed("customers".into());
    t.invoke_drop_typed_edited("customers".into());
    assert!(t.get_drop_can_drop(), "the exact name unlocks it");
    t.invoke_drop_cancel();
    assert!(!t.get_drop_visible());
    assert!(tables_in(&db).contains(&"customers".to_string()), "cancel must not drop");

    // A table nothing depends on: open its tab first, so the drop has a tab to close.
    sleep(Duration::from_millis(700)).await;
    double_click_row(&w, 4);
    until("the scratch tab", || tab_titles(&w).contains(&"scratch".to_string())).await;
    right_click_row(&w, 4);
    until("the scratch menu", || t.get_menu_visible()).await;
    choose(&w, "Drop Table");
    until("the dependency check", || t.get_drop_visible() && !t.get_drop_checking()).await;
    assert_eq!(t.get_drop_warning(), "", "nothing depends on scratch");
    t.set_drop_typed("scratch".into());
    t.invoke_drop_typed_edited("scratch".into());
    assert!(t.get_drop_can_drop());
    t.invoke_drop_confirm();
    until("the drop to finish", || !t.get_drop_visible()).await;
    assert!(!tables_in(&db).contains(&"scratch".to_string()), "the table is gone");
    until("the tree to refresh", || !labels(&w).contains(&"scratch".to_string())).await;
    assert!(!tab_titles(&w).contains(&"scratch".to_string()), "its tab closes with it");
    assert!(toast_texts(&w).iter().any(|x| x == "Dropped table scratch"));

    // ---- a failed drop stays open and says why --------------------------------
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute("CREATE TABLE late (id INTEGER)", []).unwrap();
    }
    t.invoke_refresh();
    until("a table created behind the app's back to appear", || {
        labels(&w).contains(&"late".to_string())
    })
    .await;
    let late = labels(&w).iter().position(|l| l == "late").unwrap();
    // Remove it again before the drop runs, so the server refuses.
    rusqlite::Connection::open(&db).unwrap().execute("DROP TABLE late", []).unwrap();
    right_click_row(&w, late);
    until("the late menu", || t.get_menu_visible()).await;
    choose(&w, "Drop Table");
    until("the dependency check", || t.get_drop_visible() && !t.get_drop_checking()).await;
    t.set_drop_typed("late".into());
    t.invoke_drop_typed_edited("late".into());
    t.invoke_drop_confirm();
    until("the failure", || !t.get_drop_error().is_empty() && !t.get_drop_running()).await;
    assert!(t.get_drop_visible(), "a failed drop keeps the dialog open");
    t.invoke_drop_cancel();

    finished.set(true);
    slint::quit_event_loop().unwrap();
}

#[test]
fn browse_filter_navigate_generate_and_drop_in_a_real_database() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-schema-tree-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let db = dir.join("shop.sqlite");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
             CREATE TABLE orders (
                 id INTEGER PRIMARY KEY,
                 customer_id INTEGER REFERENCES customers(id),
                 note TEXT
             );
             CREATE TABLE scratch (id INTEGER PRIMARY KEY, v TEXT);
             CREATE VIEW order_notes AS SELECT id, note FROM orders;
             INSERT INTO customers (name) VALUES ('Ada'), ('Grace'), ('Linus');
             INSERT INTO orders (customer_id, note) VALUES (1, 'first'), (2, 'second');",
        )
        .unwrap();
    }
    {
        let mut profile = ConnectionProfile::new(
            "shop-db".to_string(),
            "Shop".to_string(),
            DbDriver::Sqlite,
            String::new(),
            0,
            String::new(),
        );
        profile.database = Some(db.to_string_lossy().to_string());
        let mut state = AppState::new();
        state.add_profile(profile);
        state.save_profiles_to_store().unwrap();
    }

    let w = dbhive_slint::app::build().unwrap();
    w.show().unwrap();

    let finished = Rc::new(Cell::new(false));
    slint::spawn_local(scenario(w.clone_strong(), db.clone(), finished.clone())).unwrap();
    // A failed assertion in the scenario panics inside the event loop and
    // comes out of here.
    w.run().unwrap();
    assert!(finished.get(), "the event loop ended before the scenario did");
    let _ = std::fs::remove_dir_all(&dir);
}
