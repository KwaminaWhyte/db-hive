//! Controller glue for the History / Snippets / Templates panel.
//!
//! A child module of `app` so it can use the controller's private pieces
//! (`Ctx`, `Session`, `run_tab`, `with_ctx`) without widening their
//! visibility. The panel's logic lives in `dbhive_ui_model::history_panel` and
//! `dbhive_ui_model::history`; this file only moves values between that logic,
//! the stores in `AppState`, and the `HistoryState` global in the window.

use std::rc::Rc;

use dbhive_core::state::AppState;
use dbhive_ui_model::history::{
    export_templates_now, import_templates_now, load_templates,
    merge_imported, save_templates, sync_parameters, template_categories,
    QueryTemplate, TemplateParameter, HISTORY_LIMITS,
};
use dbhive_ui_model::history_panel::{
    append_sql, build_snippet, build_template, duplicate_template, history_rows, rerun_plan,
    scoped_history, set_param_default, snippet_rows, template_rows, toggle_param_required,
    PanelTab, RerunPlan, SnippetDraft, TemplateDraft, TemplateRun,
};
use slint::ComponentHandle;

use super::{with_ctx, Ctx, PendingAction};
use crate::history_view::{count_label, form_params, rows_model, run_params, strings_model};
use crate::{runtime, sql_highlight};
use crate::{HistoryState, PanelMode, Screen, Theme, Tone};

/// A confirmation the panel is waiting on.
pub(super) enum Pending {
    ClearHistory,
    DeleteSnippet(String),
    DeleteTemplate(String),
}

/// Which editor form is open, so Save and SQL edits go to the right place.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FormKind {
    None,
    Snippet,
    Template,
}

/// The panel's own state. Search text lives in the `HistoryState` global
/// (the fields are bound to it), everything else is here.
pub(super) struct PanelSession {
    tab: PanelTab,
    form: FormKind,
    scoped: bool,
    limit_index: usize,
    snippet_tag: Option<String>,
    template_category: Option<String>,

    templates: Vec<QueryTemplate>,
    templates_loading: bool,
    templates_loaded: bool,
    /// Why `templates.json` could not be read; retry-able.
    templates_load_error: String,
    history_error: String,
    snippets_error: String,
    templates_error: String,

    editing_snippet: Option<String>,
    editing_template: Option<String>,
    draft_params: Vec<TemplateParameter>,
    run: Option<TemplateRun>,
}

impl Default for PanelSession {
    fn default() -> Self {
        Self {
            tab: PanelTab::History,
            form: FormKind::None,
            scoped: true,
            limit_index: 1,
            snippet_tag: None,
            template_category: None,
            templates: Vec::new(),
            templates_loading: false,
            templates_loaded: false,
            templates_load_error: String::new(),
            history_error: String::new(),
            snippets_error: String::new(),
            templates_error: String::new(),
            editing_snippet: None,
            editing_template: None,
            draft_params: Vec::new(),
            run: None,
        }
    }
}

fn index_of(options: &[String], picked: &Option<String>) -> i32 {
    picked
        .as_ref()
        .and_then(|p| options.iter().position(|o| o == p))
        .map_or(-1, |i| i as i32)
}

impl Ctx {
    // -- Projection ---------------------------------------------------------

