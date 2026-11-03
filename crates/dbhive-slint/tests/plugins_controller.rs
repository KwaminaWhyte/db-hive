//! The plugin manager driven end to end: the real window, the real controller,
//! the real plugin engine and a real plugin on disk.
//!
//! The flow: open the panel from the `go-plugins` palette command, see the
//! marketplace, try to install an entry that is not bundled (a failed card with
//! its reason), install one that is (a plugin written to a temp directory and
//! offered through `DBHIVE_PLUGIN_SOURCES`), see its detail, disable and enable
//! it (the toolbar button comes and goes), configure it through the generated
//! settings form, open its interface, edit a field, press its button and read
//! the toast it raised, press its toolbar button, watch a permission it did not
//! declare be refused, then uninstall it behind a confirmation.
//!
//! Every store and every plugin directory is under a temp directory
//! (`DBHIVE_DATA_DIR` is set before anything is built), so this never touches
//! the developer's own plugins or connections.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use dbhive_slint::{PluginsState, Screen, Tone};
use slint::{ComponentHandle, Model, Timer, TimerMode};

const PLUGIN_ID: &str = "com.dbhive.csv-exporter";

const MANIFEST: &str = r#"{
  "id": "com.dbhive.csv-exporter", "name": "Temp Exporter", "version": "1.0.0",
  "description": "A plugin written by the test.",
  "author": { "name": "Test Author" },
  "category": "export", "main": "index.js", "pluginType": "javascript",
  "permissions": ["showNotification", "modifyUI"],
  "minVersion": "0.1.0", "license": "MIT", "keywords": ["csv", "temp"],
  "configSchema": { "type": "object", "properties": {
    "delimiter": { "type": "string", "enum": [",", ";"], "default": "," },
    "limit": { "type": "integer", "default": 10, "minimum": 1, "maximum": 100 }
  } }
}"#;

// Never reaches a database or a file: it can only describe an interface and
// raise a toast. The `denied` button tries to write a file anyway.
const CODE: &str = r#"
__plugin_exports__ = {
  onLoad: function () { return { success: true }; },
  renderUi: function () {
    return {
      title: 'Temp exporter',
      toolbar: [ DBHive.ui.toolbarItem('quick', 'Quick export', { icon: 'export', tooltip: 'Run the quick export' }) ],
      panels: [ DBHive.ui.panel('Options', [
        DBHive.ui.input('name', 'File name', 'out.csv'),
        DBHive.ui.checkbox('flag', 'Include header', true),
        DBHive.ui.button('run', 'Run export', { style: 'primary' }),
        DBHive.ui.button('denied', 'Try to write a file')
      ]) ]
    };
  },
  onUiEvent: function (event) {
    var cfg = DBHive.getConfig() || {};
    if (event.type === 'buttonPressed' && event.id === 'run') {
      DBHive.showNotification({ title: 'Exporter', notificationType: 'success',
        message: 'ran ' + event.fields.name + ' header=' + event.fields.flag + ' delimiter=' + cfg.delimiter });
    } else if (event.type === 'buttonPressed' && event.id === 'quick') {
      DBHive.showNotification({ title: 'Exporter', message: 'quick export', notificationType: 'warning' });
    } else if (event.type === 'buttonPressed' && event.id === 'denied') {
      try { DBHive.writeFile('x.txt', 'y'); DBHive.showNotification({ title: 'Exporter', message: 'WROTE A FILE' }); }
      catch (e) { DBHive.showNotification({ title: 'Exporter', message: 'refused: ' + e.message, notificationType: 'error' }); }
    }
    return null;
  }
};
"#;

fn toasts(w: &dbhive_slint::AppWindow) -> Vec<(String, Tone)> {
    w.get_toasts()
        .iter()
        .map(|t| (t.text.to_string(), t.tone))
        .collect()
}

fn toast_with(w: &dbhive_slint::AppWindow, needle: &str) -> Option<(String, Tone)> {
    toasts(w).into_iter().find(|(t, _)| t.contains(needle))
}

fn market_state(ps: &PluginsState, id: &str) -> Option<(i32, String)> {
    ps.get_market()
        .iter()
        .find(|m| m.id == id)
        .map(|m| (m.state, m.note.to_string()))
}

