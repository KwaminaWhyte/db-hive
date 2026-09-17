//! Query history, saved snippets and query templates.
//!
//! Parity target: the React `HistoryPanel`, `SnippetSidebar` and
//! `TemplatesPanel`, which the web shell showed as three separate panels.
//! They share one side panel here because they are all "a list of SQL you
//! can put back in the editor", and a native window has no room for three
//! sidebars.
//!
//! Clicking an entry does not run anything itself: the panel has no editor
//! and no connection. It returns [`HistoryAction::UseSql`] and the caller
//! decides what to do with the statement, which keeps the panel usable from
//! the workspace, a dialog, or anywhere else.
//!
//! History and snippets live in `AppState` (`history.json`, `snippets.json`).
//! Templates had no backend at all in the web shell: `useQueryTemplates`
//! kept them in `localStorage`, which does not exist here, so they are
//! persisted to `templates.json` in the same application data directory
//! through `dbhive_core::storage::JsonStore`.
//!
//! The pure logic (filtering, formatting, the template model, placeholder
//! scanning and substitution, validation, import merging and template
//! persistence) lives, with its tests, in `dbhive_ui_model::history`; this
//! file is drawing and glue.

use std::collections::HashMap;

use chrono::Utc;

use dbhive_core::models::{QueryHistory, QuerySnippet};
use dbhive_core::state::AppState;

// Re-exported so `screens::history::ParamType` and friends keep their paths.
pub use dbhive_ui_model::history::*;

use crate::app::DbHiveApp;
use crate::task::Job;

/// What the caller should do once the panel has been drawn.
///
/// The panel cannot execute SQL itself (it holds neither the editor nor a
/// connection), so a click is handed back rather than acted on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryAction {
    None,
    /// Put this statement in the editor.
    UseSql(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    History,
    Snippets,
    Templates,
}

// ===========================================================================
// Screen state
// ===========================================================================

pub struct History {
    pub open: bool,
    pub tab: Tab,

    // History tab
    pub limit: usize,
    pub entries: Vec<QueryHistory>,
    /// Connection, database and limit the loaded entries belong to. A change
    /// here reloads, which is what the React `useCallback` dependency list
    /// did.
    loaded_key: Option<(Option<String>, Option<String>, usize)>,
    pub confirm_clear: bool,

    // Snippets tab
    pub snippets: Vec<QuerySnippet>,
    snippets_loaded: bool,
    pub snippet_search: String,
    pub snippet_tag: Option<String>,
    pub snippet_form: Option<SnippetForm>,
    /// (id, name) of the snippet awaiting delete confirmation.
    pub snippet_delete: Option<(String, String)>,

    // Templates tab
    pub templates: Vec<QueryTemplate>,
    templates_requested: bool,
    pub template_search: String,
    pub template_category: Option<String>,
    pub template_form: Option<TemplateForm>,
    pub template_run: Option<TemplateRun>,
    pub template_delete: Option<(String, String)>,
    pub import_path: String,
    pub show_import: bool,

    pub error: Option<String>,
    load_templates_job: Job<Result<Vec<QueryTemplate>, String>>,
    save_job: Job<Result<(), String>>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            open: false,
            tab: Tab::default(),
            limit: 50,
            entries: Vec::new(),
            loaded_key: None,
            confirm_clear: false,
            snippets: Vec::new(),
            snippets_loaded: false,
            snippet_search: String::new(),
            snippet_tag: None,
            snippet_form: None,
            snippet_delete: None,
            templates: Vec::new(),
            templates_requested: false,
            template_search: String::new(),
            template_category: None,
            template_form: None,
            template_run: None,
            template_delete: None,
            import_path: String::new(),
            show_import: false,
            error: None,
            load_templates_job: Job::default(),
            save_job: Job::default(),
        }
    }
}

impl History {
    /// Drop the loaded history so the next frame refetches it.
    ///
    /// The web shell passed a `refreshTrigger` prop that it bumped after
    /// every execution; call this from the workspace once a statement has
    /// been recorded, or the panel keeps showing the previous set until the
    /// user presses Refresh.
    pub fn invalidate_history(&mut self) {
        self.loaded_key = None;
    }

    /// Drop both history and snippets, e.g. after another window wrote to
    /// the shared stores.
    pub fn invalidate(&mut self) {
        self.loaded_key = None;
        self.snippets_loaded = false;
    }
}

/// Create/edit form for a snippet.
#[derive(Default)]
pub struct SnippetForm {
    /// `None` while creating.
    pub id: Option<String>,
    pub name: String,
    pub query: String,
    pub description: String,
    /// Comma separated, as the web shell's single tags input was.
    pub tags: String,
}

/// Create/edit form for a template.
pub struct TemplateForm {
    pub id: Option<String>,
    pub name: String,
    pub description: String,
    pub category: String,
    pub tags: String,
    pub sql: String,
    pub parameters: Vec<TemplateParameter>,
    /// The SQL the parameter list was last derived from. When the two differ
    /// the list is rebuilt, which is the web shell's two-way live sync.
    synced_sql: String,
}

impl Default for TemplateForm {
    fn default() -> Self {
        Self {
            id: None,
            name: String::new(),
            description: String::new(),
            category: String::new(),
            tags: String::new(),
            sql: String::new(),
            parameters: Vec::new(),
            synced_sql: String::new(),
        }
    }
}