    /// Project the panel's state into the `HistoryState` global.
    pub(super) fn refresh_history_panel(&self) {
        let Some(w) = self.window.upgrade() else { return };
        let h = w.global::<HistoryState>();

        // Copy out of the lock first; nothing below holds it.
        let (all_history, all_snippets) = {
            let g = self.state.lock().unwrap();
            (g.get_all_history(), g.get_all_snippets())
        };
        let dark = w.global::<Theme>().get_dark();
        let now = chrono::Utc::now();

        let mut s = self.session.borrow_mut();
        let ws_connection = s.workspace.connection_id.clone();
        let ws_database = s.workspace.database.clone();
        let scope_label = if ws_connection.is_empty() {
            String::new()
        } else {
            match &ws_database {
                Some(db) if !db.is_empty() => format!("{} / {}", s.workspace.connection_name, db),
                _ => s.workspace.connection_name.clone(),
            }
        };
        let p = &mut s.history;

        // A tag or category that no longer exists must not keep filtering.
        let snippet_tags = dbhive_ui_model::history::snippet_tags(&all_snippets);
        if p.snippet_tag.as_ref().is_some_and(|t| !snippet_tags.contains(t)) {
            p.snippet_tag = None;
        }
        let categories = template_categories(&p.templates);
        if p.template_category.as_ref().is_some_and(|c| !categories.contains(c)) {
            p.template_category = None;
        }

        // History.
        let limit = HISTORY_LIMITS[p.limit_index.min(HISTORY_LIMITS.len() - 1)];
        let scope = (p.scoped && !ws_connection.is_empty())
            .then_some((ws_connection.as_str(), ws_database.as_deref()));
        let entries = scoped_history(&all_history, scope, limit);
        let history_count = entries.len();
        h.set_history(rows_model(history_rows(&entries, now)));
        h.set_history_summary(
            format!("{}, newest first", count_label(history_count, "query", "queries")).into(),
        );
        h.set_scoped(p.scoped && !ws_connection.is_empty());
        h.set_scope_label(scope_label.into());
        h.set_limit_value(limit.to_string().into());
        h.set_limit_options(strings_model(
            &HISTORY_LIMITS.iter().map(|n| n.to_string()).collect::<Vec<_>>(),
        ));

        // Snippets.
        let snippet_search = h.get_snippet_search().to_string();
        let rows = snippet_rows(&all_snippets, &snippet_search, p.snippet_tag.as_deref());
        let snippet_shown = rows.len();
        h.set_snippets(rows_model(rows));
        h.set_snippet_tags(strings_model(&snippet_tags));
        h.set_snippet_tag(index_of(&snippet_tags, &p.snippet_tag));
        h.set_snippet_summary(
            format!(
                "{} of {}",
                snippet_shown,
                count_label(all_snippets.len(), "snippet", "snippets")
            )
            .into(),
        );

        // Templates.
        let template_search = h.get_template_search().to_string();
        let rows = template_rows(&p.templates, &template_search, p.template_category.as_deref());
        let template_shown = rows.len();
        h.set_templates(rows_model(rows));
        h.set_template_categories(strings_model(&categories));
        h.set_template_category(index_of(&categories, &p.template_category));
        h.set_template_summary(
            format!(
                "{} of {}",
                template_shown,
                count_label(p.templates.len(), "template", "templates")
            )
            .into(),
        );

        h.set_tab_labels(strings_model(&[
            "History".to_string(),
            if all_snippets.is_empty() {
                "Snippets".to_string()
            } else {
                format!("Snippets ({})", all_snippets.len())
            },
            if p.templates.is_empty() {
                "Templates".to_string()
            } else {
                format!("Templates ({})", p.templates.len())
            },
        ]));
        h.set_tab(p.tab.index());

        // The list-level loading and error state follows the visible tab.
        h.set_loading(p.tab == PanelTab::Templates && p.templates_loading);
        let (error, retry) = match p.tab {
            PanelTab::History => (p.history_error.clone(), false),
            PanelTab::Snippets => (p.snippets_error.clone(), false),
            PanelTab::Templates if !p.templates_load_error.is_empty() => {
                (p.templates_load_error.clone(), true)
            }
            PanelTab::Templates => (p.templates_error.clone(), false),
        };
        h.set_error(error.into());
        h.set_error_retry(retry);

        // Editor and run form.
        h.set_form_params(form_params(&p.draft_params));
        if let Some(run) = &p.run {
            h.set_run_title(format!("Run: {}", run.template().name).into());
            h.set_run_errors(strings_model(&run.errors()));
            h.set_run_preview(run.preview().into());
        }
        h.set_form_lines(sql_highlight::to_model_for(&h.get_form_sql(), dark));
    }

