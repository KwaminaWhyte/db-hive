//! Controller glue for the create-table wizard and the edit-table dialog.
//!
//! A child module of `app` so it can use the controller's private pieces
//! (`Ctx`, `Session`, `with_ctx`). The dialogs' logic lives in
//! `dbhive_ui_model::table_dialog`; this file moves values between that logic,
//! the open connection's driver, and the `TableDialogState` global.
//!
//! Every statement comes from `dbhive_core::ddl`, which escapes identifiers.
//! They run one after another and stop at the first failure, and the dialog
//! reports each statement's outcome rather than a single pass or fail.

use std::rc::Rc;

use dbhive_core::ddl::get_ddl_generator;
use dbhive_core::models::DbDriver;
use dbhive_ui_model::builder_editor::Change;
use dbhive_ui_model::table_dialog::{
    report, report_ok, AlterEditor, CreateWizard, DialogView, RefColumns, StatementReport,
};
use dbhive_ui_model::table_view::sql_qualifier;
use dbhive_ui_model::workspace_tabs::TabKind;
use slint::ComponentHandle;

use super::{with_ctx, Ctx, PendingAction};
use crate::{runtime, table_dialog_view, AppWindow, TableDialogState, Theme, Tone};

pub(super) enum Dialog {
    Create(CreateWizard),
    Edit(AlterEditor),
}

/// The dialog's own state: what is open, and what the connection offers it.
#[derive(Default)]
pub(super) struct DialogSession {
    dialog: Option<Dialog>,
    driver: Option<DbDriver>,
    /// What the driver's metadata calls take: a schema, or the database.
    schema_arg: String,
    /// The table being edited.
    table: String,
    /// Tables a foreign key may point at.
    fk_tables: Vec<String>,
    loading: bool,
    load_error: String,
    running: bool,
    report: Vec<StatementReport>,
    /// Supersedes work still in flight when the dialog is reopened.
    latest: runtime::Latest,
}

fn is_view(table_type: &str) -> bool {
    table_type.to_uppercase().contains("VIEW")
}

impl Ctx {
    // -- Open and close -----------------------------------------------------------

    /// Common start: the connection, its driver and schema. `None` (after a
    /// toast saying why) when a dialog cannot open.
    fn table_dialog_target(self: &Rc<Self>) -> Option<(String, DbDriver, String)> {
        let s = self.session.borrow();
        let ws = &s.workspace;
        let Some(driver) = ws.driver.clone().filter(|_| !ws.connection_id.is_empty()) else {
            drop(s);
            self.toast("Open a connection first.", Tone::Neutral);
            return None;
        };
        if get_ddl_generator(&driver).is_err() {
            drop(s);
            self.toast(
                "Creating and editing tables is not supported for this database.",
                Tone::Warning,
            );
            return None;
        }
        if ws.schema.is_none() {
            drop(s);
            self.toast(
                "Tables are still loading. Try again in a moment.",
                Tone::Neutral,
            );
            return None;
        }
        Some((
            ws.connection_id.clone(),
            driver,
            ws.schema.clone().unwrap_or_default(),
        ))
    }

    fn table_dialog_fk_candidates(&self) -> Vec<String> {
        self.session
            .borrow()
            .workspace
            .tables
            .iter()
            .filter(|t| !is_view(&t.table_type))
            .map(|t| t.name.clone())
            .collect()
    }

    pub(super) fn table_dialog_open_create(self: &Rc<Self>) {
        let Some((_, driver, schema_arg)) = self.table_dialog_target() else {
            return;
        };
        let qualifier = sql_qualifier(&driver, &schema_arg);
        let fk_tables = self.table_dialog_fk_candidates();
        {
            let mut s = self.session.borrow_mut();
            let latest = s.table_dialog.latest.clone();
            latest.begin();
            s.table_dialog = DialogSession {
                dialog: Some(Dialog::Create(CreateWizard::new(&qualifier))),
                driver: Some(driver),
                schema_arg,
                fk_tables,
                latest,
                ..DialogSession::default()
            };
        }
        self.table_dialog_show();
        self.table_dialog_refresh(Change::Structure);
    }

    /// "Edit table" on a table tab.
    pub(super) fn table_dialog_open_edit_active(self: &Rc<Self>) {
        let name = {
            let s = self.session.borrow();
            match s.workspace.tabs.active().map(|t| t.kind.clone()) {
                Some(TabKind::Table(name)) => Some(name),
                _ => None,
            }
        };
        match name {
            Some(name) => self.table_dialog_open_edit(&name),
            None => self.toast("Open a table tab to edit it.", Tone::Neutral),
        }
    }

