//! Import / export and backup / restore driven end to end: the real window,
//! the real controller, a real SQLite file.
//!
//! The flow: connect, open the Import panel from the palette command with a
//! table tab in front, import a CSV (a quoted cell with a comma in it, a
//! mapping read from the table), export the table to CSV, write a SQL dump,
//! run a SQL file, then see a failing SQL file roll back. Then the backup
//! panel: create a backup, damage the database behind the app's back,
//! restore, and watch the restored rows come back through the app's own
//! connection. Last, delete the backup behind a confirmation.
//!
//! Every store, every export and every backup goes to a temp directory
//! (`DBHIVE_DATA_DIR` is set before anything is built), so this never
//! touches the developer's own connections or backups.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::{BackupState, BackupView, ImportExportState, Screen, Tone};
use slint::{ComponentHandle, Model, Timer, TimerMode};

fn count(db: &Path, sql: &str) -> i64 {
    rusqlite::Connection::open(db).unwrap().query_row(sql, [], |r| r.get(0)).unwrap()
}

fn toast_texts(w: &dbhive_slint::AppWindow) -> Vec<String> {
    w.get_toasts().iter().map(|t| t.text.to_string()).collect()
}

#[test]
fn import_export_and_backup_restore_round_trip_on_a_real_sqlite_file() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-datatools-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let out = dir.join("out");
    let backups = dir.join("my-backups");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::create_dir_all(&backups).unwrap();

    let db = dir.join("shop.sqlite");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute("CREATE TABLE widgets (id INTEGER PRIMARY KEY, name TEXT, price REAL)", []).unwrap();
        conn.execute("INSERT INTO widgets (id, name, price) VALUES (1, 'sprocket', 1.25), (2, 'gizmo', 2.5)", [])
            .unwrap();
    }

    // The CSV: a header, a plain row, and a cell with a comma and a quote.
    let csv = dir.join("new_widgets.csv");
    std::fs::write(&csv, "id,name,price\n3,cog,1.5\n4,lever,2.25\n5,\"gear, \"\"large\"\"\",9.99\n").unwrap();
    let csv_path = csv.to_string_lossy().to_string();

    // A SQL file that works, and one that fails half way.
    let good_sql = dir.join("extras.sql");
    std::fs::write(
        &good_sql,
        "CREATE TABLE extras (id INTEGER PRIMARY KEY, label TEXT);\n\
         INSERT INTO extras (id, label) VALUES (1, 'one');\n\
         INSERT INTO extras (id, label) VALUES (2, 'two');\n",
    )
    .unwrap();
    let bad_sql = dir.join("bad.sql");
    std::fs::write(
        &bad_sql,
        "INSERT INTO extras (id, label) VALUES (3, 'three');\n\
         INSERT INTO no_such_table (id) VALUES (1);\n",
    )
    .unwrap();

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
    let failure: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let step = Rc::new(Cell::new(0u32));
    let started = Instant::now();
    let timer = Timer::default();
    // The path of the backup file the flow creates, for later steps.
    let backup_file: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));

    let (weak, fail, step2) = (w.as_weak(), failure.clone(), step.clone());
    let (db2, out2, backups2, backup_file2) = (db.clone(), out.clone(), backups.clone(), backup_file.clone());
    let (good2, bad2) = (good_sql.clone(), bad_sql.clone());
    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        let w = weak.upgrade().unwrap();
        let ie = w.global::<ImportExportState>();
        let bk = w.global::<BackupState>();
        let bail = |msg: String| {
            *fail.borrow_mut() = Some(msg);
            slint::quit_event_loop().unwrap();
        };
        if started.elapsed() > Duration::from_secs(60) {
            bail(format!("timed out at step {}", step2.get()));
            return;
        }
        match step2.get() {
            // Connect, then bring a table tab to the front: the panel should
            // pick that table as its target.
            0 => {
                w.invoke_connect("shop-db".into());
                step2.set(1);
            }
            1 if w.get_screen() == Screen::Workspace && w.get_workspace_tables().row_count() == 1 => {
                w.invoke_workspace_select_table("widgets".into());
                step2.set(2);
            }
            // Open the panel through the palette command, not the menu.
            2 if w.get_workspace_tabs().row_count() >= 1 && !w.get_workspace_running() => {
                w.invoke_run_command("go-import-export".into());
                if !ie.get_visible() {
                    return bail("the go-import-export command did not open the panel".into());
                }
                if !ie.get_location().contains("Shop") {
                    return bail(format!("location should name the connection, got {:?}", ie.get_location()));
                }
                step2.set(3);
            }
            3 if ie.get_tables().row_count() == 1 => {
                if ie.get_tables().row_data(0).unwrap() != "widgets" {
                    return bail("the table list should hold the one table".into());
                }
                if ie.get_export_table_value() != "widgets" {
                    return bail(format!(
                        "an open table tab should be the export target, got {:?}",
                        ie.get_export_table_value()
                    ));
                }
                if !ie.get_export_path().contains("widgets_") || !ie.get_export_path().ends_with(".csv") {
                    return bail(format!("export path should be a date-stamped csv, got {:?}", ie.get_export_path()));
                }
                // Typing a bad path is reported before any work starts.
                ie.set_import_path("/no/such/file.pdf".into());
                ie.invoke_import_path_edited("/no/such/file.pdf".into());
                ie.invoke_import_read_preview();
                if !ie.get_import_path_error().contains("Unsupported file type") {
                    return bail(format!("a .pdf should be refused, got {:?}", ie.get_import_path_error()));
                }
                if ie.get_step() != 0 {
                    return bail("a refused file must not leave the file step".into());
                }
                // The real file.
                let p = csv_path.clone();
                ie.set_import_path(p.clone().into());
                ie.invoke_import_path_edited(p.into());
                ie.invoke_import_read_preview();
                step2.set(4);
            }

            // Preview, then mapping: the open table tab chose the target and
            // its columns were read for the auto-mapping.
            4 if ie.get_step() == 1 && !ie.get_busy() => {
                if ie.get_preview_rows().row_count() != 3 {
                    return bail(format!("the preview should show 3 rows, got {}", ie.get_preview_rows().row_count()));
                }
                if !ie.get_preview_summary().starts_with("CSV") {
                    return bail(format!("preview summary: {:?}", ie.get_preview_summary()));
                }
                ie.invoke_import_next();
                if ie.get_step() != 2 {
                    return bail("Continue should reach the mapping step".into());
                }
                step2.set(5);
            }
            5 if !ie.get_loading_columns() && ie.get_mapping().row_count() == 3 && ie.get_mapping_ready() => {
                let rows = ie.get_mapping();
                let targets: Vec<String> = rows.iter().map(|r| r.target.to_string()).collect();
                if targets != ["id", "name", "price"] || rows.iter().any(|r| r.skip) {
                    return bail(format!("columns should auto-map by name, got {targets:?}"));
                }
                if ie.get_table_value() != "widgets" {
                    return bail(format!("target table should be widgets, got {:?}", ie.get_table_value()));
                }
                // The user gives `price` a default and moves on with a small batch.
                ie.invoke_mapping_default(2, "0".into());
                ie.invoke_import_next();
                if ie.get_step() != 3 {
                    return bail("Continue should reach the options step".into());
                }
                ie.set_batch_size("2".into());
                ie.invoke_batch_size_edited("2".into());
                ie.invoke_import_review();
                step2.set(6);
            }

            // The plan is shown before anything is written.
            6 if ie.get_step() == 4 && !ie.get_busy() => {
                if count(&db2, "SELECT COUNT(*) FROM widgets") != 2 {
                    return bail("rows were written before the plan was confirmed".into());
                }
                if !ie.get_plan_rows().starts_with("3 in 2 statement") {
                    return bail(format!("plan rows: {:?}", ie.get_plan_rows()));
                }
                if ie.get_plan_samples().row_count() != 2 || !ie.get_plan_samples().row_data(0).unwrap().contains("INSERT INTO \"widgets\"") {
                    return bail("the plan should show the INSERT statements, unqualified on SQLite".into());
                }
                if ie.get_needs_typed() || !ie.get_can_import() {
                    return bail("an append needs no typed confirmation".into());
                }
                ie.invoke_import_run();
                step2.set(7);
            }
            7 if ie.get_step() == 6 => {
                if ie.get_result_summary() != "3 rows imported, 0 failed" || ie.get_result_tone() != Tone::Success {
                    return bail(format!("import result: {:?}", ie.get_result_summary()));
                }
                if count(&db2, "SELECT COUNT(*) FROM widgets") != 5 {
                    return bail("the import should have added 3 rows".into());
                }
                let gear: String = rusqlite::Connection::open(&db2)
                    .unwrap()
                    .query_row("SELECT name FROM widgets WHERE id = 5", [], |r| r.get(0))
                    .unwrap();
                if gear != "gear, \"large\"" {
                    return bail(format!("a quoted cell with a comma must survive, got {gear:?}"));
                }
                if !toast_texts(&w).iter().any(|t| t.contains("3 rows imported")) {
                    return bail(format!("the import should toast its result: {:?}", toast_texts(&w)));
                }
                // Export the table to CSV.
                ie.invoke_tab_selected(2);
                let p = out2.join("widgets.csv").to_string_lossy().to_string();
                ie.set_export_path(p.clone().into());
                ie.invoke_export_path_edited(p.into());
                ie.invoke_export_run();
                step2.set(8);
            }
            8 if ie.get_export_result() != "" || ie.get_export_error() != "" => {
                if ie.get_export_error() != "" {
                    return bail(format!("export failed: {}", ie.get_export_error()));
                }
                let text = std::fs::read_to_string(out2.join("widgets.csv")).unwrap_or_default();
                let lines: Vec<&str> = text.lines().collect();
                if lines.first() != Some(&"id,name,price") || lines.len() != 6 {
                    return bail(format!("exported csv should have a header and 5 rows: {text:?}"));
                }
                if !text.contains("\"gear, \"\"large\"\"\"") {
                    return bail(format!("the exported comma cell must be quoted: {text:?}"));
                }
                if !ie.get_export_result().contains("Exported 5 rows") || !ie.get_export_result().contains("widgets.csv") {
                    return bail(format!("the result should name the rows and the path: {:?}", ie.get_export_result()));
                }
                if !toast_texts(&w).iter().any(|t| t.contains(&out2.join("widgets.csv").to_string_lossy().to_string())) {
                    return bail(format!("the toast should show the written path: {:?}", toast_texts(&w)));
                }
                // A SQL dump with DROP, CREATE and INSERT.
                ie.invoke_tab_selected(3);
                ie.invoke_dump_drop_toggled();
                let p = out2.join("shop.sql").to_string_lossy().to_string();
                ie.set_dump_path(p.clone().into());
                ie.invoke_dump_path_edited(p.into());
                ie.invoke_dump_run();
                step2.set(9);
            }
            9 if ie.get_dump_result() != "" || ie.get_dump_error() != "" => {
                if ie.get_dump_error() != "" {
                    return bail(format!("dump failed: {}", ie.get_dump_error()));
                }
                let sql = std::fs::read_to_string(out2.join("shop.sql")).unwrap_or_default();
                if !sql.contains("DROP TABLE") || !sql.contains("CREATE TABLE") {
                    return bail(format!("the dump should carry DROP and CREATE: {sql}"));
                }
                if sql.matches("INSERT INTO \"widgets\"").count() != 5 {
                    return bail(format!("the dump should insert 5 rows into an unqualified table: {sql}"));
                }
                if sql.contains("\"public\"") || sql.contains(&db2.to_string_lossy().to_string()) {
                    return bail(format!("a SQLite dump must not qualify tables with a schema or path: {sql}"));
                }
                if !ie.get_dump_result().starts_with("Dumped 1 table to") {
                    return bail(format!("dump result: {:?}", ie.get_dump_result()));
                }
                // Run a SQL file.
                ie.invoke_tab_selected(1);
                let p = good2.to_string_lossy().to_string();
                ie.set_sql_path(p.clone().into());
                ie.invoke_sql_path_edited(p.into());
                ie.invoke_sql_run();
                step2.set(10);
            }
            10 if ie.get_sql_has_result() || ie.get_sql_error() != "" || ie.get_sql_path_error() != "" => {
                if ie.get_sql_error() != "" || ie.get_sql_path_error() != "" {
                    return bail(format!("sql file: {} {}", ie.get_sql_error(), ie.get_sql_path_error()));
                }
                if ie.get_sql_result_tone() != Tone::Success || !ie.get_sql_result_summary().starts_with("3 statements executed") {
                    return bail(format!("sql result: {:?}", ie.get_sql_result_summary()));
                }
                if count(&db2, "SELECT COUNT(*) FROM extras") != 2 {
                    return bail("the SQL file should have created extras with 2 rows".into());
                }
                // The sidebar follows: the new table appears without a refresh.
                step2.set(11);
            }
            11 if w.get_workspace_tables().row_count() == 2 => {
                // A failing file inside a transaction rolls back, and says so.
                ie.invoke_sql_again();
                let p = bad2.to_string_lossy().to_string();
                ie.set_sql_path(p.clone().into());
                ie.invoke_sql_path_edited(p.into());
                ie.invoke_sql_run();
                step2.set(12);
            }
            12 if ie.get_sql_has_result() || ie.get_sql_error() != "" => {
                if ie.get_sql_error() != "" {
                    // Reported as an error is acceptable too, as long as nothing leaked.
                } else if ie.get_sql_result_tone() == Tone::Success {
                    return bail(format!("a failing file must not report success: {:?}", ie.get_sql_result_summary()));
                }
                if count(&db2, "SELECT COUNT(*) FROM extras") != 2 {
                    return bail("the failed file's first INSERT should have been rolled back".into());
                }
                ie.invoke_close();
                if ie.get_visible() {
                    return bail("Close should hide the panel".into());
                }

                // -- Backup. The folder is typed, so it is a temp folder.
                let p = backups2.to_string_lossy().to_string();
                bk.set_output_dir(p.clone().into());
                bk.invoke_output_dir_edited(p.into());
                w.invoke_run_command("go-backup".into());
                if !bk.get_visible() {
                    return bail("the go-backup command did not open the panel".into());
                }
                step2.set(13);
            }
            13 if !bk.get_loading() && bk.get_directory() == backups2.to_string_lossy().to_string() => {
                if bk.get_rows().row_count() != 0 {
                    return bail("the temp backup folder should start empty".into());
                }
                if bk.get_backup_blocker() != "" {
                    return bail(format!("SQLite backups should be allowed: {:?}", bk.get_backup_blocker()));
                }
                bk.invoke_new_backup();
                if bk.get_view() != BackupView::Create || bk.get_tables_enabled() {
                    return bail("the create form should open, with table choice off for SQLite".into());
                }
                // Neither schema nor data is refused by the form.
                bk.invoke_schema_toggled();
                bk.invoke_data_toggled();
                bk.invoke_create_submit();
                if bk.get_form_error() == "" || bk.get_view() != BackupView::Create {
                    return bail("a backup with neither schema nor data must be refused".into());
                }
                bk.invoke_schema_toggled();
                bk.invoke_data_toggled();
                bk.set_note("before the damage".into());
                bk.invoke_note_edited("before the damage".into());
                bk.invoke_create_submit();
                if bk.get_view() != BackupView::Progress {
                    return bail("creating should switch to the progress view".into());
                }
                step2.set(14);
            }
            14 if bk.get_op_done() => {
                if bk.get_op_failed() || !bk.get_op_result().contains("Backup written to") {
                    return bail(format!("backup result: failed={} {:?}", bk.get_op_failed(), bk.get_op_result()));
                }
                let files: Vec<_> = std::fs::read_dir(&backups2).unwrap().flatten().collect();
                if files.len() != 1 {
                    return bail(format!("exactly one backup file expected, found {}", files.len()));
                }
                *backup_file2.borrow_mut() = files[0].path().to_string_lossy().to_string();
                if !toast_texts(&w).iter().any(|t| t.contains(&*backup_file2.borrow())) {
                    return bail(format!("the toast should show the backup path: {:?}", toast_texts(&w)));
                }
                bk.invoke_progress_dismiss();
                step2.set(15);
            }
            15 if bk.get_view() == BackupView::List && !bk.get_loading() && bk.get_rows().row_count() == 1 => {
                let row = bk.get_rows().row_data(0).unwrap();
                if !row.ok || !row.subtitle.contains("Shop") || !row.subtitle.contains("Sqlite") {
                    return bail(format!("the list row should name the connection: {:?}", row.subtitle));
                }
                // Damage the database behind the app's back, then restore.
                rusqlite::Connection::open(&db2).unwrap().execute("DELETE FROM widgets", []).unwrap();
                if count(&db2, "SELECT COUNT(*) FROM widgets") != 0 {
                    return bail("the damage did not apply".into());
                }
                bk.invoke_restore_asked(row.id.clone());
                if bk.get_view() != BackupView::Restore {
                    return bail("Restore should open the confirmation".into());
                }
                if bk.get_drop_applies() {
                    return bail("drop and recreate must not be offered for SQLite".into());
                }
                if bk.get_restore_warnings().row_count() == 0 || bk.get_restore_blocker() != "" {
                    return bail("the restore view should warn and not be blocked".into());
                }
                // Cancelling restores nothing.
                bk.invoke_restore_cancel();
                if count(&db2, "SELECT COUNT(*) FROM widgets") != 0 || bk.get_view() != BackupView::List {
                    return bail("cancelling a restore must change nothing".into());
                }
                bk.invoke_restore_asked(row.id.clone());
                bk.invoke_restore_submit();
                step2.set(16);
            }
            16 if bk.get_op_done() => {
                if bk.get_op_failed() {
                    return bail(format!("restore failed: {:?}", bk.get_op_result()));
                }
                if count(&db2, "SELECT COUNT(*) FROM widgets") != 5 {
                    return bail("the restore should have brought back 5 rows".into());
                }
                // The app's own connection sees the restored file.
                bk.invoke_progress_dismiss();
                bk.invoke_close();
                w.invoke_workspace_new_query();
                step2.set(17);
            }
            17 if w.get_workspace_active_is_query() => {
                w.invoke_workspace_editor_edited("SELECT COUNT(*) AS n FROM widgets".into());
                w.invoke_workspace_run_query();
                step2.set(18);
            }
            18 if w.get_workspace_has_result() && !w.get_workspace_running() => {
                let cell = w.get_workspace_rows().row_data(0).unwrap().cells.row_data(0).unwrap();
                if cell.text != "5" {
                    return bail(format!("the app should read the restored rows, got {:?}", cell.text));
                }
                // Delete the backup, behind a confirmation.
                bk.invoke_open_requested();
                step2.set(19);
            }
            19 if bk.get_rows().row_count() == 1 && !bk.get_loading() => {
                let id = bk.get_rows().row_data(0).unwrap().id;
                bk.invoke_delete_asked(id);
                if !w.get_show_confirm() || w.get_confirm_title() != "Delete this backup?" {
                    return bail(format!("delete should ask first, got {:?}", w.get_confirm_title()));
                }
                w.invoke_confirm_cancelled();
                w.set_show_confirm(false);
                if !Path::new(&*backup_file2.borrow()).exists() {
                    return bail("cancelling the confirmation deleted the file".into());
                }
                let id = bk.get_rows().row_data(0).unwrap().id;
                bk.invoke_delete_asked(id);
                w.invoke_confirm_accepted();
                w.set_show_confirm(false);
                step2.set(20);
            }
            20 if bk.get_rows().row_count() == 0 && !bk.get_loading() => {
                if Path::new(&*backup_file2.borrow()).exists() {
                    return bail("the backup file should be gone".into());
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