/// Fill-in form shown before running a parameterised template.
pub struct TemplateRun {
    pub template: QueryTemplate,
    pub values: HashMap<String, String>,
    pub errors: HashMap<String, String>,
}

/// Work that has to happen outside the drawing borrow of `app.screens`.
enum Intent {
    RefreshHistory,
    ClearHistory,
    SaveSnippet(QuerySnippet),
    DeleteSnippet(String),
    SaveTemplates,
    ExportTemplates,
    ImportTemplates(String),
}


// ===========================================================================
// Screen
// ===========================================================================

/// Draw the panel. The returned action is the statement the user picked, if
/// any; the caller owns the editor and decides what to do with it.
pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) -> HistoryAction {
    let palette = app.mode.palette();
    let ctx = ui.ctx().clone();
    let (connection_id, database) = active_scope(app);

    // --- poll background work -------------------------------------------
    if let Some(result) = app.screens.history.load_templates_job.take_result() {
        match result {
            Ok(list) => app.screens.history.templates = list,
            Err(e) => {
                app.screens.history.error = Some(e.clone());
                app.toasts.error(format!("Could not load templates: {}", e));
            }
        }
    }
    if let Some(Err(e)) = app.screens.history.save_job.take_result() {
        app.screens.history.error = Some(e.clone());
        app.toasts.error(e);
    }

    // --- first load / reload on scope change ------------------------------
    if !app.screens.history.templates_requested {
        app.screens.history.templates_requested = true;
        app.screens
            .history
            .load_templates_job
            .start(app.rt.handle(), &ctx, async move { load_templates() });
    }
    if !app.screens.history.snippets_loaded {
        reload_snippets(app);
    }
    let key = (connection_id.clone(), database.clone(), app.screens.history.limit);
    if app.screens.history.loaded_key.as_ref() != Some(&key) {
        reload_history(app, &connection_id, &database);
    }

    // --- draw --------------------------------------------------------------
    let mut action = HistoryAction::None;
    let mut intents: Vec<Intent> = Vec::new();
    {
        let h = &mut app.screens.history;

        ui.horizontal(|ui| {
            ui.selectable_value(&mut h.tab, Tab::History, "History");
            ui.selectable_value(&mut h.tab, Tab::Snippets, "Snippets");
            ui.selectable_value(&mut h.tab, Tab::Templates, "Templates");
        });
        ui.add_space(6.0);

        if let Some(err) = h.error.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(palette.destructive, err);
                if ui.small_button("Dismiss").clicked() {
                    h.error = None;
                }
            });
            ui.add_space(4.0);
        }

        match h.tab {
            Tab::History => history_tab(
                h,
                ui,
                palette,
                connection_id.as_deref(),
                database.as_deref(),
                &mut action,
                &mut intents,
            ),
            Tab::Snippets => snippets_tab(h, ui, palette, &mut action, &mut intents),
            Tab::Templates => templates_tab(h, ui, palette, &mut action, &mut intents),
        }

        // Dialogs are drawn last so they sit above the list.
        clear_history_dialog(h, &ctx, palette, connection_id.as_deref(), database.as_deref(), &mut intents);
        snippet_form_dialog(h, &ctx, palette, &mut intents);
        confirm_delete_dialog(h, &ctx, palette, &mut intents);
        template_form_dialog(h, &ctx, palette, &mut intents);
        template_run_dialog(h, &ctx, palette, &mut action, &mut intents);
        import_dialog(h, &ctx, palette, &mut intents);
    }

    // --- act ---------------------------------------------------------------
    for intent in intents {
        apply(app, &ctx, &connection_id, &database, intent);
    }

    action
}

/// Connection and database the panel scopes itself to, from the open
/// workspace. Nothing open means unscoped, and the panel says so.
fn active_scope(app: &DbHiveApp) -> (Option<String>, Option<String>) {
    match app.workspace.as_ref() {
        Some(w) => (Some(w.connection_id.clone()), w.current_database.clone()),
        None => (None, None),
    }
}

fn reload_history(app: &mut DbHiveApp, connection_id: &Option<String>, database: &Option<String>) {
    // Copy out of the lock first; filtering happens with the lock released.
    let all: Vec<QueryHistory> = {
        let guard = app.state.lock().unwrap();
        guard.get_all_history()
    };
    let h = &mut app.screens.history;
    h.entries = filter_history(&all, connection_id.as_deref(), database.as_deref(), h.limit);
    h.loaded_key = Some((connection_id.clone(), database.clone(), h.limit));
}

fn reload_snippets(app: &mut DbHiveApp) {
    let mut snippets: Vec<QuerySnippet> = {
        let guard = app.state.lock().unwrap();
        guard.get_all_snippets()
    };
    snippets.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    let h = &mut app.screens.history;
    h.snippets = snippets;
    h.snippets_loaded = true;
}

