//! Native file and folder pickers.
//!
//! What to ask for (title, filters, starting folder, suggested name) is
//! `dbhive_ui_model::file_pick`. This module only shows the platform panel.
//!
//! `rfd::AsyncFileDialog`, never the blocking `FileDialog`: the blocking one
//! parks the calling thread until the panel closes, and on the UI thread that
//! freezes the window. The async future runs on the shared Tokio runtime and
//! the chosen path comes back through `runtime::spawn`, like any other slow
//! operation. (On macOS rfd hands the panel itself to the main thread.)
//!
//! Tests must never open a real panel: they install a [`FilePicker`] with
//! [`use_picker`] before building the window.

use std::cell::RefCell;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use dbhive_ui_model::file_pick::{PickKind, PickRequest};

/// The chosen path, or `None` when the panel was cancelled.
pub type PickFuture = Pin<Box<dyn Future<Output = Option<PathBuf>> + Send>>;

/// Something that can answer a [`PickRequest`].
pub trait FilePicker: Send + Sync {
    fn pick(&self, request: PickRequest) -> PickFuture;
}

/// The platform's own panels, through rfd.
pub struct NativePicker;

impl FilePicker for NativePicker {
    fn pick(&self, request: PickRequest) -> PickFuture {
        Box::pin(async move {
            let mut d = rfd::AsyncFileDialog::new().set_title(&request.title);
            if let Some(dir) = &request.directory {
                d = d.set_directory(dir);
            }
            for (name, exts) in &request.filters {
                let refs: Vec<&str> = exts.iter().map(String::as_str).collect();
                d = d.add_filter(name, &refs);
            }
            let handle = match request.kind {
                PickKind::Open => d.pick_file().await,
                PickKind::Save => {
                    if let Some(name) = &request.file_name {
                        d = d.set_file_name(name);
                    }
                    d.save_file().await
                }
                PickKind::Folder => d.pick_folder().await,
            };
            handle.map(|h| h.path().to_path_buf())
        })
    }
}

/// A picker that answers every request with the same path (or with a
/// cancel), recording what it was asked. For tests.
pub struct FixedPicker {
    answer: Option<PathBuf>,
    asked: std::sync::Mutex<Vec<PickRequest>>,
}

impl FixedPicker {
    pub fn new(answer: Option<PathBuf>) -> Self {
        Self {
            answer,
            asked: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Every request made so far, oldest first.
    pub fn requests(&self) -> Vec<PickRequest> {
        self.asked.lock().unwrap().clone()
    }
}

impl FilePicker for FixedPicker {
    fn pick(&self, request: PickRequest) -> PickFuture {
        self.asked.lock().unwrap().push(request);
        let answer = self.answer.clone();
        Box::pin(async move { answer })
    }
}

thread_local! {
    static OVERRIDE: RefCell<Option<Arc<dyn FilePicker>>> = const { RefCell::new(None) };
}

/// Use `picker` for windows built on this thread from now on, instead of the
/// native panels. A test calls this before `app::build`.
pub fn use_picker(picker: Arc<dyn FilePicker>) {
    OVERRIDE.with(|o| *o.borrow_mut() = Some(picker));
}

/// The picker a new window should use.
pub fn current() -> Arc<dyn FilePicker> {
    OVERRIDE
        .with(|o| o.borrow().clone())
        .unwrap_or_else(|| Arc::new(NativePicker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_override_wins_and_records_requests() {
        let fixed = Arc::new(FixedPicker::new(Some(PathBuf::from("/tmp/x.db"))));
        use_picker(fixed.clone());
        let picker = current();
        let req = dbhive_ui_model::file_pick::sqlite_file("");
        let got = crate::runtime::handle().block_on(picker.pick(req.clone()));
        assert_eq!(got, Some(PathBuf::from("/tmp/x.db")));
        assert_eq!(fixed.requests(), vec![req]);
    }
}
