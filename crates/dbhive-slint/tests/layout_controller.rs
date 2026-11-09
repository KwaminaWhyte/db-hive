//! The two resizable panes: drags are clamped, saved on release, and the
//! next window opens with the saved sizes.
//!
//! The splitter itself is pointer-driven and covered by the workspace
//! snapshot; this drives the controller callbacks the splitter emits.

use dbhive_ui_model::layout::{EDITOR_MAX, EDITOR_MIN, SIDEBAR_MAX, SIDEBAR_MIN};

#[test]
fn pane_drags_clamp_save_and_restore() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-layout-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let first = dbhive_slint::app::build().unwrap();
    assert_eq!(first.get_sidebar_width(), 264.0);
    assert_eq!(first.get_editor_height(), 220.0);

    // Drag past both limits.
    first.invoke_sidebar_resize(9_000.0);
    first.invoke_editor_resize(1.0);
    assert_eq!(first.get_sidebar_width(), SIDEBAR_MAX);
    assert_eq!(first.get_editor_height(), EDITOR_MIN);

    // A usable value in the middle sticks live.
    first.invoke_sidebar_resize(320.0);
    first.invoke_editor_resize(300.0);
    assert_eq!(first.get_sidebar_width(), 320.0);
    assert_eq!(first.get_editor_height(), 300.0);

    // Releasing the pointer writes the file.
    first.invoke_layout_committed();
    let saved = std::fs::read_to_string(dir.join("layout.json")).expect("layout.json");
    assert!(saved.contains("320"), "{saved}");
    assert!(saved.contains("300"), "{saved}");

    // A second window opens with the saved sizes.
    let second = dbhive_slint::app::build().unwrap();
    assert_eq!(second.get_sidebar_width(), 320.0);
    assert_eq!(second.get_editor_height(), 300.0);

    // Below the minimum the other way, and above the sidebar's.
    second.invoke_sidebar_resize(1.0);
    second.invoke_editor_resize(9_000.0);
    assert_eq!(second.get_sidebar_width(), SIDEBAR_MIN);
    assert_eq!(second.get_editor_height(), EDITOR_MAX);
}