fn apply(
    app: &mut DbHiveApp,
    ctx: &egui::Context,
    connection_id: &Option<String>,
    database: &Option<String>,
    intent: Intent,
) {
    match intent {
        Intent::RefreshHistory => reload_history(app, connection_id, database),

        Intent::ClearHistory => {
            // Clear inside the lock, snapshot, then write to disk off the
            // frame: a synchronous file write in `update` is a stall.
            let snapshot = {
                let mut guard = app.state.lock().unwrap();
                match connection_id {
                    Some(id) => guard.clear_history_by_connection(id),
                    None => guard.clear_all_history(),
                };
                guard.query_history.clone()
            };
            app.screens.history.save_job.start(app.rt.handle(), ctx, async move {
                AppState::save_history_to_store(&snapshot).map_err(|e| e.to_string())
            });
            reload_history(app, connection_id, database);
            app.toasts.success("Query history cleared");
        }

        Intent::SaveSnippet(snippet) => {
            {
                let mut guard = app.state.lock().unwrap();
                guard.add_snippet(snippet);
            }
            persist_snippets(app, ctx);
            reload_snippets(app);
            app.toasts.success("Snippet saved");
        }

        Intent::DeleteSnippet(id) => {
            let removed = {
                let mut guard = app.state.lock().unwrap();
                guard.remove_snippet(&id).is_some()
            };
            if removed {
                persist_snippets(app, ctx);
                reload_snippets(app);
                app.toasts.success("Snippet deleted");
            } else {
                app.toasts.error("That snippet no longer exists");
            }
        }

        Intent::SaveTemplates => {
            let snapshot = app.screens.history.templates.clone();
            app.screens
                .history
                .save_job
                .start(app.rt.handle(), ctx, async move { save_templates(&snapshot) });
        }

        Intent::ExportTemplates => {
            let snapshot = app.screens.history.templates.clone();
            match export_templates_now(&snapshot) {
                Ok(path) => app.toasts.success(format!("Exported to {}", path.display())),
                Err(e) => {
                    app.screens.history.error = Some(e.clone());
                    app.toasts.error(e);
                }
            }
        }

        Intent::ImportTemplates(path) => match import_templates_now(path.trim()) {
            Ok(incoming) => {
                let added = merge_imported(&app.screens.history.templates, incoming);
                let count = added.len();
                app.screens.history.templates.extend(added);
                if count > 0 {
                    apply(app, ctx, connection_id, database, Intent::SaveTemplates);
                }
                app.toasts
                    .success(format!("Imported {} template(s)", count));
                app.screens.history.show_import = false;
            }
            Err(e) => {
                app.screens.history.error = Some(e.clone());
                app.toasts.error(e);
            }
        },
    }
}

/// Persist snippets through core, off the frame.
///
/// The closure has no await points, so holding the `std::sync::Mutex` guard
/// inside it is safe; it just must not happen on the UI thread.
fn persist_snippets(app: &mut DbHiveApp, ctx: &egui::Context) {
    let state = app.state.clone();
    app.screens.history.save_job.start(app.rt.handle(), ctx, async move {
        let guard = state.lock().unwrap();
        guard.save_snippets_to_store().map_err(|e| e.to_string())
    });
}


// ===========================================================================
// History tab
// ===========================================================================

#[allow(clippy::too_many_arguments)]
fn history_tab(
    h: &mut History,
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    connection_id: Option<&str>,
    database: Option<&str>,
    action: &mut HistoryAction,
    intents: &mut Vec<Intent>,
) {
    ui.label(crate::theme::text::strong(palette, "Query History"));
    ui.label(crate::theme::text::muted(
        palette,
        match (connection_id, database) {
            (Some(_), Some(db)) => format!("History for {}", db),
            (Some(_), None) => "History for current connection".to_string(),
            _ => "No active connection".to_string(),
        },
    ));
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("history_limit")
            .selected_text(format!("Last {}", h.limit))
            .width(110.0)
            .show_ui(ui, |ui| {
                for limit in HISTORY_LIMITS {
                    ui.selectable_value(&mut h.limit, limit, format!("Last {}", limit));
                }
            });
        if ui.button("Refresh").clicked() {
            intents.push(Intent::RefreshHistory);
        }
        if ui
            .add_enabled(!h.entries.is_empty(), egui::Button::new("Clear"))
            .clicked()
        {
            h.confirm_clear = true;
        }
    });
    ui.add_space(6.0);

    if h.entries.is_empty() {
        ui.add_space(32.0);
        ui.vertical_centered(|ui| {
            ui.label(crate::theme::text::heading(palette, "No Query History"));
            ui.label(crate::theme::text::muted(
                palette,
                "Your executed queries will appear here. Run a query to get started.",
            ));
            ui.add_space(8.0);
            if ui.button("Run Your First Query").clicked() {
                *action = HistoryAction::UseSql("SELECT 1;".to_string());
            }
        });
        return;
    }

    let now = Utc::now();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for entry in &h.entries {
                egui::Frame::none()
                    .fill(palette.card)
                    .stroke(egui::Stroke::new(1.0_f32, palette.border))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if entry.success {
                                ui.colored_label(palette.success, "Success");
                            } else {
                                ui.colored_label(palette.destructive, "Failed");
                            }
                            ui.label(crate::theme::text::muted(
                                palette,
                                format_relative(&entry.executed_at, now),
                            ));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    // Only successful statements are offered
                                    // for re-run, as the web shell did.
                                    if entry.success && ui.small_button("Re-run").clicked() {
                                        *action = HistoryAction::UseSql(entry.query.clone());
                                    }
                                },
                            );
                        });

                        ui.label(crate::theme::text::muted(
                            palette,
                            format!("{} / {}", entry.connection_name, entry.database),
                        ));
                        ui.add_space(4.0);

                        egui::Frame::none()
                            .fill(palette.muted)
                            .rounding(egui::Rounding::same(4.0))
                            .inner_margin(egui::Margin::same(6.0))
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(entry.query.trim())
                                            .monospace()
                                            .size(11.0)
                                            .color(palette.foreground),
                                    )
                                    .wrap(),
                                );
                            });

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(crate::theme::text::muted(
                                palette,
                                format!("Time: {}", format_execution_time(entry.execution_time_ms)),
                            ));
                            if entry.success {
                                if let Some(rows) = entry.row_count {
                                    ui.label(crate::theme::text::muted(
                                        palette,
                                        format!("Rows: {}", format_row_count(rows)),
                                    ));
                                }
                            }
                        });

                        if let Some(err) = &entry.error_message {
                            ui.colored_label(
                                palette.destructive,
                                egui::RichText::new(format!("Error: {}", err)).size(11.0),
                            );
                        }
                    });
                ui.add_space(6.0);
            }
        });
}

