//! Controller glue for schema migrations.
//!
//! A child module of `app`. The wizard's state and every decision (what a
//! step needs, how the diff reads, how dangerous the SQL is, what the result
//! means) live in `dbhive_ui_model::migrations` and `migrations_panel`; the
//! operations (diff, generate, apply) are `dbhive_core::service_migrations`.
//! This file moves values between those, the stores in `AppState`, and the
//! `MigrationsState` global in the window.
//!
//! There is no migration history to list or roll back: the backend keeps none
//! The transaction toggle is the only
//! undo, and it covers the run in progress.

use std::collections::HashSet;
use std::rc::Rc;

use dbhive_core::models::ConnectionProfile;
use dbhive_core::service_migrations as svc;
use dbhive_ui_model::migrations::{
    executable_count, split_statements, statements_to_text, Step, Wizard,
};
use dbhive_ui_model::migrations_panel::{
    apply_confirmation, blocked_reason, default_expanded, default_schema, diff_summary, diff_tree,
    mixed_driver_warning, option_index, outcome_view, profile_options, sql_summary, ProfileOption,
};
use slint::ComponentHandle;

use super::{with_ctx, Ctx, PendingAction};
use crate::connection_list::driver_label;
use crate::{migrations_view as view, runtime, sql_highlight};
use crate::{MigrationsState, Theme, Tone};

pub(super) struct MigrationSession {
    wizard: Wizard,
    expanded: HashSet<String>,
    /// The user typed a schema; stop suggesting one when the target changes.
    schema_touched: bool,
    busy: bool,
    busy_label: &'static str,
    ticket: runtime::Latest,
}

impl Default for MigrationSession {
    fn default() -> Self {
        Self {
            wizard: Wizard::default(),
            expanded: HashSet::new(),
            schema_touched: false,
            busy: false,
            busy_label: "",
            ticket: runtime::Latest::default(),
        }
    }
}