#[test]
fn a_plugin_installs_runs_with_a_toast_is_gated_and_uninstalls_through_the_real_window() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-slint-plugins-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    // The plugin the "marketplace" can install: its folder name is the last
    // segment of its id, which is how the engine finds a bundled source.
    let source = dir.join("sources").join("csv-exporter");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("manifest.json"), MANIFEST).unwrap();
    std::fs::write(source.join("index.js"), CODE).unwrap();
    std::env::set_var("DBHIVE_PLUGIN_SOURCES", dir.join("sources"));

    let installed_dir = dir.join("plugins").join(PLUGIN_ID);

    let w = dbhive_slint::app::build().unwrap();
    let failure: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let step = Rc::new(Cell::new(0u32));
    let started = Instant::now();
    let timer = Timer::default();

    let (weak, fail, step2) = (w.as_weak(), failure.clone(), step.clone());
    let installed_dir2 = installed_dir.clone();
    timer.start(TimerMode::Repeated, Duration::from_millis(20), move || {
        let w = weak.upgrade().unwrap();
        let ps = w.global::<PluginsState>();
        let bail = |msg: String| {
            *fail.borrow_mut() = Some(msg);
            slint::quit_event_loop().unwrap();
        };
        if started.elapsed() > Duration::from_secs(90) {
            bail(format!("timed out at step {}", step2.get()));
            return;
        }
        match step2.get() {
            // Open from the palette command, on the connections screen: the
            // panel does not need a workspace.
            0 => {
                if w.get_screen() != Screen::Connections {
                    return bail("the test starts on the connections screen".into());
                }
                w.invoke_run_command("go-plugins".into());
                if !ps.get_visible() {
                    return bail("go-plugins did not open the panel".into());
                }
                step2.set(1);
            }
            // Nothing installed: the empty state, and the marketplace listing.
            1 if ps.get_load() == 0 => {
                if ps.get_installed().row_count() != 0 || ps.get_has_detail() {
                    return bail("a fresh data directory has no plugins".into());
                }
                if ps.get_market().row_count() != 3 {
                    return bail(format!(
                        "the catalogue has 3 entries, got {}",
                        ps.get_market().row_count()
                    ));
                }
                ps.invoke_tab_selected(1);
                if ps.get_tab() != 0 {
                    // `tab` is an `in` property: only the controller sets it.
                }
                // Install the entry nothing bundles: the card must fail, with a
                // reason, and stay installable.
                ps.invoke_install("com.dbhive.sql-formatter".into());
                match market_state(&ps, "com.dbhive.sql-formatter") {
                    Some((1, _)) => {}
                    other => {
                        return bail(format!(
                            "the card should say Installing at once, got {:?}",
                            other
                        ))
                    }
                }
                step2.set(2);
            }
            2 if market_state(&ps, "com.dbhive.sql-formatter").is_some_and(|(s, _)| s != 1) => {
                let (state, note) = market_state(&ps, "com.dbhive.sql-formatter").unwrap();
                if state != 3 || !note.contains("not bundled") {
                    return bail(format!(
                        "an unbundled entry should fail with a reason, got {} {:?}",
                        state, note
                    ));
                }
                if toast_with(&w, "not bundled").is_none_or(|(_, tone)| tone != Tone::Danger) {
                    return bail("the failure should also raise a danger toast".into());
                }
                if ps.get_installed().row_count() != 0 {
                    return bail("a failed install must not leave a plugin behind".into());
                }
                // Now the one that is bundled.
                ps.invoke_install(PLUGIN_ID.into());
                if market_state(&ps, PLUGIN_ID).is_none_or(|(s, _)| s != 1) {
                    return bail("the real install should show Installing".into());
                }
                step2.set(3);
            }
            3 if market_state(&ps, PLUGIN_ID).is_some_and(|(s, _)| s == 2)
                && ps.get_toolbar().row_count() == 1 =>
            {
                if !installed_dir2.join("manifest.json").is_file()
                    || !installed_dir2.join("index.js").is_file()
                {
                    return bail("the plugin should be copied into the plugin directory".into());
                }
                if toast_with(&w, "Temp Exporter installed").is_none() {
                    return bail(format!(
                        "an install toast was expected, got {:?}",
                        toasts(&w)
                    ));
                }
                if ps.get_installed().row_count() != 1 {
                    return bail("the installed list should hold the plugin".into());
                }
                let t = ps.get_toolbar().row_data(0).unwrap();
                if t.label != "Quick export"
                    || t.plugin_id != PLUGIN_ID
                    || !t.tooltip.contains("Temp Exporter")
                {
                    return bail(format!(
                        "the plugin's toolbar button is wrong: {:?} {:?}",
                        t.label, t.tooltip
                    ));
                }
                ps.invoke_tab_selected(0);
                step2.set(4);
            }
            // Detail: everything the checklist lists.
            4 if ps.get_has_detail() => {
                let facts = |m: slint::ModelRc<dbhive_slint::PluginFact>| -> Vec<(String, String)> {
                    m.iter()
                        .map(|f| (f.label.to_string(), f.value.to_string()))
                        .collect()
                };
                let info = facts(ps.get_d_info());
                let stats = facts(ps.get_d_stats());
                let has = |v: &[(String, String)], k: &str, want: &str| {
                    v.iter().any(|(a, b)| a == k && b == want)
                };
                if ps.get_d_name() != "Temp Exporter"
                    || !has(&info, "Author", "Test Author")
                    || !has(&info, "License", "MIT")
                    || !has(&info, "Category", "Export Formats")
                    || !has(&info, "Type", "JavaScript")
                    || !stats.iter().any(|(k, _)| k == "Installed")
                    || !stats.iter().any(|(k, _)| k == "Executions")
                    || !stats.iter().any(|(k, _)| k == "Errors")
                {
                    return bail(format!("detail facts are wrong: {:?} {:?}", info, stats));
                }
                if ps.get_d_permissions().row_count() != 2 || ps.get_d_keywords().row_count() != 2 {
                    return bail("permissions and keywords should be listed".into());
                }
                if !ps.get_d_enabled() || !ps.get_d_can_open() || !ps.get_d_has_settings() {
                    return bail("a fresh plugin is enabled, openable and has settings".into());
                }
                // Disable: the toolbar button must go, the state file must say so.
                ps.invoke_toggle_enabled("".into());
                step2.set(5);
            }
            5 if !ps.get_d_enabled() && ps.get_d_busy() == "" => {
                if ps.get_toolbar().row_count() != 0 {
                    return bail("a disabled plugin must leave no toolbar button".into());
                }
                if ps.get_d_can_open() {
                    return bail("a disabled plugin must not offer its interface".into());
                }
                let state =
                    std::fs::read_to_string(installed_dir2.join(".state.json")).unwrap_or_default();
                if !state.contains("false") {
                    return bail(format!(
                        "the disabled state should be saved, got {:?}",
                        state
                    ));
                }
                if toast_with(&w, "Temp Exporter disabled").is_none() {
                    return bail("a disabled toast was expected".into());
                }
                ps.invoke_toggle_enabled("".into());
                step2.set(6);
            }
            6 if ps.get_d_enabled()
                && ps.get_d_busy() == ""
                && ps.get_toolbar().row_count() == 1 =>
            {
                // Settings, from the manifest's schema.
                ps.invoke_open_settings();
                step2.set(7);
            }
            7 if ps.get_form_open() => {
                let kinds: Vec<(i32, String)> = ps
                    .get_form_items()
                    .iter()
                    .map(|i| (i.kind, i.id.to_string()))
                    .collect();
                if !kinds.contains(&(7, "delimiter".into()))
                    || !kinds.contains(&(5, "limit".into()))
                {
                    return bail(format!(
                        "the settings form should hold a select and a number, got {:?}",
                        kinds
                    ));
                }
                // Pick ';', type a bad number (Save must refuse), then a good one.
                ps.invoke_item_select(1, "delimiter".into(), 1);
                ps.invoke_item_number(1, "limit".into(), "lots".into());
                ps.invoke_settings_save();
                if !ps.get_form_open() || ps.get_form_error().is_empty() {
                    return bail("a bad number must keep the form open with a reason".into());
                }
                ps.invoke_item_number(1, "limit".into(), "5".into());
                ps.invoke_settings_save();
                step2.set(8);
            }
            8 if !ps.get_form_open() => {
                if toast_with(&w, "settings saved").is_none() {
                    return bail("a settings-saved toast was expected".into());
                }
                let cfg =
                    std::fs::read_to_string(installed_dir2.join("config.json")).unwrap_or_default();
                if !cfg.contains("\";\"") || !cfg.contains('5') {
                    return bail(format!(
                        "the saved config should hold the choices, got {:?}",
                        cfg
                    ));
                }
                // Open the interface the plugin describes.
                ps.invoke_open_interface();
                step2.set(9);
            }
            9 if ps.get_d_interface_open()
                && ps.get_view_items().row_count() > 0
                && ps.get_d_busy() == "" =>
            {
                if ps.get_view_title() != "Temp exporter" || !ps.get_view_error().is_empty() {
                    return bail(format!(
                        "interface title/error wrong: {:?} {:?}",
                        ps.get_view_title(),
                        ps.get_view_error()
                    ));
                }
                let kinds: Vec<i32> = ps.get_view_items().iter().map(|i| i.kind).collect();
                // header, input, checkbox, button, button
                if kinds != vec![0, 4, 6, 3, 3] {
                    return bail(format!("unexpected interface rows {:?}", kinds));
                }
                // Edit a field and a checkbox, then press the button.
                ps.invoke_item_input(0, "name".into(), "typed.csv".into());
                ps.invoke_item_check(0, "flag".into());
                ps.invoke_item_button(0, "run".into());
                step2.set(10);
            }
            10 if toast_with(&w, "ran typed.csv").is_some() => {
                let (text, tone) = toast_with(&w, "ran typed.csv").unwrap();
                if text != "Exporter: ran typed.csv header=false delimiter=;"
                    || tone != Tone::Success
                {
                    return bail(format!(
                        "the toast should carry the edits and the saved config, got {:?} {:?}",
                        text, tone
                    ));
                }
                // The toolbar button, from the header, with no panel in the way.
                ps.invoke_toolbar_pressed(PLUGIN_ID.into(), "quick".into());
                step2.set(11);
            }
            11 if toast_with(&w, "quick export").is_some() => {
                if toast_with(&w, "quick export").unwrap().1 != Tone::Warning {
                    return bail("the plugin asked for a warning toast".into());
                }
                // A permission the manifest never declared.
                ps.invoke_item_button(0, "denied".into());
                step2.set(12);
            }
            12 if toast_with(&w, "refused").is_some() => {
                let (text, tone) = toast_with(&w, "refused").unwrap();
                if toast_with(&w, "WROTE A FILE").is_some() {
                    return bail("a plugin without writeFiles wrote a file".into());
                }
                if !text.contains("Permission denied") || tone != Tone::Danger {
                    return bail(format!(
                        "the refusal should read as a permission error, got {:?} {:?}",
                        text, tone
                    ));
                }
                if installed_dir2.join("x.txt").exists()
                    || dir_has_file(
                        &installed_dir2
                            .parent()
                            .unwrap()
                            .parent()
                            .unwrap()
                            .join("plugin-data"),
                        "x.txt",
                    )
                {
                    return bail("the refused write left a file behind".into());
                }
                // Running left its mark on the plugin's statistics.
                let runs = ps
                    .get_d_stats()
                    .iter()
                    .find(|f| f.label == "Executions")
                    .map(|f| f.value.to_string());
                if runs.as_deref().is_none_or(|r| r == "0") {
                    return bail(format!(
                        "executions should have been counted, got {:?}",
                        runs
                    ));
                }
                // Uninstall asks first; cancelling keeps the plugin.
                ps.invoke_uninstall();
                if !w.get_show_confirm() || w.get_confirm_title() != "Uninstall plugin?" {
                    return bail(format!(
                        "uninstall should ask first, got {:?}",
                        w.get_confirm_title()
                    ));
                }
                w.invoke_confirm_cancelled();
                w.set_show_confirm(false);
                if !installed_dir2.exists() || ps.get_installed().row_count() != 1 {
                    return bail("cancelling the confirmation removed the plugin".into());
                }
                ps.invoke_uninstall();
                w.invoke_confirm_accepted();
                w.set_show_confirm(false);
                step2.set(13);
            }
            13 if ps.get_installed().row_count() == 0 => {
                if installed_dir2.exists() {
                    return bail("the plugin directory should be gone".into());
                }
                if ps.get_toolbar().row_count() != 0
                    || ps.get_has_detail()
                    || ps.get_d_interface_open()
                {
                    return bail("nothing of the plugin may stay on screen".into());
                }
                if toast_with(&w, "Temp Exporter uninstalled").is_none() {
                    return bail("an uninstalled toast was expected".into());
                }
                ps.invoke_tab_selected(1);
                step2.set(14);
            }
            // The card goes back to installable.
            14 if market_state(&ps, PLUGIN_ID).is_some_and(|(s, _)| s == 0) => {
                ps.invoke_close();
                if ps.get_visible() {
                    return bail("close should hide the panel".into());
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

fn dir_has_file(root: &Path, name: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else {
        return false;
    };
    entries.flatten().any(|e| {
        let p = e.path();
        p.file_name().is_some_and(|n| n == name) || (p.is_dir() && dir_has_file(&p, name))
    })
}