fn clear_history_dialog(
    h: &mut History,
    ctx: &egui::Context,
    palette: &crate::theme::Palette,
    connection_id: Option<&str>,
    database: Option<&str>,
    intents: &mut Vec<Intent>,
) {
    if !h.confirm_clear {
        return;
    }
    let mut open = true;
    egui::Window::new("Clear query history?")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(crate::theme::text::muted(
                palette,
                match (connection_id, database) {
                    (Some(_), Some(db)) => format!(
                        "This will permanently delete the query history for the {} database.",
                        db
                    ),
                    (Some(_), None) => {
                        "This will permanently delete the query history for the current connection."
                            .to_string()
                    }
                    _ => "This will permanently delete all query history.".to_string(),
                },
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    h.confirm_clear = false;
                }
                if ui
                    .button(egui::RichText::new("Clear History").color(palette.destructive))
                    .clicked()
                {
                    intents.push(Intent::ClearHistory);
                    h.confirm_clear = false;
                }
            });
        });
    if !open {
        h.confirm_clear = false;
    }
}

// ===========================================================================
// Snippets tab
// ===========================================================================

fn snippets_tab(
    h: &mut History,
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    action: &mut HistoryAction,
    intents: &mut Vec<Intent>,
) {
    ui.horizontal(|ui| {
        ui.label(crate::theme::text::strong(palette, "Snippets"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("New").clicked() {
                h.snippet_form = Some(SnippetForm::default());
            }
        });
    });
    ui.add_space(4.0);

    ui.add(
        egui::TextEdit::singleline(&mut h.snippet_search)
            .hint_text("Search snippets...")
            .desired_width(f32::INFINITY),
    );

    let tags = snippet_tags(&h.snippets);
    if !tags.is_empty() {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(h.snippet_tag.is_none(), "All")
                .clicked()
            {
                h.snippet_tag = None;
            }
            for tag in &tags {
                let selected = h.snippet_tag.as_deref() == Some(tag.as_str());
                if ui.selectable_label(selected, tag).clicked() {
                    // A second click on the active pill clears it, which the
                    // web shell's single-select tag filter also did.
                    h.snippet_tag = if selected { None } else { Some(tag.clone()) };
                }
            }
        });
    }
    ui.add_space(6.0);

    let visible: Vec<QuerySnippet> = filter_snippets(
        &h.snippets,
        &h.snippet_search,
        h.snippet_tag.as_deref(),
    )
    .into_iter()
    .cloned()
    .collect();

    if visible.is_empty() {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            if h.snippets.is_empty() {
                ui.label(crate::theme::text::heading(palette, "No snippets yet"));
                ui.label(crate::theme::text::muted(
                    palette,
                    "Save a statement you use often and it will show up here.",
                ));
            } else {
                ui.label(crate::theme::text::heading(palette, "No matches"));
                ui.label(crate::theme::text::muted(
                    palette,
                    "No snippet matches that search.",
                ));
            }
        });
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for snippet in &visible {
                egui::Frame::none()
                    .fill(palette.card)
                    .stroke(egui::Stroke::new(1.0_f32, palette.border))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Clicking the body inserts. The web shell used
                            // double-click for this and single-click to edit,
                            // which was a persistent complaint; the actions
                            // are explicit buttons here instead.
                            let title = ui
                                .add(
                                    egui::Label::new(crate::theme::text::strong(
                                        palette,
                                        snippet.name.clone(),
                                    ))
                                    .sense(egui::Sense::click()),
                                )
                                .on_hover_text("Click to use this snippet");
                            if title.clicked() {
                                *action = HistoryAction::UseSql(snippet.query.clone());
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("Delete").clicked() {
                                        h.snippet_delete =
                                            Some((snippet.id.clone(), snippet.name.clone()));
                                    }
                                    if ui.small_button("Edit").clicked() {
                                        h.snippet_form = Some(SnippetForm {
                                            id: Some(snippet.id.clone()),
                                            name: snippet.name.clone(),
                                            query: snippet.query.clone(),
                                            description: snippet
                                                .description
                                                .clone()
                                                .unwrap_or_default(),
                                            tags: snippet
                                                .tags
                                                .clone()
                                                .unwrap_or_default()
                                                .join(", "),
                                        });
                                    }
                                    if ui.small_button("Use").clicked() {
                                        *action = HistoryAction::UseSql(snippet.query.clone());
                                    }
                                },
                            );
                        });

                        if let Some(desc) = snippet.description.as_deref() {
                            if !desc.is_empty() {
                                ui.label(crate::theme::text::muted(palette, desc));
                            }
                        }
                        ui.label(
                            egui::RichText::new(first_line(&snippet.query, 90))
                                .monospace()
                                .size(11.0)
                                .color(palette.muted_foreground),
                        );
                        if let Some(tags) = &snippet.tags {
                            if !tags.is_empty() {
                                ui.label(crate::theme::text::muted(
                                    palette,
                                    tags.join(" · "),
                                ));
                            }
                        }
                    });
                ui.add_space(6.0);
            }
        });

    // Nothing above this point may mutate the snippet list; it is borrowed
    // by the loop, so saves go through intents.
    let _ = intents;
}

