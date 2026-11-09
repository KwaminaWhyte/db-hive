//! The window registry: opening, closing and remembering the geometry of
//! every window.
//!
//! Slint 1.18 gives each component instance its own copy of the globals, so
//! two windows have separate screen state, tabs and toasts. What they share
//! is the core: the same `AppState`, the same Tokio runtime and the same
//! connection pool. A second window is therefore a second controller over
//! the same engine, not a second process.
//!
//! Geometry is saved in `window-state.json`, in logical pixels, keyed by
//! label: `main` for the first window, `window-2`, `window-3` and so on for
//! the rest (see `dbhive_ui_model::window_state`). The file is written only
//! when a polled geometry actually changed, and once more when a window
//! closes.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dbhive_core::state::AppState;
use dbhive_core::storage::JsonStore;
use dbhive_ui_model::window_state::{WindowGeometry, WindowStates, MAIN_LABEL};
use slint::{CloseRequestResponse, ComponentHandle, Timer, TimerMode};

use super::{build_window, install_panic_hook, unregister_ctx, Ctx, WindowOptions};
use crate::{AppWindow, ShellState};

const STORE_FILE: &str = "window-state.json";
const STORE_KEY: &str = "windows";
const POLL: Duration = Duration::from_secs(1);

struct OpenWindow {
    ui: u64,
    label: String,
    window: Rc<AppWindow>,
    /// Kept alive for the life of the window; it is not Clone.
    _poll: Timer,
}

thread_local! {
    static WINDOWS: RefCell<Vec<OpenWindow>> = const { RefCell::new(Vec::new()) };
    static SAVED: RefCell<Saved> = const { RefCell::new(Saved::new_const()) };
}

/// The geometry store, opened on first use.
struct Saved {
    store: Option<JsonStore>,
    states: Option<WindowStates>,
}

impl Saved {
    const fn new_const() -> Self {
        Self {
            store: None,
            states: None,
        }
    }

    fn open(&mut self) -> &mut WindowStates {
        if self.states.is_none() {
            match JsonStore::open(STORE_FILE) {
                Ok(store) => {
                    let states = WindowStates::from_json(store.get(STORE_KEY));
                    self.store = Some(store);
                    self.states = Some(states);
                }
                Err(e) => {
                    eprintln!("dbhive: could not open {}: {}", STORE_FILE, e);
                    self.states = Some(WindowStates::default());
                }
            }
        }
        self.states.as_mut().expect("states set above")
    }

    fn persist(&mut self) {
        let (Some(store), Some(states)) = (self.store.as_mut(), self.states.as_ref()) else {
            return;
        };
        store.set(STORE_KEY, states.to_json());
        if let Err(e) = store.save() {
            eprintln!("dbhive: could not save {}: {}", STORE_FILE, e);
        }
    }
}

fn with_saved<T>(f: impl FnOnce(&mut Saved) -> T) -> T {
    SAVED.with(|s| f(&mut s.borrow_mut()))
}

/// The app's entry point: the first window, then the event loop, then one
/// last save for every window still open.
pub fn run() -> Result<(), slint::PlatformError> {
    install_panic_hook();

    let state = Arc::new(Mutex::new(AppState::new()));
    let load_error = dbhive_core::service::load_persisted(&state)
        .err()
        .map(|e| format!("Could not load saved data: {}", e));

    let (window, ctx) = build_window(state, WindowOptions::main(), load_error)?;
    let window = Rc::new(window);
    attach(&window, &ctx, MAIN_LABEL);

    let result = slint::run_event_loop();
    for (label, window) in open_windows() {
        record(&label, &window);
    }
    with_saved(|saved| saved.persist());
    result
}

/// Wire this window's own window-level menu actions. Every window has its
/// own copies, so each closes itself and opens a window on the shared state.
pub(super) fn wire(window: &AppWindow, ctx: &Rc<Ctx>) {
    let shell = window.global::<ShellState>();

    let state = ctx.state.clone();
    shell.on_new_window_requested(move || {
        open(state.clone(), None);
    });

    let ui = ctx.ui_id;
    shell.on_close_window_requested(move || close_window(ui));
}

