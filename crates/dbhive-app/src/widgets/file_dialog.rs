//! Native file and folder pickers.
//!
//! Replaces `tauri-plugin-dialog`. Several screens previously made the user
//! type a path because no picker was available.
//!
//! `rfd::AsyncFileDialog` rather than the blocking `FileDialog`: the
//! blocking variant parks the calling thread until the panel closes, and in
//! an immediate-mode UI that means the window stops repainting entirely.
//! The async variant is driven through the same `Job` every other slow
//! operation uses.

use crate::task::Job;

/// A pending picker and the path it produced.
#[derive(Default)]
pub struct PathPicker {
    job: Job<Option<String>>,
}

/// What the user is choosing.
pub enum Pick {
    /// An existing file, optionally filtered by extension.
    OpenFile {
        title: String,
        filters: Vec<(String, Vec<String>)>,
    },
    /// A path to write, with a suggested name.
    SaveFile {
        title: String,
        suggested_name: String,
        filters: Vec<(String, Vec<String>)>,
    },
    /// A directory.
    Folder { title: String },
}

impl PathPicker {
    pub fn is_open(&self) -> bool {
        self.job.is_running()
    }

    /// Show the panel. The result arrives from `poll` on a later frame.
    pub fn open(&mut self, rt: &tokio::runtime::Handle, ctx: &egui::Context, pick: Pick) {
        // Starting a second panel while one is open would leave the first
        // orphaned, with no way to tell which result belonged to which.
        if self.job.is_running() {
            return;
        }
        self.job.start(rt, ctx, async move {
            let chosen = match pick {
                Pick::OpenFile { title, filters } => {
                    let mut d = rfd::AsyncFileDialog::new().set_title(title);
                    for (name, exts) in &filters {
                        let refs: Vec<&str> = exts.iter().map(|s| s.as_str()).collect();
                        d = d.add_filter(name, &refs);
                    }
                    d.pick_file().await.map(|h| h.path().to_path_buf())
                }
                Pick::SaveFile {
                    title,
                    suggested_name,
                    filters,
                } => {
                    let mut d = rfd::AsyncFileDialog::new()
                        .set_title(title)
                        .set_file_name(suggested_name);
                    for (name, exts) in &filters {
                        let refs: Vec<&str> = exts.iter().map(|s| s.as_str()).collect();
                        d = d.add_filter(name, &refs);
                    }
                    d.save_file().await.map(|h| h.path().to_path_buf())
                }
                Pick::Folder { title } => rfd::AsyncFileDialog::new()
                    .set_title(title)
                    .pick_folder()
                    .await
                    .map(|h| h.path().to_path_buf()),
            };
            chosen.map(|p| p.to_string_lossy().into_owned())
        });
    }

    /// The chosen path, once. Cancelling yields `None` and nothing changes.
    pub fn poll(&mut self) -> Option<String> {
        self.job.take_result().flatten()
    }
}

/// Draw a path field with a Browse button beside it.
///
/// Writes straight into `path` when the user picks something, so callers do
/// not have to thread the result back themselves.
pub fn path_field(
    ui: &mut egui::Ui,
    rt: &tokio::runtime::Handle,
    picker: &mut PathPicker,
    path: &mut String,
    hint: &str,
    pick: impl FnOnce() -> Pick,
) -> bool {
    let mut changed = false;
    if let Some(chosen) = picker.poll() {
        *path = chosen;
        changed = true;
    }
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(path)
                .hint_text(hint)
                .desired_width(320.0),
        );
        let busy = picker.is_open();
        if ui
            .add_enabled(!busy, egui::Button::new("Browse…"))
            .clicked()
        {
            let ctx = ui.ctx().clone();
            picker.open(rt, &ctx, pick());
        }
        if busy {
            ui.add(egui::Spinner::new());
        }
    });
    changed
}

/// Common filter sets.
pub fn sql_filters() -> Vec<(String, Vec<String>)> {
    vec![("SQL".into(), vec!["sql".into()])]
}
pub fn data_filters() -> Vec<(String, Vec<String>)> {
    vec![
        (
            "Data files".into(),
            vec!["csv".into(), "tsv".into(), "xlsx".into(), "xls".into()],
        ),
        ("CSV".into(), vec!["csv".into()]),
        ("Excel".into(), vec!["xlsx".into(), "xls".into()]),
    ]
}
pub fn json_filters() -> Vec<(String, Vec<String>)> {
    vec![("JSON".into(), vec!["json".into()])]
}
pub fn sqlite_filters() -> Vec<(String, Vec<String>)> {
    vec![(
        "SQLite database".into(),
        vec!["db".into(), "sqlite".into(), "sqlite3".into()],
    )]
}