fn snippet_form_dialog(
    h: &mut History,
    ctx: &egui::Context,
    palette: &crate::theme::Palette,
    intents: &mut Vec<Intent>,
) {
    let Some(form) = h.snippet_form.as_mut() else {
        return;
    };
    let editing = form.id.is_some();
    let mut open = true;
    let mut close = false;

    egui::Window::new(if editing { "Edit snippet" } else { "New snippet" })
        .collapsible(false)
        .resizable(true)
        .default_width(460.0)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(crate::theme::text::muted(palette, "Name"));
            ui.add(
                egui::TextEdit::singleline(&mut form.name)
                    .hint_text("e.g. Active users")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(6.0);

            ui.label(crate::theme::text::muted(palette, "Description"));
            ui.add(
                egui::TextEdit::singleline(&mut form.description)
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(6.0);

            ui.label(crate::theme::text::muted(palette, "Tags (comma separated)"));
            ui.add(egui::TextEdit::singleline(&mut form.tags).desired_width(f32::INFINITY));
            ui.add_space(6.0);

            ui.label(crate::theme::text::muted(palette, "SQL"));
            ui.add(
                egui::TextEdit::multiline(&mut form.query)
                    .code_editor()
                    .desired_rows(8)
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                let valid = !form.name.trim().is_empty() && !form.query.trim().is_empty();
                if ui
                    .add_enabled(valid, egui::Button::new("Save"))
                    .clicked()
                {
                    let tags = parse_tags(&form.tags);
                    let mut snippet = QuerySnippet::new(
                        form.name.trim().to_string(),
                        form.query.clone(),
                        Some(form.description.trim().to_string())
                            .filter(|d| !d.is_empty()),
                        if tags.is_empty() { None } else { Some(tags) },
                    );
                    if let Some(id) = &form.id {
                        // Keep the id so this updates rather than duplicates.
                        snippet.id = id.clone();
                    }
                    intents.push(Intent::SaveSnippet(snippet));
                    close = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });

    if close || !open {
        h.snippet_form = None;
    }
}

fn confirm_delete_dialog(
    h: &mut History,
    ctx: &egui::Context,
    palette: &crate::theme::Palette,
    intents: &mut Vec<Intent>,
) {
    if let Some((id, name)) = h.snippet_delete.clone() {
        let mut open = true;
        let mut close = false;
        egui::Window::new("Delete snippet")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(crate::theme::text::muted(
                    palette,
                    format!("Delete \"{}\"? This cannot be undone.", name),
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui
                        .button(egui::RichText::new("Delete").color(palette.destructive))
                        .clicked()
                    {
                        intents.push(Intent::DeleteSnippet(id.clone()));
                        close = true;
                    }
                });
            });
        if close || !open {
            h.snippet_delete = None;
        }
    }

    if let Some((id, name)) = h.template_delete.clone() {
        let mut open = true;
        let mut close = false;
        egui::Window::new("Delete template")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(crate::theme::text::muted(
                    palette,
                    format!("Delete \"{}\"? This cannot be undone.", name),
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui
                        .button(egui::RichText::new("Delete").color(palette.destructive))
                        .clicked()
                    {
                        h.templates.retain(|t| t.id != id);
                        intents.push(Intent::SaveTemplates);
                        close = true;
                    }
                });
            });
        if close || !open {
            h.template_delete = None;
        }
    }
}

// ===========================================================================
// Templates tab
// ===========================================================================