    fn table_dialog_open_edit(self: &Rc<Self>, table: &str) {
        let Some((_, driver, schema_arg)) = self.table_dialog_target() else {
            return;
        };
        let viewing = self
            .session
            .borrow()
            .workspace
            .tables
            .iter()
            .any(|t| t.name == table && is_view(&t.table_type));
        if viewing {
            self.toast("A view cannot be edited here.", Tone::Warning);
            return;
        }
        {
            let mut s = self.session.borrow_mut();
            let latest = s.table_dialog.latest.clone();
            latest.begin();
            s.table_dialog = DialogSession {
                driver: Some(driver),
                schema_arg,
                table: table.to_string(),
                latest,
                ..DialogSession::default()
            };
        }
        self.table_dialog_show();
        self.table_dialog_load_edit(false);
    }

    fn table_dialog_show(&self) {
        if let Some(w) = self.window.upgrade() {
            w.global::<TableDialogState>().set_visible(true);
        }
    }

    pub(super) fn table_dialog_close(&self) {
        {
            let mut s = self.session.borrow_mut();
            s.table_dialog.latest.begin();
            s.table_dialog.dialog = None;
            s.table_dialog.loading = false;
            s.table_dialog.report.clear();
        }
        if let Some(w) = self.window.upgrade() {
            w.global::<TableDialogState>().set_visible(false);
        }
    }

    pub(super) fn table_dialog_open_now(&self) -> bool {
        self.window
            .upgrade()
            .is_some_and(|w| w.global::<TableDialogState>().get_visible())
    }

    // -- Loading the table to edit ------------------------------------------------

    /// Read the live table. With `keep_report`, the editor is rebuilt from
    /// what the database now holds (after a run) and the run's report stays.
    fn table_dialog_load_edit(self: &Rc<Self>, keep_report: bool) {
        let (connection_id, schema_arg, table, qualifier, ticket) = {
            let mut guard = self.session.borrow_mut();
            let s = &mut *guard;
            let d = &mut s.table_dialog;
            if !keep_report {
                d.dialog = None;
            }
            d.loading = !keep_report;
            d.load_error.clear();
            let qualifier = d
                .driver
                .as_ref()
                .map(|x| sql_qualifier(x, &d.schema_arg))
                .unwrap_or_default();
            (
                s.workspace.connection_id.clone(),
                d.schema_arg.clone(),
                d.table.clone(),
                qualifier,
                d.latest.begin(),
            )
        };
        let Some(d) = self.open_driver(&connection_id) else {
            self.session.borrow_mut().table_dialog.load_error =
                "This connection is no longer open.".to_string();
            self.session.borrow_mut().table_dialog.loading = false;
            self.table_dialog_refresh(Change::Structure);
            return;
        };
        self.table_dialog_refresh(Change::Structure);

        runtime::spawn(
            {
                let table = table.clone();
                async move {
                    d.get_table_schema(&schema_arg, &table)
                        .await
                        .map_err(|e| e.to_string())
                }
            },
            move |result| {
                with_ctx(|ctx| {
                    if !ticket.is_current() {
                        return;
                    }
                    {
                        let mut s = ctx.session.borrow_mut();
                        s.table_dialog.loading = false;
                        match result {
                            Ok(schema) => {
                                let mut editor =
                                    AlterEditor::from_schema(&qualifier, &table, &schema);
                                if keep_report {
                                    editor.step = 1;
                                }
                                s.table_dialog.dialog = Some(Dialog::Edit(editor));
                            }
                            Err(e) => {
                                s.table_dialog.load_error = format!("Could not read {table}: {e}");
                            }
                        }
                    }
                    ctx.table_dialog_refresh(Change::Structure);
                })
            },
        );
    }

    fn table_dialog_retry_load(self: &Rc<Self>) {
        self.table_dialog_load_edit(false);
    }

    // -- Projection -----------------------------------------------------------------

    pub(super) fn table_dialog_refresh(&self, change: Change) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let dark = w.global::<Theme>().get_dark();

        let (view, report, loading, load_error, running) = {
            let s = self.session.borrow();
            let d = &s.table_dialog;
            let view: Option<DialogView> = match (&d.dialog, &d.driver) {
                (Some(Dialog::Create(c)), Some(driver)) => Some(c.view(driver, &d.fk_tables)),
                (Some(Dialog::Edit(e)), Some(driver)) => Some(e.view(driver)),
                _ => None,
            };
            (
                view,
                d.report.clone(),
                d.loading,
                d.load_error.clone(),
                d.running,
            )
        };