impl Ctx {
    /// Saved connections in a stable order (the store is a hash map).
    fn migration_profiles(&self) -> Vec<ConnectionProfile> {
        let mut profiles = self.profiles();
        profiles.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        });
        profiles
    }

    fn migration_options(&self, profiles: &[ConnectionProfile]) -> Vec<ProfileOption> {
        let connected = self.session.borrow().connected.clone();
        profile_options(profiles, &connected, driver_label)
    }

    // -- Projection ---------------------------------------------------------

    pub(super) fn refresh_migrations_panel(&self) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        let m = w.global::<MigrationsState>();
        let dark = w.global::<Theme>().get_dark();

        let profiles = self.migration_profiles();
        let options = self.migration_options(&profiles);
        let connected = self.session.borrow().connected.clone();

        let s = self.session.borrow();
        let g = &s.migrations;
        let wz = &g.wizard;

        m.set_connections(view::connection_labels(&options));
        m.set_source_index(option_index(&options, wz.source_id.as_deref()));
        m.set_target_index(option_index(&options, wz.target_id.as_deref()));
        if m.get_schema() != wz.schema.as_str() {
            m.set_schema(wz.schema.as_str().into());
        }

        // A note under the pickers: what is not open, then mixed dialects.
        let find = |id: &Option<String>| {
            id.as_deref()
                .and_then(|id| profiles.iter().find(|p| p.id == id))
        };
        let (source, target) = (find(&wz.source_id), find(&wz.target_id));
        let offline = dbhive_ui_model::migrations::offline_selection(source, target, &connected);
        let warning = if !offline.is_empty() {
            Some(format!(
                "Not connected: {}. Open {} from the connections screen first.",
                offline.join(", "),
                if offline.len() == 1 { "it" } else { "them" }
            ))
        } else {
            mixed_driver_warning(source, target, driver_label)
        };
        m.set_select_warning(warning.unwrap_or_default().into());

        m.set_step(wz.step.index() as i32);

        // Diff.
        match &wz.diff {
            Some(diff) => {
                m.set_diff_empty(dbhive_ui_model::migrations::is_empty_diff(diff));
                m.set_diff_summary(diff_summary(diff).into());
                m.set_diff_rows(view::diff_rows(&diff_tree(diff, &g.expanded)));
            }
            None => {
                m.set_diff_empty(false);
                m.set_diff_summary("".into());
                m.set_diff_rows(view::diff_rows(&[]));
            }
        }

        // SQL. The text itself is only written when it differs: typing already
        // updated the property, and rewriting it would move the caret.
        if m.get_sql() != wz.sql.as_str() {
            m.set_sql(wz.sql.as_str().into());
        }
        m.set_sql_lines(sql_highlight::to_model_for(&wz.sql, dark));
        let statements = split_statements(&wz.sql);
        let target_name = target.map_or("the target".to_string(), |p| p.name.clone());
        let edited = wz.sql != statements_to_text(&wz.generated);
        m.set_sql_summary(sql_summary(&statements, &target_name, edited).into());
        m.set_sql_edited(edited);
        m.set_target_name(target_name.as_str().into());
        m.set_use_transaction(wz.use_transaction);
        m.set_acknowledged(wz.acknowledged);

        // Footer and result.
        m.set_blocked_reason(
            blocked_reason(wz, &profiles, &connected)
                .unwrap_or_default()
                .into(),
        );
        m.set_busy(g.busy);
        m.set_busy_label(g.busy_label.into());
        m.set_error(wz.last_error.clone().unwrap_or_default().into());
        match &wz.outcome {
            Some(result) => {
                let o = outcome_view(result, wz.use_transaction);
                m.set_has_outcome(true);
                m.set_outcome_ok(o.ok);
                m.set_outcome_headline(o.headline.into());
                m.set_outcome_detail(o.detail.into());
                m.set_outcome_statement(o.failed_statement.into());
            }
            None => m.set_has_outcome(false),
        }
    }

    // -- Open and close ---------------------------------------------------------

    pub(super) fn open_migrations(self: &Rc<Self>) {
        let Some(w) = self.window.upgrade() else {
            return;
        };
        {
            let mut s = self.session.borrow_mut();
            // Start at step one but keep the connections and schema the user
            // chose last time, so reopening after a failure is not a retype.
            s.migrations.wizard.reset();
            s.migrations.expanded.clear();
            s.migrations.busy = false;
        }
        w.global::<MigrationsState>().set_visible(true);
        self.refresh_migrations_panel();
    }

    fn hide_migrations(&self) {
        if self.session.borrow().migrations.busy {
            return;
        }
        if let Some(w) = self.window.upgrade() {
            w.global::<MigrationsState>().set_visible(false);
        }
    }

    // -- Intents -----------------------------------------------------------------

    fn migration_pick(&self, source: bool, index: i32) {
        let profiles = self.migration_profiles();
        let Some(profile) = usize::try_from(index).ok().and_then(|i| profiles.get(i)) else {
            return;
        };
        {
            let mut s = self.session.borrow_mut();
            let touched = s.migrations.schema_touched;
            let wz = &mut s.migrations.wizard;
            if source {
                wz.source_id = Some(profile.id.clone());
            } else {
                wz.target_id = Some(profile.id.clone());
                // The target's dialect decides what a schema is called.
                if !touched {
                    wz.schema = default_schema(&profile.driver, profile.database.as_deref());
                }
            }
        }
        self.refresh_migrations_panel();
    }

    fn migration_compute(self: &Rc<Self>) {
        let (source, target, schema) = {
            let s = self.session.borrow();
            let wz = &s.migrations.wizard;
            match (&wz.source_id, &wz.target_id) {
                (Some(a), Some(b)) => (a.clone(), b.clone(), wz.schema.trim().to_string()),
                _ => return,
            }
        };
        let ticket = {
            let mut s = self.session.borrow_mut();
            s.migrations.busy = true;
            s.migrations.busy_label = "Comparing schemas…";
            s.migrations.wizard.last_error = None;
            s.migrations.wizard.outcome = None;
            s.migrations.ticket.begin()
        };
        self.refresh_migrations_panel();

        let state = self.state.clone();
        runtime::spawn(
            self.ui_id,
            async move { svc::compute_schema_diff(&state, &source, &target, &schema).await },
            move |result| {
                with_ctx(|ctx| {
                    if !ticket.is_current() {
                        return;
                    }
                    {
                        let mut s = ctx.session.borrow_mut();
                        s.migrations.busy = false;
                        match result {
                            Ok(diff) => {
                                s.migrations.expanded = default_expanded(&diff);
                                s.migrations.wizard.set_diff(diff);
                                s.migrations.wizard.step = Step::Diff;
                            }
                            Err(e) => {
                                s.migrations.wizard.last_error =
                                    Some(format!("Could not compare the schemas: {e}"));
                            }
                        }
                    }
                    ctx.refresh_migrations_panel();
                })
            },
        );
    }

    fn migration_generate(&self) {
        let (diff, target) = {
            let s = self.session.borrow();
            let wz = &s.migrations.wizard;
            match (&wz.diff, &wz.target_id) {
                (Some(d), Some(t)) => (d.clone(), t.clone()),
                _ => return,
            }
        };
        // Pure string work once the target's dialect is read out of state.
        match svc::generate_migration(&self.state, &diff, &target) {
            Ok(statements) => {
                let mut s = self.session.borrow_mut();
                let wz = &mut s.migrations.wizard;
                wz.sql = statements_to_text(&statements);
                wz.generated = statements;
                wz.acknowledged = false;
                wz.outcome = None;
                wz.last_error = None;
                wz.step = Step::Sql;
            }
            Err(e) => {
                self.session.borrow_mut().migrations.wizard.last_error =
                    Some(format!("Could not generate the SQL: {e}"));
            }
        }
        self.refresh_migrations_panel();
    }

    fn migration_edit(&self, f: impl FnOnce(&mut Wizard)) {
        f(&mut self.session.borrow_mut().migrations.wizard);
        self.refresh_migrations_panel();
    }

    fn migration_back(&self) {
        self.migration_edit(|wz| {
            wz.step = match wz.step {
                Step::Sql => Step::Diff,
                _ => Step::Select,
            };
            wz.outcome = None;
            wz.last_error = None;
        });
    }

    /// Why Apply cannot run now (not acknowledged, nothing executable, a
    /// connection closed), for the footer and as a guard on the callback: the
    /// button is disabled, but the rule must not live only in the button.
    fn migration_apply_blocked(&self) -> Option<String> {
        let profiles = self.migration_profiles();
        let connected = self.session.borrow().connected.clone();
        let s = self.session.borrow();
        if s.migrations.busy {
            return Some("Busy".to_string());
        }
        blocked_reason(&s.migrations.wizard, &profiles, &connected)
    }

    fn migration_apply_asked(&self) {
        if self.session.borrow().migrations.wizard.step != Step::Sql
            || self.migration_apply_blocked().is_some()
        {
            return;
        }
        let (title, message) = {
            let s = self.session.borrow();
            let wz = &s.migrations.wizard;
            let name = wz
                .target_id
                .as_deref()
                .and_then(|id| self.find_profile(id))
                .map_or("the target".to_string(), |p| p.name);
            apply_confirmation(&name, &split_statements(&wz.sql), wz.use_transaction)
        };
        self.session.borrow_mut().pending_action = Some(PendingAction::ApplyMigration);
        self.show_confirm(&title, &message, "Apply migration");
    }

    pub(super) fn migration_apply_confirmed(self: &Rc<Self>) {
        if self.migration_apply_blocked().is_some() {
            return;
        }
        let (target, statements, use_transaction) = {
            let s = self.session.borrow();
            let wz = &s.migrations.wizard;
            match &wz.target_id {
                Some(t) => (t.clone(), split_statements(&wz.sql), wz.use_transaction),
                None => return,
            }
        };
        if executable_count(&statements) == 0 {
            return;
        }
        {
            let mut s = self.session.borrow_mut();
            s.migrations.busy = true;
            s.migrations.busy_label = "Applying the migration…";
            s.migrations.wizard.outcome = None;
            s.migrations.wizard.last_error = None;
        }
        self.refresh_migrations_panel();

        let state = self.state.clone();
        let target_id = target.clone();
        runtime::spawn(
            self.ui_id,
            async move { svc::apply_migration(&state, &target_id, &statements, use_transaction).await },
            move |result| {
                with_ctx(|ctx| {
                    let mut toast = None;
                    {
                        let mut s = ctx.session.borrow_mut();
                        s.migrations.busy = false;
                        match result {
                            Ok(applied) => {
                                toast = Some(if applied.is_success() {
                                    (
                                        dbhive_ui_model::migrations::apply_summary(&applied),
                                        Tone::Success,
                                    )
                                } else {
                                    ("The migration failed".to_string(), Tone::Danger)
                                });
                                s.migrations.wizard.outcome = Some(applied);
                                // Applying again needs a fresh confirmation of intent.
                                s.migrations.wizard.acknowledged = false;
                            }
                            // Never started: no such connection.
                            Err(e) => s.migrations.wizard.last_error = Some(e.to_string()),
                        }
                    }
                    ctx.refresh_migrations_panel();
                    if let Some((text, tone)) = toast {
                        ctx.toast(text, tone);
                    }
                    // The sidebar should show the tables the migration made.
                    let showing_target = ctx.session.borrow().workspace.connection_id == target;
                    if showing_target {
                        ctx.load_tables();
                    }
                })
            },
        );
    }
}