fn templates_tab(
    h: &mut History,
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    action: &mut HistoryAction,
    intents: &mut Vec<Intent>,
) {
    ui.horizontal(|ui| {
        ui.label(crate::theme::text::strong(palette, "Templates"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("New").clicked() {
                h.template_form = Some(TemplateForm::default());
            }
            if ui
                .add_enabled(!h.templates.is_empty(), egui::Button::new("Export"))
                .on_hover_text("Write every template to a JSON file")
                .clicked()
            {
                intents.push(Intent::ExportTemplates);
            }
            if ui.button("Import").clicked() {
                h.show_import = true;
            }
        });
    });
    ui.add_space(4.0);

    ui.add(
        egui::TextEdit::singleline(&mut h.template_search)
            .hint_text("Search templates...")
            .desired_width(f32::INFINITY),
    );

    let categories = template_categories(&h.templates);
    if !categories.is_empty() {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(h.template_category.is_none(), "All")
                .clicked()
            {
                h.template_category = None;
            }
            for cat in &categories {
                let selected = h.template_category.as_deref() == Some(cat.as_str());
                if ui.selectable_label(selected, cat).clicked() {
                    h.template_category = if selected { None } else { Some(cat.clone()) };
                }
            }
        });
    }
    ui.add_space(6.0);

    if h.load_templates_job.is_running() {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new());
            ui.label(crate::theme::text::muted(palette, "Loading templates"));
        });
        return;
    }

    let visible: Vec<QueryTemplate> = filter_templates(
        &h.templates,
        &h.template_search,
        h.template_category.as_deref(),
    )
    .into_iter()
    .cloned()
    .collect();

    if visible.is_empty() {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            if h.templates.is_empty() {
                ui.label(crate::theme::text::heading(palette, "No templates yet"));
                ui.label(crate::theme::text::muted(
                    palette,
                    "Create a template to save reusable queries with parameters.",
                ));
            } else {
                ui.label(crate::theme::text::heading(palette, "No templates found"));
            }
        });
        return;
    }

    let mut duplicate: Option<String> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for template in &visible {
                egui::Frame::none()
                    .fill(palette.card)
                    .stroke(egui::Stroke::new(1.0_f32, palette.border))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let title = ui
                                .add(
                                    egui::Label::new(crate::theme::text::strong(
                                        palette,
                                        template.name.clone(),
                                    ))
                                    .sense(egui::Sense::click()),
                                )
                                .on_hover_text("Click to use this template");
                            if title.clicked() {
                                run_template(h, template, action);
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("Delete").clicked() {
                                        h.template_delete =
                                            Some((template.id.clone(), template.name.clone()));
                                    }
                                    if ui.small_button("Duplicate").clicked() {
                                        duplicate = Some(template.id.clone());
                                    }
                                    if ui.small_button("Edit").clicked() {
                                        h.template_form = Some(form_from(template));
                                    }
                                    if ui.small_button("Use").clicked() {
                                        run_template(h, template, action);
                                    }
                                },
                            );
                        });

                        if let Some(desc) = template.description.as_deref() {
                            if !desc.is_empty() {
                                ui.label(crate::theme::text::muted(palette, desc));
                            }
                        }
                        ui.horizontal_wrapped(|ui| {
                            if let Some(cat) = template.category.as_deref() {
                                if !cat.is_empty() {
                                    ui.label(crate::theme::text::muted(palette, cat));
                                }
                            }
                            let n = template.parameters.len();
                            if n > 0 {
                                ui.label(crate::theme::text::muted(
                                    palette,
                                    format!("{} param{}", n, if n == 1 { "" } else { "s" }),
                                ));
                            }
                        });
                        ui.label(
                            egui::RichText::new(first_line(&template.sql, 90))
                                .monospace()
                                .size(11.0)
                                .color(palette.muted_foreground),
                        );
                    });
                ui.add_space(6.0);
            }
        });

    if let Some(id) = duplicate {
        if let Some(original) = h.templates.iter().find(|t| t.id == id).cloned() {
            let copy = QueryTemplate::new(
                format!("{} (Copy)", original.name),
                original.description.clone(),
                original.sql.clone(),
                original.parameters.clone(),
                original.category.clone(),
                original.tags.clone(),
            );
            h.templates.push(copy);
            intents.push(Intent::SaveTemplates);
        }
    }
}

/// A zero-parameter template runs straight away; anything else opens the
/// fill-in form first.
fn run_template(h: &mut History, template: &QueryTemplate, action: &mut HistoryAction) {
    if template.parameters.is_empty() {
        *action = HistoryAction::UseSql(template.sql.clone());
        return;
    }
    let values: HashMap<String, String> = template
        .parameters
        .iter()
        .map(|p| (p.name.clone(), p.default_value.clone().unwrap_or_default()))
        .collect();
    h.template_run = Some(TemplateRun {
        errors: validate_template_values(template, &values),
        template: template.clone(),
        values,
    });
    // The freshly opened form should not shout about empty required fields
    // before the user has typed anything.
    if let Some(run) = h.template_run.as_mut() {
        run.errors.retain(|_, e| e != "This field is required");
    }
}

fn form_from(template: &QueryTemplate) -> TemplateForm {
    TemplateForm {
        id: Some(template.id.clone()),
        name: template.name.clone(),
        description: template.description.clone().unwrap_or_default(),
        category: template.category.clone().unwrap_or_default(),
        tags: template.tags.clone().unwrap_or_default().join(", "),
        sql: template.sql.clone(),
        parameters: template.parameters.clone(),
        synced_sql: template.sql.clone(),
    }
}

