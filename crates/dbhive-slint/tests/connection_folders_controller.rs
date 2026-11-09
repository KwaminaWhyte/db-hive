//! Connection folders, the editor's new fields and the Browse buttons, through
//! the real window and controller against real SQLite files.
//!
//! * The list groups saved connections under folder headers, with "Unfiled"
//!   last, and the header line counts connections and folders.
//! * A header click collapses the folder and the choice is written to disk.
//! * The editor's SSH local port is validated, Browse fills the SQLite path
//!   (through a test picker; no real panel opens), the Folder field saves
//!   with the profile, and the new connection lands in its folder.
//! * Browse in the import and backup panels fills their fields too.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dbhive_core::models::{ConnectionProfile, DbDriver};
use dbhive_core::state::AppState;
use dbhive_slint::file_dialog::FixedPicker;
use dbhive_slint::{
    BackupState, BrowseTarget, ConnectionEditorState, ConnectionsState, FileBrowse,
    ImportExportState,
};
use dbhive_ui_model::file_pick::PickKind;
use slint::{ComponentHandle, Model, Timer, TimerMode};

fn rows(w: &dbhive_slint::AppWindow) -> Vec<String> {
    w.get_connections()
        .iter()
        .map(|c| {
            if c.is_folder {
                format!(
                    "[{} {}{}]",
                    c.name,
                    c.count,
                    if c.collapsed { " closed" } else { "" }
                )
            } else {
                c.name.to_string()
            }
        })
        .collect()
}

fn sqlite_profile(
    id: &str,
    name: &str,
    path: &std::path::Path,
    folder: Option<&str>,
) -> ConnectionProfile {
    let mut p = ConnectionProfile::new(
        id.to_string(),
        name.to_string(),
        DbDriver::Sqlite,
        String::new(),
        0,
        String::new(),
    );
    p.database = Some(path.to_string_lossy().to_string());
    p.folder = folder.map(str::to_string);
    p
}

