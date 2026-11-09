//! The workspace's resizable panes: the schema sidebar's width and the SQL
//! editor's height, remembered across restarts in `layout.json`.
//!
//! The clamp and the file format live in `dbhive_ui_model::layout`; this
//! file only moves numbers between the window, the session and the store.

use std::rc::Rc;

use dbhive_core::storage::JsonStore;
use dbhive_ui_model::layout::PaneSizes;

use super::Ctx;
use crate::AppWindow;

const STORE_FILE: &str = "layout.json";
const STORE_KEY: &str = "panes";

#[derive(Default)]
pub(super) struct LayoutSession {
    pub(super) sizes: PaneSizes,
    store: Option<JsonStore>,
}

impl LayoutSession {
    /// The saved layout, or the defaults on a first run.
    fn load() -> Self {
        match JsonStore::open(STORE_FILE) {
            Ok(store) => {
                let sizes = PaneSizes::from_json(store.get(STORE_KEY));
                Self {
                    sizes,
                    store: Some(store),
                }
            }
            Err(e) => {
                eprintln!("dbhive: could not open {}: {}", STORE_FILE, e);
                Self::default()
            }
        }
    }

    fn save(&mut self) {
        let Some(store) = self.store.as_mut() else {
            return;
        };
        store.set(STORE_KEY, self.sizes.to_json());
        if let Err(e) = store.save() {
            eprintln!("dbhive: could not save {}: {}", STORE_FILE, e);
        }
    }
}

impl Ctx {
    pub(super) fn resize_sidebar(&self, width: f32) {
        let width = PaneSizes::clamp_sidebar(width);
        self.session.borrow_mut().layout.sizes.sidebar = width;
        if let Some(w) = self.window.upgrade() {
            w.set_sidebar_width(width);
        }
    }

    pub(super) fn resize_editor(&self, height: f32) {
        let height = PaneSizes::clamp_editor(height);
        self.session.borrow_mut().layout.sizes.editor = height;
        if let Some(w) = self.window.upgrade() {
            w.set_editor_height(height);
        }
    }

    pub(super) fn layout_committed(&self) {
        self.session.borrow_mut().layout.save();
    }
}

/// Apply the saved sizes to this window and wire its two splitters. Each
/// window has its own sizes; the store is shared, so the last drag wins.
pub(super) fn wire(window: &AppWindow, ctx: &Rc<Ctx>) {
    let layout = LayoutSession::load();
    let sizes = layout.sizes;
    {
        let mut s = ctx.session.borrow_mut();
        s.layout = layout;
    }
    window.set_sidebar_width(sizes.sidebar);
    window.set_editor_height(sizes.editor);

    {
        let ctx = ctx.clone();
        window.on_sidebar_resize(move |w| ctx.resize_sidebar(w));
    }
    {
        let ctx = ctx.clone();
        window.on_editor_resize(move |h| ctx.resize_editor(h));
    }
    {
        let ctx = ctx.clone();
        window.on_layout_committed(move || ctx.layout_committed());
    }
}