    fn set_panel_mode(&self, mode: PanelMode) {
        if let Some(w) = self.window.upgrade() {
            w.global::<HistoryState>().set_mode(mode);
        }
    }

    // -- Open and close -----------------------------------------------------

    pub(super) fn open_history_panel(self: &Rc<Self>) {
        let Some(w) = self.window.upgrade() else { return };
        if w.get_screen() != Screen::Workspace {
            self.toast("Open a connection to see its history.", Tone::Neutral);
            return;
        }
        {
            let mut s = self.session.borrow_mut();
            s.history.history_error.clear();
            s.history.snippets_error.clear();
            s.history.templates_error.clear();
            s.history.run = None;
        }
        self.set_panel_mode(PanelMode::Lists);
        w.global::<HistoryState>().set_visible(true);
        self.refresh_history_panel();
        if !self.session.borrow().history.templates_loaded {
            self.load_panel_templates();
        }
    }

    pub(super) fn hide_history_panel(&self) {
        if let Some(w) = self.window.upgrade() {
            w.global::<HistoryState>().set_visible(false);
        }
    }

    fn select_panel_tab(&self, index: i32) {
        self.session.borrow_mut().history.tab = PanelTab::from_index(index);
        self.refresh_history_panel();
    }

    /// Read `templates.json` off the UI thread.
    fn load_panel_templates(self: &Rc<Self>) {
        {
            let mut s = self.session.borrow_mut();
            s.history.templates_loading = true;
            s.history.templates_load_error.clear();
        }
        self.refresh_history_panel();
        runtime::spawn(async move { load_templates() }, |result| {
            with_ctx(|ctx| {
                {
                    let mut s = ctx.session.borrow_mut();
                    s.history.templates_loading = false;
                    match result {
                        Ok(list) => {
                            s.history.templates = list;
                            s.history.templates_loaded = true;
                        }
                        Err(e) => s.history.templates_load_error = e,
                    }
                }
                ctx.refresh_history_panel();
            })
        });
    }

    fn retry_panel(self: &Rc<Self>) {
        let tab = self.session.borrow().history.tab;
        if tab == PanelTab::Templates {
            self.load_panel_templates();
        }
    }

    // -- Tabs that hold SQL -------------------------------------------------

    /// A new query tab holding `sql`, made active. Returns its id.
    pub(super) fn open_query_tab_with(&self, sql: &str) -> u64 {
        let id = self.session.borrow_mut().workspace.tabs.open_query(sql);
        self.refresh_workspace();
        id
    }

    /// Add `sql` to the active query tab (after a blank line, or replacing a
    /// blank editor). With a table preview active there is no editor to put
    /// it in, so it opens in a new query tab.
    pub(super) fn insert_into_active_query(&self, sql: &str) {
        let appended = {
            let mut s = self.session.borrow_mut();
            match s.workspace.tabs.active_mut() {
                Some(tab) if tab.is_query() => {
                    tab.sql = append_sql(&tab.sql, sql);
                    true
                }
                _ => false,
            }
        };
        if appended {
            self.refresh_workspace();
        } else {
            self.open_query_tab_with(sql);
        }
    }

    /// Open `sql` in a new tab and run it through the normal run path, so the
    /// destructive-statement guard still applies.
    fn run_in_new_tab(self: &Rc<Self>, sql: &str) {
        let id = self.open_query_tab_with(sql);
        self.run_tab(id, true);
    }

    // -- History --------------------------------------------------------------

    fn history_entry(&self, id: &str) -> Option<dbhive_core::models::QueryHistory> {
        let g = self.state.lock().unwrap();
        g.query_history.iter().find(|h| h.id == id).cloned()
    }

    fn history_open(&self, id: &str) {
        let Some(entry) = self.history_entry(id) else { return };
        self.open_query_tab_with(&entry.query);
        self.hide_history_panel();
    }