        let state = w.global::<TableDialogState>();
        state.set_loading(loading);
        state.set_load_error(load_error.clone().into());
        state.set_running(running);
        match view {
            Some(v) => table_dialog_view::apply(&w, &v, change, &report, dark),
            None => {
                // Edit mode before the table has arrived: just a title.
                state.set_mode(1);
                state.set_title("Edit table".into());
                let table = self.session.borrow().table_dialog.table.clone();
                state.set_subtitle(table.into());
            }
        }
    }

    // -- Edits ------------------------------------------------------------------------

    fn table_dialog_act(self: &Rc<Self>, action: &str, i: i32, j: i32, text: &str) {
        let change = {
            let mut s = self.session.borrow_mut();
            let d = &mut s.table_dialog;
            if d.running {
                return;
            }
            // Any edit makes the last run's report stale.
            d.report.clear();
            let tables = d.fk_tables.clone();
            match d.dialog.as_mut() {
                Some(Dialog::Create(w)) => w.apply(action, i, j, text, &tables),
                Some(Dialog::Edit(e)) => e.apply(action, i, j, text),
                None => Change::None,
            }
        };
        if change == Change::None {
            return;
        }
        self.table_dialog_refresh(change);
        self.table_dialog_load_refs();
    }

    /// Ask for the columns of any foreign key target not yet read.
    fn table_dialog_load_refs(self: &Rc<Self>) {
        let (connection_id, schema_arg, missing) = {
            let mut guard = self.session.borrow_mut();
            let s = &mut *guard;
            let connection_id = s.workspace.connection_id.clone();
            let d = &mut s.table_dialog;
            let Some(Dialog::Create(w)) = d.dialog.as_mut() else {
                return;
            };
            let missing = w.missing_ref_tables();
            for t in &missing {
                w.ref_columns.insert(t.clone(), RefColumns::Loading);
            }
            (connection_id, d.schema_arg.clone(), missing)
        };
        if missing.is_empty() {
            return;
        }
        self.table_dialog_refresh(Change::Structure);
        let Some(d) = self.open_driver(&connection_id) else {
            return;
        };

        for table in missing {
            let (d, schema_arg) = (d.clone(), schema_arg.clone());
            runtime::spawn(
                {
                    let table = table.clone();
                    async move {
                        d.get_table_schema(&schema_arg, &table)
                            .await
                            .map(|s| s.columns.into_iter().map(|c| c.name).collect::<Vec<_>>())
                            .map_err(|e| e.to_string())
                    }
                },
                move |result| {
                    with_ctx(|ctx| {
                        {
                            let mut s = ctx.session.borrow_mut();
                            // The dialog may have closed, or become another
                            // one, while the read was in flight.
                            let Some(Dialog::Create(w)) = s.table_dialog.dialog.as_mut() else {
                                return;
                            };
                            w.ref_columns.insert(
                                table.clone(),
                                match result {
                                    Ok(cols) => RefColumns::Loaded(cols),
                                    Err(e) => RefColumns::Failed(e),
                                },
                            );
                        }
                        ctx.table_dialog_refresh(Change::Structure);
                    })
                },
            );
        }
    }

    // -- Running ------------------------------------------------------------------------

    /// The statements the open dialog would run, or why it cannot.
    fn table_dialog_statements(&self) -> Result<Vec<String>, String> {
        let s = self.session.borrow();
        let d = &s.table_dialog;
        let driver = d.driver.as_ref().ok_or("No connection")?;
        match &d.dialog {
            Some(Dialog::Create(w)) => w.statements(driver),
            Some(Dialog::Edit(e)) => e.statements(driver),
            None => Err("Nothing is open".to_string()),
        }
    }

    /// The Apply / Create button. Dropping a column deletes its data for good,
    /// so that asks first.
    fn table_dialog_apply(self: &Rc<Self>) {
        let statements = match self.table_dialog_statements() {
            Ok(s) if !s.is_empty() => s,
            _ => return,
        };
        let drops: Vec<String> = match &self.session.borrow().table_dialog.dialog {
            Some(Dialog::Edit(e)) => e
                .rows
                .iter()
                .filter(|r| r.removed)
                .filter_map(|r| r.row.origin.as_ref().map(|o| o.name.clone()))
                .collect(),
            _ => Vec::new(),
        };
        if drops.is_empty() {
            self.table_dialog_run(statements);
            return;
        }
        self.session.borrow_mut().pending_action = Some(PendingAction::ApplyTableDialog);
        self.show_confirm(
            "Drop columns?",
            &format!(
                "This permanently deletes {} and the data in {}. It cannot be undone.",
                drops.join(", "),
                if drops.len() == 1 { "it" } else { "them" }
            ),
            "Apply changes",
        );
    }

    /// The confirmation for a column drop was accepted.
    pub(super) fn table_dialog_apply_confirmed(self: &Rc<Self>) {
        if let Ok(statements) = self.table_dialog_statements() {
            if !statements.is_empty() {
                self.table_dialog_run(statements);
            }
        }
    }

    fn table_dialog_run(self: &Rc<Self>, statements: Vec<String>) {
        let (connection_id, ticket, creating, table_name) = {
            let mut guard = self.session.borrow_mut();
            let s = &mut *guard;
            let connection_id = s.workspace.connection_id.clone();
            let d = &mut s.table_dialog;
            if d.running {
                return;
            }
            d.running = true;
            d.report.clear();
            let (creating, name) = match &d.dialog {
                Some(Dialog::Create(w)) => (true, w.name.trim().to_string()),
                _ => (false, d.table.clone()),
            };
            (connection_id, d.latest.begin(), creating, name)
        };
        let Some(d) = self.open_driver(&connection_id) else {
            self.session.borrow_mut().table_dialog.running = false;
            self.toast("This connection is no longer open.", Tone::Danger);
            return;
        };
        self.table_dialog_refresh(Change::Structure);

        runtime::spawn(
            {
                let statements = statements.clone();
                async move {
                    // In order, stopping at the first failure, so a broken
                    // statement does not leave later ones applied on top.
                    let mut results: Vec<Result<(), String>> = Vec::new();
                    for stmt in &statements {
                        match d.execute_query(stmt).await {
                            Ok(_) => results.push(Ok(())),
                            Err(e) => {
                                results.push(Err(e.to_string()));
                                break;
                            }
                        }
                    }
                    results
                }
            },
            move |results| {
                with_ctx(|ctx| {
                    let rep = report(&statements, &results);
                    let ok = report_ok(&rep);
                    let ran_any = results.iter().any(|r| r.is_ok());
                    let still_here = ticket.is_current();
                    {
                        let mut s = ctx.session.borrow_mut();
                        s.table_dialog.running = false;
                        if still_here {
                            s.table_dialog.report = rep;
                        }
                    }
                    if still_here {
                        ctx.table_dialog_refresh(Change::Structure);
                    }
                    if ok {
                        ctx.toast(
                            if creating {
                                format!("Created table {table_name}")
                            } else {
                                format!("Updated table {table_name}")
                            },
                            Tone::Success,
                        );
                    } else {
                        ctx.toast(
                            "The table was not fully updated. See the report.",
                            Tone::Danger,
                        );
                    }
                    if ran_any {
                        // The sidebar and any open tab for the table now
                        // describe something that changed.
                        ctx.load_tables();
                        if !creating {
                            ctx.table_dialog_refresh_open_tab(&table_name);
                            if still_here {
                                ctx.table_dialog_load_edit(true);
                            }
                        }
                    }
                })
            },
        );
    }

    /// Reload the inspector of an open tab for the table that was altered.
    fn table_dialog_refresh_open_tab(self: &Rc<Self>, table: &str) {
        let id = {
            let s = self.session.borrow();
            s.workspace
                .tabs
                .tabs()
                .iter()
                .find(|t| matches!(&t.kind, TabKind::Table(n) if n == table))
                .map(|t| t.id)
        };
        if let Some(id) = id {
            self.inspector_open(id);
        }
    }

    fn table_dialog_copy(self: &Rc<Self>) {
        let Ok(statements) = self.table_dialog_statements() else {
            return;
        };
        if statements.is_empty() {
            return;
        }
        if let Some(w) = self.window.upgrade() {
            w.invoke_copy_to_clipboard(statements.join("\n\n").as_str().into());
        }
        self.toast("Copied the SQL", Tone::Success);
    }
}

/// Connect every `TableDialogState` callback to the controller.
pub(super) fn wire(window: &AppWindow, ctx: &Rc<Ctx>) {
    let d = window.global::<TableDialogState>();

    macro_rules! on {
        ($setter:ident, |$c:ident $(, $a:ident)*| $body:expr) => {{
            let $c = ctx.clone();
            d.$setter(move |$($a),*| {
                let $c = &$c;
                $body
            });
        }};
    }

    on!(on_open_create, |c| {
        c.dispatch(dbhive_ui_model::command_palette::CommandId::NewTable);
    });
    on!(on_open_edit_active, |c| c.table_dialog_open_edit_active());
    on!(on_close, |c| c.table_dialog_close());
    on!(on_retry_load, |c| c.table_dialog_retry_load());
    on!(on_act, |c, action, i, j, text| c
        .table_dialog_act(&action, i, j, &text));
    on!(on_apply, |c| c.table_dialog_apply());
    on!(on_copy_sql, |c| c.table_dialog_copy());
}
