//! The schema browser against a real Postgres: several schemas, tables that
//! point at each other across them, a second database, routines.
//!
//! SQLite has one schema, so `schema_tree_controller.rs` cannot show what a
//! Postgres connection needs: schemas as nodes that load their tables on first
//! expand, a table opened from a schema other than the current one, a drop
//! warning that finds a foreign key from another schema and offers CASCADE,
//! switching to another database, creating one, and listing routines.
//!
//! It starts its own throwaway cluster (`initdb` into a temp directory, trust
//! authentication, a free loopback port, a socket directory inside the temp
//! directory) and never touches any Postgres the developer is running. When
//! no `initdb` can be found it says so and passes without testing anything;
//! set `DBHIVE_PG_BIN` to the directory holding the binaries to point at one.
//!
//! Every store is redirected to a temp directory, so this never touches the
//! developer's own saved connections or history.

use std::cell::Cell;
use std::future::poll_fn;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::task::Poll;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{
    AppWindow, InspectorBridge, Screen, TableDialogState, TreeRowKind, TreeState, TreeStatus,
};
use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, Model, SharedString, Timer, TimerMode};

// -- a throwaway cluster ------------------------------------------------------

fn pg_bin() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var("DBHIVE_PG_BIN") {
        candidates.push(dir.into());
    }
    if let Ok(path) = std::env::var("PATH") {
        candidates.extend(std::env::split_paths(&path));
    }
    for root in ["/opt/homebrew/opt", "/usr/local/opt"] {
        if let Ok(entries) = std::fs::read_dir(root) {
            let mut found: Vec<PathBuf> = entries
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("postgresql"))
                .map(|e| e.path().join("bin"))
                .collect();
            found.sort();
            candidates.extend(found.into_iter().rev());
        }
    }
    if let Ok(entries) = std::fs::read_dir("/usr/lib/postgresql") {
        let mut found: Vec<PathBuf> = entries.flatten().map(|e| e.path().join("bin")).collect();
        found.sort();
        candidates.extend(found.into_iter().rev());
    }
    candidates.into_iter().find(|d| {
        d.join("initdb").is_file() && d.join("pg_ctl").is_file() && d.join("psql").is_file()
    })
}

struct Cluster {
    bin: PathBuf,
    dir: PathBuf,
    sock: PathBuf,
    port: u16,
}