/// Connect every `MigrationsState` callback to the controller.
pub(super) fn wire(window: &crate::AppWindow, ctx: &Rc<Ctx>) {
    let m = window.global::<MigrationsState>();

    macro_rules! on {
        ($setter:ident, |$c:ident $(, $arg:ident)*| $body:expr) => {{
            let $c = ctx.clone();
            m.$setter(move |$($arg),*| {
                let $c = &$c;
                $body
            });
        }};
    }

    on!(on_open_requested, |c| c.open_migrations());
    on!(on_close, |c| c.hide_migrations());
    on!(on_source_picked, |c, i| c.migration_pick(true, i));
    on!(on_target_picked, |c, i| c.migration_pick(false, i));
    on!(on_schema_edited, |c, t| {
        {
            let mut s = c.session.borrow_mut();
            s.migrations.schema_touched = true;
            s.migrations.wizard.schema = t.to_string();
        }
        c.refresh_migrations_panel();
    });
    on!(on_compute_diff, |c| c.migration_compute());
    on!(on_toggle_row, |c, key| {
        {
            let mut s = c.session.borrow_mut();
            let key = key.to_string();
            if !s.migrations.expanded.remove(&key) {
                s.migrations.expanded.insert(key);
            }
        }
        c.refresh_migrations_panel();
    });
    on!(on_generate, |c| c.migration_generate());
    on!(on_sql_changed, |c, t| c
        .migration_edit(|wz| wz.sql = t.to_string()));
    on!(on_reset_sql, |c| c.migration_edit(
        |wz| wz.sql = statements_to_text(&wz.generated)
    ));
    on!(on_transaction_toggled, |c| c.migration_edit(|wz| wz
        .use_transaction =
        !wz.use_transaction));
    on!(on_acknowledge_toggled, |c| c
        .migration_edit(|wz| wz.acknowledged = !wz.acknowledged));
    on!(on_back, |c| c.migration_back());
    on!(on_apply_requested, |c| c.migration_apply_asked());
}