    fn history_rerun(self: &Rc<Self>, id: &str) {
        let Some(entry) = self.history_entry(id) else { return };
        let current = self.session.borrow().workspace.connection_id.clone();
        match rerun_plan(&entry, &current) {
            RerunPlan::RunHere => {
                self.hide_history_panel();
                self.run_in_new_tab(&entry.query);
            }
            RerunPlan::OpenOnly(reason) => {
                self.open_query_tab_with(&entry.query);
                self.hide_history_panel();
                self.toast(reason, Tone::Warning);
            }
        }
    }

    fn history_clear_asked(&self) {
        let scoped = {
            let s = self.session.borrow();
            s.history.scoped && !s.workspace.connection_id.is_empty()
        };
        let (message, label) = if scoped {
            let name = self.session.borrow().workspace.connection_name.clone();
            (format!("Every saved query for \"{name}\" will be removed. This cannot be undone."), "Clear history")
        } else {
            ("Every saved query on every connection will be removed. This cannot be undone.".to_string(), "Clear all")
        };
        self.session.borrow_mut().pending_action =
            Some(PendingAction::History(Pending::ClearHistory));
        self.show_confirm("Clear query history?", &message, label);
    }

    fn history_clear_confirmed(self: &Rc<Self>) {
        let (scoped, connection) = {
            let s = self.session.borrow();
            (s.history.scoped, s.workspace.connection_id.clone())
        };
        let snapshot = {
            let mut g = self.state.lock().unwrap();
            if scoped && !connection.is_empty() {
                g.clear_history_by_connection(&connection);
            } else {
                g.clear_all_history();
            }
            g.query_history.clone()
        };
        self.session.borrow_mut().history.history_error.clear();
        self.refresh_history_panel();
        runtime::spawn(
            async move { AppState::save_history_to_store(&snapshot).map_err(|e| e.to_string()) },
            |saved| {
                with_ctx(|ctx| match saved {
                    Ok(()) => ctx.toast("Query history cleared", Tone::Success),
                    Err(e) => {
                        ctx.session.borrow_mut().history.history_error =
                            format!("Cleared, but the change could not be saved: {e}");
                        ctx.refresh_history_panel();
                    }
                })
            },
        );
    }

    // -- Snippets ---------------------------------------------------------------

    fn persist_snippets(self: &Rc<Self>) {
        let state = self.state.clone();
        runtime::spawn(
            async move {
                let guard = state.lock().unwrap();
                guard.save_snippets_to_store().map_err(|e| e.to_string())
            },
            |saved| {
                with_ctx(|ctx| {
                    ctx.session.borrow_mut().history.snippets_error = match saved {
                        Ok(()) => String::new(),
                        Err(e) => format!("Could not save snippets: {e}"),
                    };
                    ctx.refresh_history_panel();
                })
            },
        );
    }

    fn snippet_insert(self: &Rc<Self>, id: &str) {
        let snippet = self.state.lock().unwrap().get_snippet(id).cloned();
        let Some(snippet) = snippet else { return };
        self.insert_into_active_query(&snippet.query);
        self.hide_history_panel();
        self.toast(format!("Inserted {}", snippet.name), Tone::Success);
    }

    fn load_form(&self, name: &str, description: &str, category: &str, tags: &str, sql: &str) {
        let Some(w) = self.window.upgrade() else { return };
        let h = w.global::<HistoryState>();
        h.set_form_name(name.into());
        h.set_form_description(description.into());
        h.set_form_category(category.into());
        h.set_form_tags(tags.into());
        h.set_form_sql(sql.into());
        h.set_form_error("".into());
    }

    fn snippet_new(&self) {
        {
            let mut s = self.session.borrow_mut();
            s.history.editing_snippet = None;
            s.history.form = FormKind::Snippet;
        }
        self.load_form("", "", "", "", "");
        if let Some(w) = self.window.upgrade() {
            w.global::<HistoryState>().set_form_is_new(true);
        }
        self.set_panel_mode(PanelMode::SnippetForm);
        self.refresh_history_panel();
    }

