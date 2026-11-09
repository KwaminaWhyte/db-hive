//! Two windows over one engine: each gets its own screen state and globals,
//! while both talk to the same `AppState`.
//!
//! The second window is opened through the real registry (`windows::open`),
//! with the testing backend. No connection is opened; the point is that the
//! windows are independent, not that they share a driver.

use std::sync::{Arc, Mutex};

use dbhive_core::state::AppState;
use dbhive_slint::app::windows;
use dbhive_slint::{Screen, Theme};
use slint::ComponentHandle;

#[test]
fn a_second_window_has_its_own_state_and_the_registry_keeps_both() {
    i_slint_backend_testing::init_integration_test_with_system_time();

    let dir = std::env::temp_dir().join(format!("dbhive-multiwindow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var(dbhive_core::storage::DATA_DIR_ENV, &dir);

    let main = dbhive_slint::app::build().unwrap();
    assert!(
        windows::window_labels().is_empty(),
        "build() does not attach"
    );

    let state = Arc::new(Mutex::new(AppState::new()));
    let second = windows::open(state, None).expect("second window");
    assert_eq!(windows::window_count(), 1);
    assert_eq!(windows::window_labels(), vec!["window-2".to_string()]);

    // A property on the first window does not move the second.
    main.set_connection_search("only in main".into());
    assert_eq!(second.get_connection_search().to_string(), "");

    // A global is per window too: setting the first to light leaves the
    // second on the default dark.
    main.global::<Theme>().set_dark(false);
    assert!(second.global::<Theme>().get_dark());

    // Both start on the connections screen.
    assert_eq!(second.get_screen(), Screen::Connections);

    // The label sequence does not repeat a live one.
    let third = windows::open(Arc::new(Mutex::new(AppState::new())), None).expect("third window");
    assert_eq!(third.get_screen(), Screen::Connections);
    assert_eq!(
        windows::window_labels(),
        vec!["window-2".to_string(), "window-3".to_string()]
    );

    // Closing one removes exactly that window; the others stay.
    let ids = windows::window_uids();
    windows::close_window(ids[1]);
    assert_eq!(windows::window_labels(), vec!["window-2".to_string()]);
    assert_eq!(windows::window_count(), 1);
    assert_eq!(windows::window_uids(), vec![ids[0]]);
}