#[test]
fn group_collapse_persist_edit_and_browse() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-folders-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let db = dir.join("a.sqlite");
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute("CREATE TABLE t (id INTEGER)", [])
        .unwrap();
    let picked: PathBuf = dir.join("picked.sqlite");
    rusqlite::Connection::open(&picked)
        .unwrap()
        .execute("CREATE TABLE p (id INTEGER)", [])
        .unwrap();

    {
        let mut state = AppState::new();
        state.add_profile(sqlite_profile("a", "Alpha", &db, Some("Work")));
        state.add_profile(sqlite_profile("b", "Beta", &db, Some("work ")));
        state.add_profile(sqlite_profile("c", "Gamma", &db, None));
        state.save_profiles_to_store().unwrap();
    }

    let picker = Arc::new(FixedPicker::new(Some(picked.clone())));
    dbhive_slint::file_dialog::use_picker(picker.clone());

    let w = dbhive_slint::app::build().unwrap();
    let failure: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let step = Rc::new(Cell::new(0u32));
    let started = Instant::now();
    let timer = Timer::default();
    let picked_str = picked.to_string_lossy().to_string();

    let (weak, fail, step2, dir2, picker2) = (
        w.as_weak(),
        failure.clone(),
        step.clone(),
        dir.clone(),
        picker.clone(),
    );
    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        let w = weak.upgrade().unwrap();
        let bail = |msg: String| {
            *fail.borrow_mut() = Some(msg);
            slint::quit_event_loop().unwrap();
        };
        if started.elapsed() > Duration::from_secs(20) {
            bail(format!("timed out at step {}", step2.get()));
            return;
        }
        let browse = w.global::<FileBrowse>();
        match step2.get() {
            0 => {
                let got = rows(&w);
                let want = ["[Work 2]", "Alpha", "Beta", "[Unfiled 1]", "Gamma"];
                if got != want {
                    return bail(format!("rows were {got:?}"));
                }
                let summary = w.global::<ConnectionsState>().get_summary();
                if summary != "3 connections in 1 folder" {
                    return bail(format!("summary was {summary:?}"));
                }
                w.global::<ConnectionsState>()
                    .invoke_toggle_folder("work".into());
                step2.set(1);
            }
            1 => {
                let got = rows(&w);
                if got != ["[Work 2 closed]", "[Unfiled 1]", "Gamma"] {
                    return bail(format!("collapsed rows were {got:?}"));
                }
                // A search opens collapsed folders so matches are visible.
                w.invoke_connection_search_changed("beta".into());
                let got = rows(&w);
                if got != ["[Work 1]", "Beta"] {
                    return bail(format!("search rows were {got:?}"));
                }
                w.invoke_connection_search_changed("".into());
                step2.set(2);
            }
            // The collapse is written off the UI thread.
            2 if std::fs::read_to_string(dir2.join("connection_folders.json"))
                .is_ok_and(|s| s.contains("\"work\"")) =>
            {
                // New connection: validate the SSH local port first.
                w.invoke_new_connection();
                w.invoke_editor_toggle_ssh();
                let e = w.global::<ConnectionEditorState>();
                e.invoke_ssh_local_port_changed("abc".into());
                let errors: Vec<String> = w
                    .get_editor_errors()
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                if !errors.iter().any(|m| m.contains("Local port")) {
                    return bail(format!("no local port error in {errors:?}"));
                }
                e.invoke_ssh_local_port_changed("15432".into());
                if w.get_editor().ssh_local_port != "15432" {
                    return bail("local port not projected".into());
                }
                // Now a SQLite file, picked with Browse.
                w.invoke_editor_driver_selected(2);
                w.invoke_editor_name_changed("Picked".into());
                e.invoke_folder_changed("Work".into());
                let data = w.get_editor();
                if data.folder != "Work" || !data.folder_options.iter().any(|f| f == "Work") {
                    return bail("folder field or chips missing".into());
                }
                browse.invoke_browse(BrowseTarget::SqliteFile);
                step2.set(3);
            }
            3 if !browse.get_busy() && w.get_editor().database == picked_str.as_str() => {
                let asked = picker2.requests();
                if asked.len() != 1 || asked[0].kind != PickKind::Open {
                    return bail(format!("picker was asked {asked:?}"));
                }
                if w.get_editor_errors().row_count() > 0 {
                    return bail(format!(
                        "unexpected errors {:?}",
                        w.get_editor_errors().iter().collect::<Vec<_>>()
                    ));
                }
                w.invoke_editor_save();
                step2.set(4);
            }
            // Saved into the collapsed Work folder.
            4 if !w.get_show_connection_editor() => {
                let got = rows(&w);
                if got != ["[Work 3 closed]", "[Unfiled 1]", "Gamma"] {
                    return bail(format!("rows after save were {got:?}"));
                }
                // Browse in the import and backup panels.
                browse.invoke_browse(BrowseTarget::ImportFile);
                step2.set(5);
            }
            5 if !browse.get_busy()
                && w.global::<ImportExportState>().get_import_path() == picked_str.as_str() =>
            {
                browse.invoke_browse(BrowseTarget::BackupDir);
                step2.set(6);
            }
            6 if !browse.get_busy()
                && w.global::<BackupState>().get_output_dir() == picked_str.as_str() =>
            {
                let kinds: Vec<PickKind> = picker2.requests().iter().map(|r| r.kind).collect();
                if kinds != [PickKind::Open, PickKind::Open, PickKind::Folder] {
                    return bail(format!("picker kinds were {kinds:?}"));
                }
                step2.set(7);
                slint::quit_event_loop().unwrap();
            }
            _ => {}
        }
    });

    slint::run_event_loop().unwrap();
    drop(timer);

    // The new profile kept its folder on disk.
    let saved = std::fs::read_to_string(dir.join("profiles.json")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    if let Some(msg) = failure.borrow().clone() {
        panic!("{msg}");
    }
    assert_eq!(step.get(), 7);
    assert!(saved.contains("Picked"), "profile not saved: {saved}");
}