impl Cluster {
    fn start(bin: PathBuf, dir: &Path) -> Cluster {
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let data = dir.join("data");
        // A unix socket path is limited to about 100 bytes, which the temp
        // directory on macOS can use up on its own.
        let sock = PathBuf::from("/tmp").join(format!("dbpg{}", std::process::id()));
        std::fs::create_dir_all(&sock).unwrap();
        let init = Command::new(bin.join("initdb"))
            .args(["-D"])
            .arg(&data)
            .args([
                "-U",
                "postgres",
                "-A",
                "trust",
                "-E",
                "UTF8",
                "--no-locale",
                "--no-sync",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .expect("run initdb");
        assert!(
            init.status.success(),
            "initdb failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );
        let options = format!(
            "-p {port} -c listen_addresses=127.0.0.1 -c unix_socket_directories={} -c fsync=off",
            sock.display()
        );
        let start = Command::new(bin.join("pg_ctl"))
            .arg("-D")
            .arg(&data)
            .args(["-o", &options, "-l"])
            .arg(dir.join("server.log"))
            .args(["-w", "start"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .expect("run pg_ctl");
        assert!(
            start.status.success(),
            "pg_ctl start failed: {}",
            String::from_utf8_lossy(&start.stderr)
        );
        Cluster {
            bin,
            dir: dir.to_path_buf(),
            sock,
            port,
        }
    }

    /// Run SQL through psql, failing the test on any error.
    fn sql(&self, database: &str, sql: &str) -> String {
        let out = Command::new(self.bin.join("psql"))
            .args([
                "-h",
                "127.0.0.1",
                "-p",
                &self.port.to_string(),
                "-U",
                "postgres",
                "-d",
                database,
            ])
            .args(["-X", "-q", "-t", "-A", "-v", "ON_ERROR_STOP=1", "-c", sql])
            .output()
            .expect("run psql");
        assert!(
            out.status.success(),
            "psql failed on {sql:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }
}

impl Drop for Cluster {
    fn drop(&mut self) {
        let _ = Command::new(self.bin.join("pg_ctl"))
            .arg("-D")
            .arg(self.dir.join("data"))
            .args(["-m", "immediate", "-w", "stop"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = std::fs::remove_dir_all(&self.dir);
        let _ = std::fs::remove_dir_all(&self.sock);
    }
}

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

async fn until(what: &str, cond: impl Fn() -> bool) {
    let started = Instant::now();
    while !cond() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "timed out waiting for: {what}"
        );
        sleep(Duration::from_millis(10)).await;
    }
}

async fn settle() {
    for _ in 0..8 {
        sleep(Duration::from_millis(10)).await;
    }
}

fn press_key(w: &AppWindow, k: Key) {
    let text: SharedString = k.into();
    w.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    w.window().dispatch_event(WindowEvent::KeyReleased { text });
}

fn labels(w: &AppWindow) -> Vec<String> {
    w.global::<TreeState>()
        .get_rows()
        .iter()
        .map(|r| r.label.to_string())
        .collect()
}

fn row_of(w: &AppWindow, label: &str) -> usize {
    labels(w)
        .iter()
        .position(|l| l == label)
        .unwrap_or_else(|| panic!("no row {label:?} in {:?}", labels(w)))
}

fn tab_titles(w: &AppWindow) -> Vec<String> {
    w.get_workspace_tabs()
        .iter()
        .map(|t| t.title.to_string())
        .collect()
}

fn toast_texts(w: &AppWindow) -> Vec<String> {
    w.get_toasts().iter().map(|t| t.text.to_string()).collect()
}

fn menu_labels(w: &AppWindow) -> Vec<String> {
    w.global::<TreeState>()
        .get_menu_items()
        .iter()
        .map(|m| m.label.to_string())
        .collect()
}

fn routine_labels(w: &AppWindow) -> Vec<String> {
    w.global::<TreeState>()
        .get_routines()
        .iter()
        .map(|r| r.label.to_string())
        .collect()
}

/// Row `i`'s centre (see `schema_tree_controller.rs` for the arithmetic).
fn row_pos(row: usize) -> LogicalPosition {
    LogicalPosition::new(120.0, 233.0 + 30.0 * row as f32)
}

fn pointer(w: &AppWindow, at: LogicalPosition, button: PointerEventButton) {
    w.window()
        .dispatch_event(WindowEvent::PointerMoved { position: at });
    w.window().dispatch_event(WindowEvent::PointerPressed {
        position: at,
        button,
    });
    w.window().dispatch_event(WindowEvent::PointerReleased {
        position: at,
        button,
    });
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

fn choose(w: &AppWindow, label_starts: &str) {
    let t = w.global::<TreeState>();
    let items = menu_labels(w);
    let i = items
        .iter()
        .position(|l| l.starts_with(label_starts))
        .unwrap_or_else(|| panic!("no menu entry starting {label_starts:?} in {items:?}"));
    t.set_menu_visible(false);
    t.invoke_menu_chosen(i as i32);
}

// -- the scenario ------------------------------------------------------------

async fn scenario(w: AppWindow, pg: Rc<Cluster>, finished: Rc<Cell<bool>>) {
    let t = w.global::<TreeState>();

    // ---- connect: databases, then schemas, with only the current one loaded ----
    w.invoke_connect("pg".into());
    until("the tree to fill", || {
        w.get_screen() == Screen::Workspace
            && t.get_status() == TreeStatus::Populated
            && t.get_rows().row_count() == 9
    })
    .await;
    assert_eq!(
        labels(&w),
        [
            "app",
            "public",
            "customers",
            "order_totals",
            "orders",
            "audit",
            "billing",
            "other",
            "postgres"
        ],
        "databases, then the current database's schemas (system schemas hidden), the \
         current schema open with its tables and a view"
    );
    let rows: Vec<_> = t.get_rows().iter().collect();
    assert_eq!(rows[0].kind, TreeRowKind::Database);
    assert!(
        rows[0].active && !rows[7].active,
        "the connected database is marked"
    );
    assert_eq!(rows[1].kind, TreeRowKind::Schema);
    assert!(rows[1].active, "the schema other tools act on is marked");
    assert_eq!(rows[3].kind, TreeRowKind::View);
    assert_eq!(rows[2].kind, TreeRowKind::Table);
    assert!(
        !labels(&w).contains(&"invoices".to_string()),
        "billing is not loaded until it is opened"
    );

    // ---- a search loads the schemas it has not seen ---------------------------
    t.set_filter("invoic".into());
    t.invoke_filter_edited("invoic".into());
    until("the search to find a table in an unopened schema", || {
        labels(&w) == ["app", "billing", "invoices"]
    })
    .await;
    t.invoke_clear_filter();
    until("the full tree back", || t.get_rows().row_count() == 9).await;
    assert!(
        !labels(&w).contains(&"invoices".to_string()),
        "searching did not open billing for good"
    );

    // ---- expand a schema with the keyboard: its tables arrive ------------------
    click_row(&w, row_of(&w, "billing"));
    until("the click to focus billing", || t.get_cursor() == 6).await;
    press_key(&w, Key::RightArrow);
    until("billing's tables", || {
        labels(&w).contains(&"invoices".to_string())
    })
    .await;
    assert_eq!(
        labels(&w),
        [
            "app",
            "public",
            "customers",
            "order_totals",
            "orders",
            "audit",
            "billing",
            "invoices",
            "payments",
            "other",
            "postgres"
        ]
    );

    // ---- a table from another schema opens in its own tab ----------------------
    sleep(Duration::from_millis(700)).await;
    double_click_row(&w, row_of(&w, "invoices"));
    until("the billing.invoices tab", || {
        tab_titles(&w).contains(&"billing.invoices".to_string())
    })
    .await;
    until("its rows", || {
        !w.get_workspace_running() && w.get_workspace_rows().row_count() == 4
    })
    .await;
    assert_eq!(w.get_workspace_result_title(), "invoices");
    sleep(Duration::from_millis(700)).await;
    double_click_row(&w, row_of(&w, "customers"));
    until("the customers tab", || {
        tab_titles(&w).contains(&"customers".to_string())
    })
    .await;
    until("its rows", || {
        !w.get_workspace_running() && w.get_workspace_rows().row_count() == 3
    })
    .await;

    // ---- Create Table on a schema makes it the current one ------------------------
    right_click_row(&w, row_of(&w, "billing"));
    until("the schema menu", || t.get_menu_visible()).await;
    assert_eq!(menu_labels(&w), ["Create Table…", "Refresh"]);
    choose(&w, "Create Table");
    let d = w.global::<TableDialogState>();
    until("the create wizard", || d.get_visible() && d.get_mode() == 0).await;
    assert_eq!(
        d.get_subtitle(),
        "in billing",
        "the wizard creates the table in the schema it was raised on"
    );
    d.invoke_close();
    until("the wizard to close", || !d.get_visible()).await;
    assert!(
        t.get_rows()
            .iter()
            .any(|r| r.active && r.label == "billing"),
        "billing is now the marked schema"
    );
    // The customers tab was opened while public was current. It must still be public's.
    let customers_tab = tab_titles(&w)
        .iter()
        .position(|x| x == "customers")
        .unwrap();
    w.invoke_workspace_tab_selected(customers_tab as i32);
    settle().await;
    w.global::<InspectorBridge>().invoke_refresh();
    settle().await;
    until("customers to reload from public", || {
        !w.get_workspace_running()
    })
    .await;
    assert_eq!(
        w.get_workspace_rows().row_count(),
        3,
        "a table tab keeps the schema it was opened in when another becomes current"
    );
    assert_eq!(
        w.get_workspace_rows_error(),
        "",
        "{}",
        w.get_workspace_rows_error()
    );

    // ---- Edit Table works on a table outside the current schema -----------------------
    right_click_row(&w, row_of(&w, "orders"));
    until("the table menu", || t.get_menu_visible()).await;
    choose(&w, "Edit Table");
    until("the edit dialog", || {
        d.get_visible() && !d.get_loading() && d.get_mode() == 1
    })
    .await;
    assert_eq!(d.get_subtitle(), "public.orders");
    d.invoke_close();
    until("the dialog to close", || !d.get_visible()).await;

    // ---- Generate SELECT is qualified ------------------------------------------------------
    right_click_row(&w, row_of(&w, "payments"));
    until("the payments menu", || t.get_menu_visible()).await;
    choose(&w, "Generate SELECT");
    until("the SELECT", || {
        w.get_workspace_editor_text().starts_with("SELECT")
    })
    .await;
    assert_eq!(
        w.get_workspace_editor_text(),
        "SELECT *\nFROM \"billing\".\"payments\"\nLIMIT 100;"
    );

    // ---- Procedures & Functions ---------------------------------------------------------------
    assert!(t.get_routines_supported(), "Postgres has stored routines");
    t.invoke_routines_open();
    until("the routines list", || {
        t.get_routines_visible() && t.get_routines_status() == TreeStatus::Populated
    })
    .await;
    assert_eq!(
        routine_labels(&w),
        ["billing", "close_month", "public", "add_numbers"]
    );
    assert_eq!(t.get_routines_count(), 2);
    // Search narrows it.
    t.set_routines_filter("add".into());
    t.invoke_routines_search("add".into());
    until("the search", || {
        routine_labels(&w) == ["public", "add_numbers"]
    })
    .await;
    t.set_routines_filter("zzzz".into());
    t.invoke_routines_search("zzzz".into());
    until("no matches", || {
        t.get_routines_status() == TreeStatus::NoMatches
    })
    .await;
    t.set_routines_filter("".into());
    t.invoke_routines_search("".into());
    until("everything again", || routine_labels(&w).len() == 4).await;
    // Pick a function: its source is read.
    let add = routine_labels(&w)
        .iter()
        .position(|l| l == "add_numbers")
        .unwrap();
    t.invoke_routine_pick(add as i32);
    until("the definition", || {
        !t.get_routine_def_loading() && !t.get_routine_definition().is_empty()
    })
    .await;
    assert_eq!(t.get_routine_title(), "public.add_numbers");
    assert!(
        t.get_routine_definition()
            .contains("CREATE OR REPLACE FUNCTION public.add_numbers"),
        "{}",
        t.get_routine_definition()
    );
    assert_eq!(
        t.get_routine_signature(),
        "(a integer, b integer) returns integer"
    );
    assert_eq!(t.get_routine_meta(), "function · sql");
    // A heading is not a routine.
    t.invoke_routine_pick(0);
    settle().await;
    assert_eq!(
        t.get_routine_title(),
        "public.add_numbers",
        "picking a heading changes nothing"
    );
    // A procedure.
    let close = routine_labels(&w)
        .iter()
        .position(|l| l == "close_month")
        .unwrap();
    t.invoke_routine_pick(close as i32);
    until("the procedure", || {
        !t.get_routine_def_loading()
            && t.get_routine_definition()
                .contains("PROCEDURE billing.close_month")
    })
    .await;
    assert_eq!(t.get_routine_meta(), "procedure · plpgsql");
    // Open in editor: a new tab, text ending in a semicolon, nothing run.
    let tabs_before = w.get_workspace_tabs().row_count();
    t.invoke_routine_to_editor();
    until("the definition in an editor", || {
        w.get_workspace_tabs().row_count() == tabs_before + 1
            && w.get_workspace_editor_text()
                .contains("PROCEDURE billing.close_month")
    })
    .await;
    assert!(w.get_workspace_editor_text().trim_end().ends_with(';'));
    assert!(
        !t.get_routines_visible(),
        "the panel closes so the editor shows"
    );
    settle().await;
    assert!(
        !w.get_workspace_has_result() && !w.get_workspace_running(),
        "a definition is never run"
    );

    // ---- Drop Table: nothing depends on orders, so there is no warning and no CASCADE --------
    right_click_row(&w, row_of(&w, "orders"));
    until("the orders menu", || t.get_menu_visible()).await;
    choose(&w, "Drop Table");
    until("the dependency check", || {
        t.get_drop_visible() && !t.get_drop_checking()
    })
    .await;
    assert_eq!(t.get_drop_warning(), "");
    assert!(
        !t.get_drop_offers_cascade(),
        "CASCADE is not offered when nothing depends on the table"
    );
    t.invoke_drop_cancel();

    // ---- Drop Table: a dependent in another schema, then CASCADE ------------------------------
    right_click_row(&w, row_of(&w, "customers"));
    until("the customers menu", || t.get_menu_visible()).await;
    choose(&w, "Drop Table");
    until("the dependency check", || {
        t.get_drop_visible() && !t.get_drop_checking()
    })
    .await;
    assert_eq!(
        t.get_drop_warning(),
        "2 tables have foreign keys to customers: billing.invoices, public.orders.",
        "the warning finds the foreign key from another schema too"
    );
    assert!(
        t.get_drop_offers_cascade(),
        "CASCADE is offered once the warning is up"
    );
    assert!(!t.get_drop_cascade(), "and starts off");
    assert!(!t.get_drop_sql().contains("CASCADE"));
    t.set_drop_typed("customers".into());
    t.invoke_drop_typed_edited("customers".into());
    assert!(t.get_drop_can_drop());
    // Without CASCADE the server refuses, and the dialog stays open saying so.
    t.invoke_drop_confirm();
    until("the server's refusal", || {
        !t.get_drop_error().is_empty() && !t.get_drop_running()
    })
    .await;
    assert!(t.get_drop_visible());
    assert!(
        t.get_drop_error().contains("depend"),
        "{}",
        t.get_drop_error()
    );
    assert_eq!(
        pg.sql(
            "app",
            "SELECT count(*) FROM pg_tables WHERE tablename = 'customers'"
        ),
        "1"
    );
    // With CASCADE it goes, and the dependent tables stay without the keys.
    t.invoke_drop_cascade_toggled();
    assert!(t.get_drop_cascade() && t.get_drop_sql().contains("CASCADE"));
    t.invoke_drop_confirm();
    until("the drop to finish", || !t.get_drop_visible()).await;
    assert_eq!(
        pg.sql(
            "app",
            "SELECT count(*) FROM pg_tables WHERE tablename = 'customers'"
        ),
        "0"
    );
    assert_eq!(
        pg.sql(
            "app",
            "SELECT count(*) FROM pg_constraint WHERE contype = 'f'"
        ),
        "1",
        "only the key from billing.payments to billing.invoices is left"
    );
    assert_eq!(
        pg.sql("app", "SELECT count(*) FROM billing.invoices"),
        "4",
        "CASCADE dropped the keys, not the rows"
    );
    until("the tree to refresh", || {
        !labels(&w).contains(&"customers".to_string())
    })
    .await;
    assert!(
        !tab_titles(&w).contains(&"customers".to_string()),
        "its tab closes with it"
    );

    // ---- Create Database ------------------------------------------------------------------------------
    right_click_row(&w, row_of(&w, "app"));
    until("the database menu", || t.get_menu_visible()).await;
    assert_eq!(menu_labels(&w), ["Refresh", "Create Database…"]);
    choose(&w, "Create Database");
    until("the dialog", || t.get_create_db_visible()).await;
    assert!(!t.get_create_db_can_create(), "nothing typed yet");
    t.set_create_db_name("1st db".into());
    t.invoke_create_db_edited("1st db".into());
    assert!(!t.get_create_db_can_create());
    assert!(!t.get_create_db_error().is_empty(), "the name is explained");
    t.set_create_db_name("reporting".into());
    t.invoke_create_db_edited("reporting".into());
    assert!(t.get_create_db_can_create());
    assert_eq!(t.get_create_db_sql(), "CREATE DATABASE \"reporting\"");
    // A name that exists already: the server's answer shows and the dialog stays.
    t.set_create_db_name("other".into());
    t.invoke_create_db_edited("other".into());
    t.invoke_create_db_confirm();
    until("the server's refusal", || {
        t.get_create_db_error().contains("already exists") && !t.get_create_db_running()
    })
    .await;
    assert!(t.get_create_db_visible());
    t.set_create_db_name("reporting".into());
    t.invoke_create_db_edited("reporting".into());
    assert_eq!(
        t.get_create_db_error(),
        "",
        "editing the name clears the old refusal"
    );
    t.invoke_create_db_confirm();
    until("the dialog to close", || !t.get_create_db_visible()).await;
    until("the new database to be listed", || {
        labels(&w).contains(&"reporting".to_string())
    })
    .await;
    assert_eq!(
        pg.sql(
            "postgres",
            "SELECT count(*) FROM pg_database WHERE datname = 'reporting'"
        ),
        "1"
    );
    assert!(toast_texts(&w)
        .iter()
        .any(|x| x == "Created database reporting"));

    // ---- switch database: open tables are closed, the tree is the new one's ---------------------------
    assert!(tab_titles(&w).iter().any(|x| x == "billing.invoices"));
    sleep(Duration::from_millis(700)).await;
    double_click_row(&w, row_of(&w, "other"));
    until("the switch", || {
        labels(&w).first().is_some_and(|l| l == "app")
            && t.get_rows().iter().any(|r| r.active && r.label == "other")
    })
    .await;
    until("the new database's tables", || {
        labels(&w).contains(&"elsewhere".to_string())
    })
    .await;
    assert!(
        !labels(&w).contains(&"billing".to_string()),
        "the other database has no billing schema"
    );
    assert!(
        tab_titles(&w).iter().all(|x| x.starts_with("Query")),
        "tables of the database we left are closed: {:?}",
        tab_titles(&w)
    );
    assert!(toast_texts(&w).iter().any(|x| x == "Switched to other"));
    // And its tables open against the new connection.
    sleep(Duration::from_millis(700)).await;
    double_click_row(&w, row_of(&w, "elsewhere"));
    until("elsewhere's rows", || {
        tab_titles(&w).contains(&"elsewhere".to_string())
            && !w.get_workspace_running()
            && w.get_workspace_rows().row_count() == 2
    })
    .await;

    finished.set(true);
    slint::quit_event_loop().unwrap();
}

#[test]
fn schemas_cross_schema_drop_database_switch_and_routines_on_a_real_postgres() {
    let Some(bin) = pg_bin() else {
        eprintln!(
            "skipped: no initdb/pg_ctl/psql found (set DBHIVE_PG_BIN to a Postgres bin directory)"
        );
        return;
    };
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-pgtree-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, dir.join("store"));
    std::fs::create_dir_all(dir.join("store")).unwrap();

    let pg = Rc::new(Cluster::start(bin, &dir.join("pg")));
    pg.sql("postgres", "CREATE DATABASE app");
    pg.sql("postgres", "CREATE DATABASE other");
    pg.sql(
        "app",
        "CREATE SCHEMA billing;
         CREATE SCHEMA audit;
         CREATE TABLE public.customers (id serial PRIMARY KEY, name text NOT NULL);
         CREATE TABLE public.orders (id serial PRIMARY KEY, customer_id int REFERENCES public.customers(id), total int);
         CREATE TABLE billing.invoices (id serial PRIMARY KEY, customer_id int REFERENCES public.customers(id), amount int);
         CREATE TABLE billing.payments (id serial PRIMARY KEY, invoice_id int REFERENCES billing.invoices(id));
         CREATE VIEW public.order_totals AS SELECT customer_id, sum(total) AS total FROM public.orders GROUP BY customer_id;
         INSERT INTO public.customers (name) VALUES ('Ada'), ('Grace'), ('Linus');
         INSERT INTO public.orders (customer_id, total) VALUES (1, 10), (2, 20);
         INSERT INTO billing.invoices (customer_id, amount) VALUES (1, 5), (1, 6), (2, 7), (3, 8);
         CREATE FUNCTION public.add_numbers(a int, b int) RETURNS int LANGUAGE sql AS 'SELECT a + b';
         CREATE PROCEDURE billing.close_month(period date) LANGUAGE plpgsql AS $$ BEGIN NULL; END $$;",
    );
    pg.sql("other", "CREATE TABLE public.elsewhere (id serial PRIMARY KEY, v text); INSERT INTO public.elsewhere (v) VALUES ('a'), ('b');");

    {
        let mut profile = ConnectionProfile::new(
            "pg".to_string(),
            "Local Postgres".to_string(),
            DbDriver::Postgres,
            "127.0.0.1".to_string(),
            pg.port,
            "postgres".to_string(),
        );
        profile.database = Some("app".to_string());
        let mut state = AppState::new();
        state.add_profile(profile);
        state.save_profiles_to_store().unwrap();
    }

    // This server trusts local connections and needs no password; say one is
    // stored so the shell connects instead of opening the password prompt.
    let secrets = std::sync::Arc::new(dbhive_slint::app::MemorySecrets::default());
    secrets.stored.lock().unwrap().insert("pg".to_string());
    dbhive_slint::app::use_secret_store(secrets);

    let w = dbhive_slint::app::build().unwrap();
    w.show().unwrap();

    let finished = Rc::new(Cell::new(false));
    slint::spawn_local(scenario(w.clone_strong(), pg.clone(), finished.clone())).unwrap();
    // A failed assertion in the scenario panics inside the event loop and
    // comes out of here (and `pg` still stops the cluster on the way out).
    w.run().unwrap();
    assert!(
        finished.get(),
        "the event loop ended before the scenario did"
    );
    drop(pg);
    let _ = std::fs::remove_dir_all(&dir);
}