fn template_form_dialog(
    h: &mut History,
    ctx: &egui::Context,
    palette: &crate::theme::Palette,
    intents: &mut Vec<Intent>,
) {
    let Some(form) = h.template_form.as_mut() else {
        return;
    };
    let editing = form.id.is_some();
    let mut open = true;
    let mut close = false;
    let mut saved: Option<QueryTemplate> = None;

    egui::Window::new(if editing { "Edit template" } else { "New template" })
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .default_height(520.0)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.label(crate::theme::text::muted(palette, "Name"));
                    ui.add(
                        egui::TextEdit::singleline(&mut form.name)
                            .hint_text("e.g. Find Users by Status")
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(6.0);

                    ui.label(crate::theme::text::muted(palette, "Description"));
                    ui.add(
                        egui::TextEdit::singleline(&mut form.description)
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(6.0);

                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(crate::theme::text::muted(palette, "Category"));
                            ui.add(
                                egui::TextEdit::singleline(&mut form.category)
                                    .desired_width(180.0),
                            );
                        });
                        ui.vertical(|ui| {
                            ui.label(crate::theme::text::muted(
                                palette,
                                "Tags (comma separated)",
                            ));
                            ui.add(
                                egui::TextEdit::singleline(&mut form.tags).desired_width(240.0),
                            );
                        });
                    });
                    ui.add_space(6.0);

                    ui.label(crate::theme::text::muted(
                        palette,
                        "SQL: use {{name}} or {{name:type}} for parameters",
                    ));
                    ui.add(
                        egui::TextEdit::multiline(&mut form.sql)
                            .code_editor()
                            .desired_rows(8)
                            .desired_width(f32::INFINITY),
                    );

                    // Two-way live sync: the parameter list follows the SQL,
                    // keeping whatever the user configured for parameters
                    // that survived the edit.
                    if form.sql != form.synced_sql {
                        form.parameters = sync_parameters(&form.sql, &form.parameters);
                        form.synced_sql = form.sql.clone();
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(crate::theme::text::strong(palette, "Parameters"));
                        if ui.small_button("Add").clicked() {
                            let n = form.parameters.len() + 1;
                            form.parameters.push(TemplateParameter {
                                name: format!("param_{}", n),
                                label: format!("Parameter {}", n),
                                param_type: ParamType::String,
                                default_value: None,
                                required: false,
                                description: None,
                            });
                        }
                    });

                    if form.parameters.is_empty() {
                        ui.label(crate::theme::text::muted(
                            palette,
                            "No parameters detected in the SQL.",
                        ));
                    }

                    let mut remove: Option<usize> = None;
                    for (idx, param) in form.parameters.iter_mut().enumerate() {
                        egui::Frame::none()
                            .fill(palette.muted)
                            .rounding(egui::Rounding::same(6.0))
                            .inner_margin(egui::Margin::same(8.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut param.name)
                                            .hint_text("name")
                                            .desired_width(140.0),
                                    );
                                    egui::ComboBox::from_id_salt(("param_type", idx))
                                        .selected_text(param.param_type.as_str())
                                        .width(110.0)
                                        .show_ui(ui, |ui| {
                                            for ty in ParamType::ALL {
                                                ui.selectable_value(
                                                    &mut param.param_type,
                                                    ty,
                                                    ty.as_str(),
                                                );
                                            }
                                        });
                                    ui.checkbox(&mut param.required, "Required");
                                    if ui.small_button("Remove").clicked() {
                                        remove = Some(idx);
                                    }
                                });
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut param.label)
                                            .hint_text("label")
                                            .desired_width(160.0),
                                    );
                                    let mut default =
                                        param.default_value.clone().unwrap_or_default();
                                    if ui
                                        .add(
                                            egui::TextEdit::singleline(&mut default)
                                                .hint_text("default value")
                                                .desired_width(160.0),
                                        )
                                        .changed()
                                    {
                                        param.default_value =
                                            if default.is_empty() { None } else { Some(default) };
                                    }
                                });
                                let mut desc = param.description.clone().unwrap_or_default();
                                if ui
                                    .add(
                                        egui::TextEdit::singleline(&mut desc)
                                            .hint_text("description")
                                            .desired_width(f32::INFINITY),
                                    )
                                    .changed()
                                {
                                    param.description =
                                        if desc.is_empty() { None } else { Some(desc) };
                                }
                            });
                        ui.add_space(4.0);
                    }
                    if let Some(idx) = remove {
                        form.parameters.remove(idx);
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let valid =
                            !form.name.trim().is_empty() && !form.sql.trim().is_empty();
                        if ui.add_enabled(valid, egui::Button::new("Save")).clicked() {
                            let tags = parse_tags(&form.tags);
                            let mut template = QueryTemplate::new(
                                form.name.trim().to_string(),
                                Some(form.description.trim().to_string())
                                    .filter(|d| !d.is_empty()),
                                form.sql.trim().to_string(),
                                form.parameters.clone(),
                                Some(form.category.trim().to_string())
                                    .filter(|c| !c.is_empty()),
                                if tags.is_empty() { None } else { Some(tags) },
                            );
                            if let Some(id) = &form.id {
                                template.id = id.clone();
                            }
                            saved = Some(template);
                            close = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                });
        });

    if let Some(template) = saved {
        match h.templates.iter_mut().find(|t| t.id == template.id) {
            Some(existing) => {
                // Preserve the original creation time on an edit.
                let created = existing.created_at.clone();
                *existing = QueryTemplate {
                    created_at: created,
                    ..template
                };
            }
            None => h.templates.push(template),
        }
        intents.push(Intent::SaveTemplates);
    }
    if close || !open {
        h.template_form = None;
    }
}