/// Open another window. With a connection id it connects as soon as it is
/// up; with none it lands on the connections screen.
pub fn open(state: Arc<Mutex<AppState>>, connection: Option<String>) -> Option<Rc<AppWindow>> {
    let label = next_label();
    let title = match label_number(&label) {
        Some(n) => format!("DB Hive ({n})"),
        None => "DB Hive".to_string(),
    };
    let options = WindowOptions {
        label: label.clone(),
        title,
        connection: connection.clone(),
    };
    let (window, ctx) = match build_window(state, options, None) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("dbhive: could not open a window: {}", e);
            return None;
        }
    };
    let window = Rc::new(window);
    attach(&window, &ctx, &label);
    if let Err(e) = window.show() {
        eprintln!("dbhive: could not show a window: {}", e);
        close_window(ctx.ui_id);
        return None;
    }
    if let Some(id) = connection {
        ctx.connect(id);
    }
    Some(window)
}

/// The labels of the open windows, oldest first. `main` is the first.
/// Controller ids of the open windows, oldest first.
pub fn window_uids() -> Vec<u64> {
    WINDOWS.with(|w| w.borrow().iter().map(|o| o.ui).collect())
}

pub fn window_labels() -> Vec<String> {
    WINDOWS.with(|w| w.borrow().iter().map(|o| o.label.clone()).collect())
}

/// Close the window with this controller id, closing the app when it was the
/// last one.
pub fn close_window(ui: u64) {
    let found = WINDOWS.with(|w| {
        w.borrow()
            .iter()
            .find(|o| o.ui == ui)
            .map(|o| (o.label.clone(), o.window.clone()))
    });
    if let Some((label, window)) = found {
        record(&label, &window);
    }
    WINDOWS.with(|w| w.borrow_mut().retain(|o| o.ui != ui));
    unregister_ctx(ui);
    if WINDOWS.with(|w| w.borrow().is_empty()) {
        let _ = slint::quit_event_loop();
    }
}

pub fn window_count() -> usize {
    WINDOWS.with(|w| w.borrow().len())
}

fn attach(window: &Rc<AppWindow>, ctx: &Rc<Ctx>, label: &str) {
    restore_geometry(window, label);

    let ui = ctx.ui_id;
    window.window().on_close_requested(move || {
        // Remove the window after this callback returns: dropping the
        // component from inside its own close handler is not allowed.
        let _ = slint::invoke_from_event_loop(move || close_window(ui));
        CloseRequestResponse::HideWindow
    });

    let weak = window.as_weak();
    let poll_label = label.to_string();
    let poll = Timer::default();
    poll.start(TimerMode::Repeated, POLL, move || {
        if let Some(window) = weak.upgrade() {
            record(&poll_label, &window);
        }
    });

    WINDOWS.with(|w| {
        w.borrow_mut().push(OpenWindow {
            ui,
            label: label.to_string(),
            window: Rc::clone(window),
            _poll: poll,
        })
    });
}

fn open_windows() -> Vec<(String, Rc<AppWindow>)> {
    WINDOWS.with(|w| {
        w.borrow()
            .iter()
            .map(|o| (o.label.clone(), o.window.clone()))
            .collect()
    })
}

fn record(label: &str, window: &AppWindow) {
    let Some(g) = read_geometry(window) else {
        return;
    };
    with_saved(|saved| {
        if saved.open().record(label, g) {
            saved.persist();
        }
    });
}

fn read_geometry(window: &AppWindow) -> Option<WindowGeometry> {
    let win = window.window();
    let position = win.position();
    let size = win.size();
    let mut g = WindowGeometry::from_physical(
        position.x,
        position.y,
        size.width,
        size.height,
        win.scale_factor(),
    );
    g.maximized = win.is_maximized();
    g.sanitized()
}

fn restore_geometry(window: &AppWindow, label: &str) {
    let saved = with_saved(|s| s.open().geometry(label));
    let Some(g) = saved else {
        return;
    };
    let win = window.window();
    let (x, y, width, height) = g.to_physical(win.scale_factor());
    win.set_size(slint::PhysicalSize::new(width, height));
    win.set_position(slint::PhysicalPosition::new(x, y));
    if g.maximized {
        win.set_maximized(true);
    }
}

/// `window-2` for the first extra window, then the lowest free number, never
/// reusing one an open or remembered window already holds.
fn next_label() -> String {
    let mut n = 2;
    loop {
        let candidate = format!("window-{n}");
        let open = WINDOWS.with(|w| w.borrow().iter().any(|o| o.label == candidate));
        let saved = with_saved(|s| s.open().geometry(&candidate).is_some());
        if !open && !saved {
            return candidate;
        }
        n += 1;
    }
}

fn label_number(label: &str) -> Option<u32> {
    label.strip_prefix("window-")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_parse_back_to_their_number() {
        assert_eq!(label_number("window-2"), Some(2));
        assert_eq!(label_number("window-17"), Some(17));
        assert_eq!(label_number("main"), None);
    }
}