    fn snippet_edit(&self, id: &str) {
        let snippet = self.state.lock().unwrap().get_snippet(id).cloned();
        let Some(snippet) = snippet else { return };
        let d = SnippetDraft::from_snippet(&snippet);
        {
            let mut s = self.session.borrow_mut();
            s.history.editing_snippet = Some(snippet.id.clone());
            s.history.form = FormKind::Snippet;
        }
        self.load_form(&d.name, &d.description, "", &d.tags, &d.query);
        if let Some(w) = self.window.upgrade() {
            w.global::<HistoryState>().set_form_is_new(false);
        }
        self.set_panel_mode(PanelMode::SnippetForm);
        self.refresh_history_panel();
    }

    fn snippet_save(self: &Rc<Self>) {
        let Some(w) = self.window.upgrade() else { return };
        let h = w.global::<HistoryState>();
        let draft = SnippetDraft {
            name: h.get_form_name().to_string(),
            description: h.get_form_description().to_string(),
            query: h.get_form_sql().to_string(),
            tags: h.get_form_tags().to_string(),
        };
        let editing = self.session.borrow().history.editing_snippet.clone();
        let existing = editing
            .as_deref()
            .and_then(|id| self.state.lock().unwrap().get_snippet(id).cloned());
        match build_snippet(&draft, existing.as_ref()) {
            Err(message) => h.set_form_error(message.into()),
            Ok(snippet) => {
                let name = snippet.name.clone();
                self.state.lock().unwrap().add_snippet(snippet);
                self.session.borrow_mut().history.tab = PanelTab::Snippets;
                self.set_panel_mode(PanelMode::Lists);
                self.refresh_history_panel();
                self.persist_snippets();
                self.toast(format!("Saved snippet {name}"), Tone::Success);
            }
        }
    }

    fn snippet_delete_asked(&self, id: &str) {
        let Some(snippet) = self.state.lock().unwrap().get_snippet(id).cloned() else { return };
        self.session.borrow_mut().pending_action =
            Some(PendingAction::History(Pending::DeleteSnippet(id.to_string())));
        self.show_confirm(
            "Delete snippet?",
            &format!("\"{}\" will be removed. This cannot be undone.", snippet.name),
            "Delete",
        );
    }

    fn snippet_delete_confirmed(self: &Rc<Self>, id: &str) {
        let removed = self.state.lock().unwrap().remove_snippet(id).is_some();
        if removed {
            self.refresh_history_panel();
            self.persist_snippets();
            self.toast("Snippet deleted", Tone::Neutral);
        } else {
            self.toast("That snippet no longer exists", Tone::Danger);
        }
    }

    // -- Templates --------------------------------------------------------------

    fn persist_templates(self: &Rc<Self>) {
        let snapshot = self.session.borrow().history.templates.clone();
        runtime::spawn(async move { save_templates(&snapshot) }, |saved| {
            with_ctx(|ctx| {
                ctx.session.borrow_mut().history.templates_error = match saved {
                    Ok(()) => String::new(),
                    Err(e) => format!("Could not save templates: {e}"),
                };
                ctx.refresh_history_panel();
            })
        });
    }

    fn find_template(&self, id: &str) -> Option<QueryTemplate> {
        self.session.borrow().history.templates.iter().find(|t| t.id == id).cloned()
    }

    fn template_run_asked(self: &Rc<Self>, id: &str) {
        let Some(template) = self.find_template(id) else { return };
        if template.parameters.is_empty() {
            self.hide_history_panel();
            self.run_in_new_tab(&template.sql);
            return;
        }
        let run = TemplateRun::new(&template);
        if let Some(w) = self.window.upgrade() {
            let h = w.global::<HistoryState>();
            h.set_run_params(run_params(run.fields()));
            h.set_form_error("".into());
        }
        self.session.borrow_mut().history.run = Some(run);
        self.set_panel_mode(PanelMode::TemplateRun);
        self.refresh_history_panel();
    }