fn template_run_dialog(
    h: &mut History,
    ctx: &egui::Context,
    palette: &crate::theme::Palette,
    action: &mut HistoryAction,
    _intents: &mut [Intent],
) {
    let Some(run) = h.template_run.as_mut() else {
        return;
    };
    let mut open = true;
    let mut close = false;

    egui::Window::new(format!("Run: {}", run.template.name))
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for param in &run.template.parameters {
                        let label = if param.label.is_empty() {
                            param.name.clone()
                        } else {
                            param.label.clone()
                        };
                        ui.horizontal(|ui| {
                            ui.label(crate::theme::text::muted(palette, label));
                            if param.required {
                                ui.colored_label(palette.destructive, "*");
                            }
                        });
                        if let Some(desc) = param.description.as_deref() {
                            if !desc.is_empty() {
                                ui.label(crate::theme::text::muted(palette, desc));
                            }
                        }

                        let value = run.values.entry(param.name.clone()).or_default();
                        let changed = match param.param_type {
                            ParamType::Text => ui
                                .add(
                                    egui::TextEdit::multiline(value)
                                        .desired_rows(3)
                                        .desired_width(f32::INFINITY),
                                )
                                .changed(),
                            ParamType::Boolean => {
                                let mut picked = false;
                                egui::ComboBox::from_id_salt(("run_bool", &param.name))
                                    .selected_text(if value.is_empty() {
                                        "Select...".to_string()
                                    } else {
                                        value.clone()
                                    })
                                    .width(160.0)
                                    .show_ui(ui, |ui| {
                                        for option in ["true", "false"] {
                                            if ui
                                                .selectable_label(value == option, option)
                                                .clicked()
                                            {
                                                *value = option.to_string();
                                                picked = true;
                                            }
                                        }
                                    });
                                picked
                            }
                            ParamType::Date => ui
                                .add(
                                    egui::TextEdit::singleline(value)
                                        .hint_text("YYYY-MM-DD")
                                        .desired_width(f32::INFINITY),
                                )
                                .changed(),
                            ParamType::Datetime => ui
                                .add(
                                    egui::TextEdit::singleline(value)
                                        .hint_text("YYYY-MM-DD HH:MM:SS")
                                        .desired_width(f32::INFINITY),
                                )
                                .changed(),
                            _ => ui
                                .add(
                                    egui::TextEdit::singleline(value)
                                        .desired_width(f32::INFINITY),
                                )
                                .changed(),
                        };

                        if changed {
                            // Live per-field validation, exactly the web
                            // shell's behaviour: type errors appear as you
                            // type, "required" only on submit.
                            let current = run.values.get(&param.name).cloned().unwrap_or_default();
                            match validate_parameter_value(&current, param.param_type) {
                                Ok(()) => {
                                    run.errors.remove(&param.name);
                                }
                                Err(e) => {
                                    run.errors.insert(param.name.clone(), e);
                                }
                            }
                        }

                        if let Some(err) = run.errors.get(&param.name) {
                            ui.colored_label(
                                palette.destructive,
                                egui::RichText::new(err).size(11.0),
                            );
                        }
                        ui.add_space(6.0);
                    }

                    ui.separator();
                    ui.label(crate::theme::text::muted(palette, "Preview"));
                    egui::Frame::none()
                        .fill(palette.muted)
                        .rounding(egui::Rounding::same(4.0))
                        .inner_margin(egui::Margin::same(6.0))
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(preview_sql(&run.template, &run.values))
                                        .monospace()
                                        .size(11.0)
                                        .color(palette.foreground),
                                )
                                .wrap(),
                            );
                        });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Use SQL").clicked() {
                            let errors = validate_template_values(&run.template, &run.values);
                            if errors.is_empty() {
                                *action = HistoryAction::UseSql(render_template(
                                    &run.template,
                                    &run.values,
                                ));
                                close = true;
                            } else {
                                run.errors = errors;
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                });
        });

    if close || !open {
        h.template_run = None;
    }
}

fn import_dialog(
    h: &mut History,
    ctx: &egui::Context,
    palette: &crate::theme::Palette,
    intents: &mut Vec<Intent>,
) {
    if !h.show_import {
        return;
    }
    let mut open = true;
    egui::Window::new("Import templates")
        .collapsible(false)
        .resizable(true)
        .default_width(480.0)
        .open(&mut open)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(crate::theme::text::muted(
                palette,
                "Path to a templates JSON file. Templates whose name already exists are skipped.",
            ));
            ui.add(
                egui::TextEdit::singleline(&mut h.import_path)
                    .hint_text("/path/to/query-templates.json")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !h.import_path.trim().is_empty(),
                        egui::Button::new("Import"),
                    )
                    .clicked()
                {
                    intents.push(Intent::ImportTemplates(h.import_path.clone()));
                }
                if ui.button("Cancel").clicked() {
                    h.show_import = false;
                }
            });
        });
    if !open {
        h.show_import = false;
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_limits_are_the_four_the_web_shell_offered() {
        assert_eq!(HISTORY_LIMITS, [25, 50, 100, 500]);
        assert!(HISTORY_LIMITS.contains(&History::default().limit));
    }
}