    fn template_run_edited(&self, index: usize, value: &str) {
        if let Some(run) = self.session.borrow_mut().history.run.as_mut() {
            run.set_value(index, value);
        }
        self.refresh_history_panel();
    }

    /// Validate the run form; on success hand back the statement and close
    /// the panel.
    fn template_run_statement(&self) -> Option<String> {
        let statement = self
            .session
            .borrow_mut()
            .history
            .run
            .as_mut()
            .map(|r| r.submit())?;
        match statement {
            Ok(sql) => {
                self.session.borrow_mut().history.run = None;
                self.hide_history_panel();
                Some(sql)
            }
            Err(()) => {
                if let Some(w) = self.window.upgrade() {
                    w.global::<HistoryState>().set_form_error("Fix the highlighted fields first.".into());
                }
                self.refresh_history_panel();
                None
            }
        }
    }

    fn template_form_new(&self) {
        {
            let mut s = self.session.borrow_mut();
            s.history.editing_template = None;
            s.history.draft_params.clear();
            s.history.form = FormKind::Template;
        }
        self.load_form("", "", "", "", "");
        if let Some(w) = self.window.upgrade() {
            w.global::<HistoryState>().set_form_is_new(true);
        }
        self.set_panel_mode(PanelMode::TemplateForm);
        self.refresh_history_panel();
    }

    fn template_form_edit(&self, id: &str) {
        let Some(t) = self.find_template(id) else { return };
        let d = TemplateDraft::from_template(&t);
        {
            let mut s = self.session.borrow_mut();
            s.history.editing_template = Some(t.id.clone());
            s.history.draft_params = t.parameters.clone();
            s.history.form = FormKind::Template;
        }
        self.load_form(&d.name, &d.description, &d.category, &d.tags, &d.sql);
        if let Some(w) = self.window.upgrade() {
            w.global::<HistoryState>().set_form_is_new(false);
        }
        self.set_panel_mode(PanelMode::TemplateForm);
        self.refresh_history_panel();
    }

    /// The SQL box changed: the parameter list follows the placeholders in it
    /// (a template form), and the highlighting is recomputed.
    fn form_sql_edited(&self, sql: &str) {
        {
            let mut s = self.session.borrow_mut();
            if s.history.form == FormKind::Template {
                let kept = std::mem::take(&mut s.history.draft_params);
                s.history.draft_params = sync_parameters(sql, &kept);
            }
        }
        self.refresh_history_panel();
    }

    fn template_save(self: &Rc<Self>) {
        let Some(w) = self.window.upgrade() else { return };
        let h = w.global::<HistoryState>();
        let draft = TemplateDraft {
            name: h.get_form_name().to_string(),
            description: h.get_form_description().to_string(),
            category: h.get_form_category().to_string(),
            tags: h.get_form_tags().to_string(),
            sql: h.get_form_sql().to_string(),
        };
        let (editing, params) = {
            let s = self.session.borrow();
            (s.history.editing_template.clone(), s.history.draft_params.clone())
        };
        let existing = editing.as_deref().and_then(|id| self.find_template(id));
        match build_template(&draft, &params, existing.as_ref()) {
            Err(message) => h.set_form_error(message.into()),
            Ok(template) => {
                let name = template.name.clone();
                {
                    let mut s = self.session.borrow_mut();
                    match s.history.templates.iter_mut().find(|t| t.id == template.id) {
                        Some(slot) => *slot = template,
                        None => s.history.templates.push(template),
                    }
                    s.history.tab = PanelTab::Templates;
                    s.history.draft_params.clear();
                }
                self.set_panel_mode(PanelMode::Lists);
                self.refresh_history_panel();
                self.persist_templates();
                self.toast(format!("Saved template {name}"), Tone::Success);
            }
        }
    }

    fn template_duplicate(self: &Rc<Self>, id: &str) {
        let Some(source) = self.find_template(id) else { return };
        let copy = {
            let s = self.session.borrow();
            duplicate_template(&source, &s.history.templates)
        };
        let name = copy.name.clone();
        self.session.borrow_mut().history.templates.push(copy);
        self.refresh_history_panel();
        self.persist_templates();
        self.toast(format!("Created {name}"), Tone::Success);
    }

    fn template_delete_asked(&self, id: &str) {
        let Some(t) = self.find_template(id) else { return };
        self.session.borrow_mut().pending_action =
            Some(PendingAction::History(Pending::DeleteTemplate(id.to_string())));
        self.show_confirm(
            "Delete template?",
            &format!("\"{}\" will be removed. This cannot be undone.", t.name),
            "Delete",
        );
    }

    fn template_delete_confirmed(self: &Rc<Self>, id: &str) {
        self.session.borrow_mut().history.templates.retain(|t| t.id != id);
        self.refresh_history_panel();
        self.persist_templates();
        self.toast("Template deleted", Tone::Neutral);
    }

    fn template_export(self: &Rc<Self>) {
        let snapshot = self.session.borrow().history.templates.clone();
        runtime::spawn(async move { export_templates_now(&snapshot) }, |result| {
            with_ctx(|ctx| match result {
                Ok(path) => ctx.toast(format!("Exported to {}", path.display()), Tone::Success),
                Err(e) => ctx.toast(format!("Export failed: {e}"), Tone::Danger),
            })
        });
    }

    fn template_import_open(&self) {
        if let Some(w) = self.window.upgrade() {
            let h = w.global::<HistoryState>();
            h.set_import_path("".into());
            h.set_import_error("".into());
        }
        self.set_panel_mode(PanelMode::TemplateImport);
    }

    fn template_import(self: &Rc<Self>, path: String) {
        let path = path.trim().to_string();
        runtime::spawn(async move { import_templates_now(&path) }, |result| {
            with_ctx(|ctx| match result {
                Err(e) => {
                    if let Some(w) = ctx.window.upgrade() {
                        w.global::<HistoryState>().set_import_error(e.into());
                    }
                }
                Ok(incoming) => {
                    let added = {
                        let s = ctx.session.borrow();
                        merge_imported(&s.history.templates, incoming)
                    };
                    let count = added.len();
                    if count > 0 {
                        ctx.session.borrow_mut().history.templates.extend(added);
                        ctx.persist_templates();
                    }
                    ctx.session.borrow_mut().history.tab = PanelTab::Templates;
                    ctx.set_panel_mode(PanelMode::Lists);
                    ctx.refresh_history_panel();
                    ctx.toast(
                        format!("Imported {}", count_label(count, "template", "templates")),
                        Tone::Success,
                    );
                }
            })
        });
    }

    // -- Confirmations ----------------------------------------------------------

    pub(super) fn history_confirmed(self: &Rc<Self>, pending: Pending) {
        match pending {
            Pending::ClearHistory => self.history_clear_confirmed(),
            Pending::DeleteSnippet(id) => self.snippet_delete_confirmed(&id),
            Pending::DeleteTemplate(id) => self.template_delete_confirmed(&id),
        }
    }
}

/// Connect every `HistoryState` callback to the controller.
pub(super) fn wire(window: &crate::AppWindow, ctx: &Rc<Ctx>) {
    let h = window.global::<HistoryState>();
    h.set_limit_options(strings_model(
        &HISTORY_LIMITS.iter().map(|n| n.to_string()).collect::<Vec<_>>(),
    ));

    macro_rules! on {
        ($setter:ident, |$c:ident $(, $a:ident)*| $body:expr) => {{
            let $c = ctx.clone();
            h.$setter(move |$($a),*| {
                let $c = &$c;
                $body
            });
        }};
    }

    on!(on_open_requested, |c| c.open_history_panel());
    on!(on_close, |c| c.hide_history_panel());
    on!(on_retry, |c| c.retry_panel());
    on!(on_tab_selected, |c, i| c.select_panel_tab(i));

    on!(on_refresh, |c| c.refresh_history_panel());
    on!(on_scope_toggled, |c| {
        {
            let mut s = c.session.borrow_mut();
            s.history.scoped = !s.history.scoped;
        }
        c.refresh_history_panel();
    });
    on!(on_limit_selected, |c, i| {
        c.session.borrow_mut().history.limit_index = (i.max(0) as usize).min(HISTORY_LIMITS.len() - 1);
        c.refresh_history_panel();
    });
    on!(on_history_open, |c, id| c.history_open(&id));
    on!(on_history_rerun, |c, id| c.history_rerun(&id));
    on!(on_history_clear, |c| c.history_clear_asked());

    on!(on_snippet_search_edited, |c, _t| c.refresh_history_panel());
    on!(on_snippet_tag_picked, |c, i| {
        let tags = dbhive_ui_model::history::snippet_tags(&c.state.lock().unwrap().get_all_snippets());
        c.session.borrow_mut().history.snippet_tag =
            usize::try_from(i).ok().and_then(|i| tags.get(i).cloned());
        c.refresh_history_panel();
    });
    on!(on_snippet_insert, |c, id| c.snippet_insert(&id));
    on!(on_snippet_new, |c| c.snippet_new());
    on!(on_snippet_edit, |c, id| c.snippet_edit(&id));
    on!(on_snippet_delete, |c, id| c.snippet_delete_asked(&id));

    on!(on_template_search_edited, |c, _t| c.refresh_history_panel());
    on!(on_template_category_picked, |c, i| {
        let cats = template_categories(&c.session.borrow().history.templates);
        c.session.borrow_mut().history.template_category =
            usize::try_from(i).ok().and_then(|i| cats.get(i).cloned());
        c.refresh_history_panel();
    });
    on!(on_template_run, |c, id| c.template_run_asked(&id));
    on!(on_template_new, |c| c.template_form_new());
    on!(on_template_edit, |c, id| c.template_form_edit(&id));
    on!(on_template_duplicate, |c, id| c.template_duplicate(&id));
    on!(on_template_delete, |c, id| c.template_delete_asked(&id));
    on!(on_template_export, |c| c.template_export());
    on!(on_template_import_open, |c| c.template_import_open());
    on!(on_template_import, |c, path| c.template_import(path.to_string()));

    on!(on_form_save, |c| {
        let template_form = c.session.borrow().history.form == FormKind::Template;
        if template_form {
            c.template_save();
        } else {
            c.snippet_save();
        }
    });
    on!(on_form_cancel, |c| {
        c.session.borrow_mut().history.run = None;
        c.set_panel_mode(PanelMode::Lists);
        c.refresh_history_panel();
    });
    on!(on_form_sql_edited, |c, t| c.form_sql_edited(&t));
    on!(on_form_param_required, |c, i| {
        toggle_param_required(&mut c.session.borrow_mut().history.draft_params, i.max(0) as usize);
        c.refresh_history_panel();
    });
    on!(on_form_param_default, |c, i, t| {
        // No refresh: rebuilding the rows would take focus from the field
        // being typed in. The draft is read at save time.
        set_param_default(&mut c.session.borrow_mut().history.draft_params, i.max(0) as usize, &t);
    });

    on!(on_run_edited, |c, i, t| c.template_run_edited(i.max(0) as usize, &t));
    on!(on_run_open, |c| {
        if let Some(sql) = c.template_run_statement() {
            c.open_query_tab_with(&sql);
        }
    });
    on!(on_run_submit, |c| {
        if let Some(sql) = c.template_run_statement() {
            c.run_in_new_tab(&sql);
        }
    });
    on!(on_run_cancel, |c| {
        c.session.borrow_mut().history.run = None;
        c.set_panel_mode(PanelMode::Lists);
        c.refresh_history_panel();
    });
}
