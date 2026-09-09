//! Schema migrations: diff two connections and sync the target.
//!
//! Parity target: the React `MigrationsDialog` (2.9 in
//! docs/rust-ui-migration/01-frontend-inventory.md). The same three-step
//! wizard, driving the same three operations, now in
//! `dbhive_core::service_migrations`:
//!
//! ```text
//! Select  pick source + target connection and a schema name
//! Diff    compute_schema_diff, reviewed as a tree or as a flat grid
//! SQL     generate_migration, reviewed in the editor, then apply_migration
//! ```
//!
//! Two deliberate differences from the web shell, both forced by the widgets
//! this crate standardises on:
//!
//! * The generated SQL is **editable**. `widgets::sql_editor` has no
//!   read-only mode, and hiding the statements behind a plain label would be
//!   worse. What gets executed is therefore re-parsed from the buffer with
//!   [`split_statements`] rather than being the generator's `Vec<String>`,
//!   and "Reset to generated" undoes any edit.
//! * Apply is gated twice, not once: the acknowledgement checkbox the web
//!   shell had, *and* a confirmation dialog naming the target and the
//!   statement count. This is the only operation in the screen that writes.
//!
//! There is no rollback action, because there is nothing to roll back to:
//! the backend keeps no migration history (`apply_migration` executes and
//! forgets). The transaction toggle is the only undo, and it only covers the
//! run in progress.

use std::collections::HashSet;

use dbhive_core::drivers::QueryResult;
use dbhive_core::migrations::{SchemaDiff, TableDiff};
use dbhive_core::models::{ConnectionProfile, DbError};
use dbhive_core::service_migrations::ApplyResult;

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::widgets::{results_grid, sql_editor};

// ===========================================================================
// State
// ===========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Step {
    #[default]
    Select,
    Diff,
    Sql,
}

impl Step {
    fn index(self) -> usize {
        match self {
            Step::Select => 0,
            Step::Diff => 1,
            Step::Sql => 2,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Step::Select => "Select",
            Step::Diff => "Diff",
            Step::Sql => "SQL",
        }
    }
}

/// How the diff is presented.
///
/// `Tree` is the web shell's nested, expandable view. `Grid` flattens every
/// change into one row and hands it to `widgets::results_grid`, which brings
/// filtering and sorting for free; a hundred-table diff is unreadable as a
/// tree and this is the cheap way to search it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffView {
    #[default]
    Tree,
    Grid,
}

pub struct Migrations {
    pub open: bool,

    pub step: Step,
    /// Profile id of the source (desired) schema.
    pub source_id: Option<String>,
    /// Profile id of the target, which is what gets modified.
    pub target_id: Option<String>,
    pub schema: String,

    pub diff: Option<SchemaDiff>,
    pub diff_view: DiffView,
    pub diff_grid: results_grid::GridState,
    /// Flattened diff, rebuilt only when the diff changes rather than every
    /// frame.
    diff_rows: Option<QueryResult>,

    /// Statements exactly as the generator produced them, kept so an edited
    /// buffer can be reset.
    pub generated: Vec<String>,
    /// The editable buffer actually applied.
    pub sql: String,
    pub editor: sql_editor::EditorState,

    pub use_transaction: bool,
    pub acknowledged: bool,
    /// True while the confirmation dialog is up.
    pub confirming: bool,

    pub last_error: Option<String>,
    pub outcome: Option<ApplyResult>,

    pub diff_job: Job<Result<SchemaDiff, DbError>>,
    pub apply_job: Job<Result<ApplyResult, DbError>>,

    /// Tracks the open -> closed edge so reopening starts a fresh wizard,
    /// the way remounting the React dialog did.
    was_open: bool,
}

impl Default for Migrations {
    fn default() -> Self {
        Self {
            open: false,
            step: Step::Select,
            source_id: None,
            target_id: None,
            // The web shell prefilled "public"; anything else is a first
            // keystroke every single time for the Postgres majority.
            schema: "public".to_string(),
            diff: None,
            diff_view: DiffView::Tree,
            diff_grid: results_grid::GridState::default(),
            diff_rows: None,
            generated: Vec::new(),
            sql: String::new(),
            editor: sql_editor::EditorState::default(),
            use_transaction: true,
            acknowledged: false,
            confirming: false,
            last_error: None,
            outcome: None,
            diff_job: Job::default(),
            apply_job: Job::default(),
            was_open: false,
        }
    }
}

impl Migrations {
    /// Open the wizard at step one, discarding whatever the last run left.
    pub fn open_dialog(&mut self) {
        self.open = true;
        self.reset();
    }

    /// Clear everything derived from a previous run, keeping the connection
    /// and schema choices so reopening after a failure is not a retype.
    pub fn reset(&mut self) {
        self.step = Step::Select;
        self.diff = None;
        self.diff_rows = None;
        self.generated.clear();
        self.sql.clear();
        self.acknowledged = false;
        self.confirming = false;
        self.last_error = None;
        self.outcome = None;
    }

    fn set_diff(&mut self, diff: SchemaDiff) {
        self.diff_rows = Some(diff_rows(&diff));
        self.diff = Some(diff);
        self.diff_grid.filter.clear();
    }

    fn busy(&self) -> bool {
        self.diff_job.is_running() || self.apply_job.is_running()
    }
}

// ===========================================================================
// Pure logic
// ===========================================================================

/// Nothing to migrate.
pub fn is_empty_diff(diff: &SchemaDiff) -> bool {
    diff.added_tables.is_empty()
        && diff.removed_tables.is_empty()
        && diff.modified_tables.is_empty()
}

/// Number of individual changes inside one modified table.
pub fn change_count(td: &TableDiff) -> usize {
    td.added_columns.len()
        + td.removed_columns.len()
        + td.modified_columns.len()
        + td.added_indexes.len()
        + td.removed_indexes.len()
        + td.added_fks.len()
        + td.removed_fks.len()
}

/// Why the Select step cannot proceed, or `None` if it can.
pub fn selection_problem(
    source: Option<&str>,
    target: Option<&str>,
    schema: &str,
) -> Option<&'static str> {
    match (source, target) {
        (None, _) | (_, None) => return Some("Select both a source and a target connection"),
        (Some(s), Some(t)) if s == t => {
            return Some("Source and target are the same connection; the diff would be empty")
        }
        _ => {}
    }
    if schema.trim().is_empty() {
        return Some("Enter a schema name, for example \"public\"");
    }
    None
}

/// Which of the chosen connections are not currently open.
///
/// Both have to be live: the diff reads metadata from each, and apply writes
/// to the target. Named rather than counted so the message can say which.
pub fn offline_selection(
    source: Option<&ConnectionProfile>,
    target: Option<&ConnectionProfile>,
    connected: &HashSet<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    for p in [source, target].into_iter().flatten() {
        if !connected.contains(&p.id) && !out.contains(&p.name) {
            out.push(p.name.clone());
        }
    }
    out
}

/// Render statements for the editor, the way the web shell did: one blank
/// line between statements and an explicit terminator on each.
pub fn statements_to_text(statements: &[String]) -> String {
    if statements.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (i, s) in statements.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        // A comment-only marker gets no terminator: the `;` would land
        // inside the comment, where it is both meaningless and invisible to
        // the splitter that has to read this text back.
        if dbhive_core::service_migrations::is_noop_statement(s) {
            out.push_str(s.trim_end());
            continue;
        }
        out.push_str(s.trim_end().trim_end_matches(';'));
        out.push(';');
    }
    out
}

/// Split an edited buffer back into statements.
///
/// Splitting on `;` alone is wrong the moment a default value, a comment or a
/// dollar-quoted function body contains one, and the result of getting it
/// wrong is a half-executed DDL migration. Quoted strings (`'`, `"`, backtick,
/// with doubled-quote and backslash escapes), line comments, block comments
/// and Postgres dollar quoting are all skipped over.
pub fn split_statements(sql: &str) -> Vec<String> {
    let chars: Vec<char> = sql.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        // Line comment: runs to the newline, which is kept.
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            while i < chars.len() && chars[i] != '\n' {
                cur.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // Block comment.
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            cur.push('/');
            cur.push('*');
            i += 2;
            while i < chars.len() {
                if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    cur.push('*');
                    cur.push('/');
                    i += 2;
                    break;
                }
                cur.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // Quoted string or quoted identifier.
        if c == '\'' || c == '"' || c == '`' {
            cur.push(c);
            i += 1;
            while i < chars.len() {
                // Backslash escape, as MySQL and Postgres E'' strings use.
                if chars[i] == '\\' && i + 1 < chars.len() {
                    cur.push(chars[i]);
                    cur.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                if chars[i] == c {
                    // A doubled quote is an escaped quote, not the end.
                    if chars.get(i + 1) == Some(&c) {
                        cur.push(c);
                        cur.push(c);
                        i += 2;
                        continue;
                    }
                    cur.push(c);
                    i += 1;
                    break;
                }
                cur.push(chars[i]);
                i += 1;
            }
            continue;
        }

        // Dollar-quoted body: $$...$$ or $tag$...$tag$.
        if c == '$' {
            if let Some(tag) = dollar_tag(&chars, i) {
                let close: Vec<char> = tag.chars().collect();
                cur.push_str(&tag);
                i += close.len();
                while i < chars.len() {
                    if chars[i..].starts_with(close.as_slice()) {
                        cur.push_str(&tag);
                        i += close.len();
                        break;
                    }
                    cur.push(chars[i]);
                    i += 1;
                }
                continue;
            }
        }

        if c == ';' {
            push_statement(&mut out, &mut cur);
            i += 1;
            continue;
        }

        cur.push(c);
        i += 1;
    }

    push_statement(&mut out, &mut cur);
    out
}

/// The `$tag$` opener starting at `at`, if there is one.
fn dollar_tag(chars: &[char], at: usize) -> Option<String> {
    if chars.get(at) != Some(&'$') {
        return None;
    }
    let mut j = at + 1;
    while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
        j += 1;
    }
    if chars.get(j) == Some(&'$') {
        Some(chars[at..=j].iter().collect())
    } else {
        None
    }
}

fn push_statement(out: &mut Vec<String>, cur: &mut String) {
    let trimmed = cur.trim();
    if !trimmed.is_empty() {
        out.push(trimmed.to_string());
    }
    cur.clear();
}

/// How many statements would actually reach the server.
///
/// The generator emits comment-only lines for changes it cannot express as
/// DDL; those are skipped at execution time, so counting them here would
/// promise more than apply delivers.
pub fn executable_count(statements: &[String]) -> usize {
    statements
        .iter()
        .filter(|s| !dbhive_core::service_migrations::is_noop_statement(s))
        .count()
}

/// Flatten a diff into one row per change, for `widgets::results_grid`.
pub fn diff_rows(diff: &SchemaDiff) -> QueryResult {
    let columns = vec![
        "Change".to_string(),
        "Object".to_string(),
        "Schema".to_string(),
        "Table".to_string(),
        "Name".to_string(),
        "Detail".to_string(),
    ];
    let mut rows: Vec<Vec<serde_json::Value>> = Vec::new();

    let row = |change: &str, object: &str, schema: &str, table: &str, name: &str, detail: String| {
        vec![
            serde_json::Value::String(change.to_string()),
            serde_json::Value::String(object.to_string()),
            serde_json::Value::String(schema.to_string()),
            serde_json::Value::String(table.to_string()),
            serde_json::Value::String(name.to_string()),
            serde_json::Value::String(detail),
        ]
    };

    for t in &diff.added_tables {
        rows.push(row(
            "Added",
            "table",
            &t.table.schema,
            &t.table.name,
            "",
            format!("{} column(s)", t.columns.len()),
        ));
    }
    for t in &diff.removed_tables {
        rows.push(row(
            "Removed",
            "table",
            &t.table.schema,
            &t.table.name,
            "",
            String::new(),
        ));
    }
    for td in &diff.modified_tables {
        for c in &td.added_columns {
            rows.push(row(
                "Added",
                "column",
                &td.schema,
                &td.name,
                &c.name,
                c.data_type.clone(),
            ));
        }
        for c in &td.removed_columns {
            rows.push(row(
                "Removed",
                "column",
                &td.schema,
                &td.name,
                &c.name,
                String::new(),
            ));
        }
        for c in &td.modified_columns {
            rows.push(row(
                "Modified",
                "column",
                &td.schema,
                &td.name,
                &c.name,
                format!("{} -> {}", c.target.data_type, c.source.data_type),
            ));
        }
        for idx in &td.added_indexes {
            rows.push(row(
                "Added",
                "index",
                &td.schema,
                &td.name,
                &idx.name,
                idx.columns.join(", "),
            ));
        }
        for idx in &td.removed_indexes {
            rows.push(row(
                "Removed",
                "index",
                &td.schema,
                &td.name,
                &idx.name,
                String::new(),
            ));
        }
        for fk in &td.added_fks {
            rows.push(row(
                "Added",
                "foreign key",
                &td.schema,
                &td.name,
                &fk.name,
                format!(
                    "{} -> {}({})",
                    fk.columns.join(", "),
                    fk.referenced_table,
                    fk.referenced_columns.join(", ")
                ),
            ));
        }
        for fk in &td.removed_fks {
            rows.push(row(
                "Removed",
                "foreign key",
                &td.schema,
                &td.name,
                &fk.name,
                String::new(),
            ));
        }
    }

    QueryResult::with_data(columns, rows)
}

/// One-line summary of an apply, for the toast and the inline banner.
pub fn apply_summary(result: &ApplyResult) -> String {
    match &result.error {
        Some(e) => format!(
            "Failed after {}/{} statement(s): {}",
            result.succeeded, result.executed, e
        ),
        None if result.executed == 0 => {
            "Nothing to apply: every statement was a comment".to_string()
        }
        None => format!("Applied {} statement(s)", result.succeeded),
    }
}

// ===========================================================================
// Drawing
// ===========================================================================

/// What the footer asked for, acted on after the borrows end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    ComputeDiff,
    GenerateSql,
    Back,
    RequestApply,
    ConfirmApply,
    CancelApply,
    ResetSql,
    Close,
}

/// Draw the wizard as a centred modal window. Hosts that only flip
/// `screens.migrations.open` should call this.
pub fn show_window(app: &mut DbHiveApp, ctx: &egui::Context) {
    if !app.screens.migrations.open {
        app.screens.migrations.was_open = false;
        return;
    }
    if !app.screens.migrations.was_open {
        app.screens.migrations.reset();
        app.screens.migrations.was_open = true;
    }

    let mut open = true;
    egui::Window::new("Schema Migrations")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size(egui::vec2(880.0, 620.0))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| show(app, ui));
    if !open {
        app.screens.migrations.open = false;
    }
}

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    // Collected first so the state lock is never held while drawing.
    let (profiles, connected) = snapshot(app);

    poll_jobs(app);

    let palette = app.mode.palette();
    let mut act: Option<Act> = None;

    egui::TopBottomPanel::top("migrations_header")
        .frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(0.0, 6.0)))
        .show_inside(ui, |ui| {
            ui.label(crate::theme::text::heading(palette, "Schema Migrations"));
            ui.label(crate::theme::text::muted(
                palette,
                "Compare two connections and generate migration SQL to sync schemas.",
            ));
            ui.add_space(6.0);
            step_bar(ui, palette, app.screens.migrations.step);
        });

    egui::TopBottomPanel::bottom("migrations_footer")
        .frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(0.0, 8.0)))
        .show_inside(ui, |ui| {
            ui.separator();
            act = footer(app, ui, &profiles, &connected);
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::none())
        .show_inside(ui, |ui| {
            if let Some(err) = app.screens.migrations.last_error.clone() {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(palette.destructive, "Error:");
                    ui.label(err);
                    if ui.small_button("Dismiss").clicked() {
                        app.screens.migrations.last_error = None;
                    }
                });
                ui.add_space(6.0);
            }
            if let Some(outcome) = app.screens.migrations.outcome.clone() {
                outcome_banner(ui, palette, &outcome);
            }

            match app.screens.migrations.step {
                Step::Select => select_step(app, ui, &profiles, &connected),
                Step::Diff => diff_step(app, ui),
                Step::Sql => sql_step(app, ui, &profiles),
            }
        });

    if let Some(act) = act {
        dispatch(app, ui.ctx(), act);
    }

    // Drawn from here rather than from `show_window` so the confirmation is
    // present however this screen is hosted. It is a top-level area, not a
    // child of whatever `ui` belongs to, and sits in the foreground layer so
    // it cannot end up behind the wizard it is confirming.
    confirm_dialog(app, ui.ctx());
}

/// Profiles and the ids of the connections currently open.
fn snapshot(app: &DbHiveApp) -> (Vec<ConnectionProfile>, HashSet<String>) {
    let guard = app.state.lock().unwrap();
    let mut profiles: Vec<ConnectionProfile> =
        guard.list_profiles().into_iter().cloned().collect();
    profiles.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    let connected: HashSet<String> = guard.connections.keys().cloned().collect();
    (profiles, connected)
}

fn poll_jobs(app: &mut DbHiveApp) {
    if let Some(result) = app.screens.migrations.diff_job.take_result() {
        match result {
            Ok(diff) => {
                let empty = is_empty_diff(&diff);
                app.screens.migrations.set_diff(diff);
                app.screens.migrations.step = Step::Diff;
                app.screens.migrations.last_error = None;
                if empty {
                    app.toasts.info("Schemas are identical");
                }
            }
            Err(e) => {
                let msg = format!("Diff failed: {}", e);
                app.screens.migrations.last_error = Some(msg.clone());
                app.toasts.error(msg);
            }
        }
    }

    if let Some(result) = app.screens.migrations.apply_job.take_result() {
        match result {
            Ok(outcome) => {
                let summary = apply_summary(&outcome);
                if outcome.is_success() {
                    app.toasts.success(summary);
                } else {
                    app.toasts.error(summary);
                }
                app.screens.migrations.outcome = Some(outcome);
            }
            Err(e) => {
                let msg = format!("Apply failed: {}", e);
                app.screens.migrations.last_error = Some(msg.clone());
                app.toasts.error(msg);
            }
        }
    }
}

fn step_bar(ui: &mut egui::Ui, palette: &crate::theme::Palette, current: Step) {
    ui.horizontal(|ui| {
        for (i, step) in [Step::Select, Step::Diff, Step::Sql].into_iter().enumerate() {
            if i > 0 {
                ui.label(crate::theme::text::muted(palette, "›"));
            }
            let label = format!("{}. {}", i + 1, step.title());
            if step == current {
                ui.label(crate::theme::text::strong(palette, label));
            } else if step.index() < current.index() {
                ui.label(egui::RichText::new(label).color(palette.primary));
            } else {
                ui.label(crate::theme::text::muted(palette, label));
            }
        }
    });
}

fn outcome_banner(ui: &mut egui::Ui, palette: &crate::theme::Palette, outcome: &ApplyResult) {
    let colour = if outcome.is_success() {
        palette.success
    } else {
        palette.destructive
    };
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(colour, apply_summary(outcome));
    });
    if let Some(stmt) = &outcome.failed_statement {
        ui.label(crate::theme::text::muted(
            palette,
            format!("Failed on: {}", truncate(stmt, 160)),
        ));
    }
    ui.add_space(6.0);
}

fn truncate(s: &str, max: usize) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max {
        one_line
    } else {
        let head: String = one_line.chars().take(max).collect();
        format!("{}…", head)
    }
}

// --- Step 1: select -------------------------------------------------------

fn select_step(
    app: &mut DbHiveApp,
    ui: &mut egui::Ui,
    profiles: &[ConnectionProfile],
    connected: &HashSet<String>,
) {
    let palette = app.mode.palette();

    if profiles.is_empty() {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(crate::theme::text::muted(
                palette,
                "No saved connections. Add two connections before migrating.",
            ));
        });
        return;
    }

    ui.add_space(8.0);
    ui.label(crate::theme::text::strong(palette, "Source (desired schema)"));
    profile_picker(
        ui,
        palette,
        "migrations_source",
        profiles,
        connected,
        &mut app.screens.migrations.source_id,
    );

    ui.add_space(10.0);
    ui.label(crate::theme::text::strong(palette, "Target (will be modified)"));
    profile_picker(
        ui,
        palette,
        "migrations_target",
        profiles,
        connected,
        &mut app.screens.migrations.target_id,
    );

    ui.add_space(10.0);
    ui.label(crate::theme::text::strong(palette, "Schema"));
    ui.add(
        egui::TextEdit::singleline(&mut app.screens.migrations.schema)
            .hint_text("public")
            .desired_width(360.0),
    );

    ui.add_space(10.0);
    ui.label(crate::theme::text::muted(
        palette,
        "Both connections must be open before running. Views are ignored: only base tables are diffed.",
    ));

    let source = profile_of(profiles, app.screens.migrations.source_id.as_deref());
    let target = profile_of(profiles, app.screens.migrations.target_id.as_deref());
    let offline = offline_selection(source, target, connected);
    if !offline.is_empty() {
        ui.add_space(4.0);
        ui.colored_label(
            palette.warning,
            format!("Not connected: {}", offline.join(", ")),
        );
    }
    if let (Some(s), Some(t)) = (source, target) {
        if s.driver != t.driver {
            ui.add_space(4.0);
            ui.colored_label(
                palette.warning,
                format!(
                    "{} and {} are different database types; generated SQL uses the target's dialect and may not translate cleanly.",
                    crate::screens::connections::driver_label(&s.driver),
                    crate::screens::connections::driver_label(&t.driver),
                ),
            );
        }
    }
}

fn profile_of<'a>(
    profiles: &'a [ConnectionProfile],
    id: Option<&str>,
) -> Option<&'a ConnectionProfile> {
    let id = id?;
    profiles.iter().find(|p| p.id == id)
}

fn profile_picker(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    id_salt: &str,
    profiles: &[ConnectionProfile],
    connected: &HashSet<String>,
    selected: &mut Option<String>,
) {
    let label = profile_of(profiles, selected.as_deref())
        .map(|p| {
            format!(
                "{} ({})",
                p.name,
                crate::screens::connections::driver_label(&p.driver)
            )
        })
        .unwrap_or_else(|| "Select connection".to_string());

    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(label)
        .width(360.0)
        .show_ui(ui, |ui| {
            for p in profiles {
                let is_selected = selected.as_deref() == Some(p.id.as_str());
                let mark = if connected.contains(&p.id) { "•" } else { "○" };
                let text = format!(
                    "{} {} ({})",
                    mark,
                    p.name,
                    crate::screens::connections::driver_label(&p.driver)
                );
                let colour = if connected.contains(&p.id) {
                    palette.success
                } else {
                    palette.muted_foreground
                };
                let response =
                    ui.selectable_label(is_selected, egui::RichText::new(text).color(colour));
                if response.clicked() {
                    *selected = Some(p.id.clone());
                }
            }
        });
}

// --- Step 2: diff ---------------------------------------------------------

fn diff_step(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    let Some(diff) = app.screens.migrations.diff.clone() else {
        ui.label(crate::theme::text::muted(palette, "No diff computed yet."));
        return;
    };

    if is_empty_diff(&diff) {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(crate::theme::text::strong(
                palette,
                "Schemas are identical. No migration needed.",
            ));
        });
        return;
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let view = &mut app.screens.migrations.diff_view;
        ui.selectable_value(view, DiffView::Tree, "Tree");
        ui.selectable_value(view, DiffView::Grid, "All changes");
        ui.separator();
        ui.label(crate::theme::text::muted(
            palette,
            format!(
                "{} added, {} removed, {} modified table(s)",
                diff.added_tables.len(),
                diff.removed_tables.len(),
                diff.modified_tables.len()
            ),
        ));
    });
    ui.add_space(6.0);

    match app.screens.migrations.diff_view {
        DiffView::Tree => {
            egui::ScrollArea::vertical()
                .id_salt("migrations_diff_tree")
                .auto_shrink([false, false])
                .show(ui, |ui| diff_tree(ui, palette, &diff));
        }
        DiffView::Grid => {
            if let Some(rows) = app.screens.migrations.diff_rows.clone() {
                results_grid::show(ui, palette, &rows, &mut app.screens.migrations.diff_grid);
            }
        }
    }
}

fn diff_tree(ui: &mut egui::Ui, palette: &crate::theme::Palette, diff: &SchemaDiff) {
    if !diff.added_tables.is_empty() {
        section(
            ui,
            "migrations_added",
            palette.success,
            format!("Added tables ({})", diff.added_tables.len()),
            |ui| {
                for t in &diff.added_tables {
                    ui.colored_label(
                        palette.success,
                        format!(
                            "+ {}.{} ({} columns)",
                            t.table.schema,
                            t.table.name,
                            t.columns.len()
                        ),
                    );
                }
            },
        );
    }

    if !diff.removed_tables.is_empty() {
        section(
            ui,
            "migrations_removed",
            palette.destructive,
            format!("Removed tables ({})", diff.removed_tables.len()),
            |ui| {
                for t in &diff.removed_tables {
                    ui.colored_label(
                        palette.destructive,
                        format!("- {}.{}", t.table.schema, t.table.name),
                    );
                }
            },
        );
    }

    if !diff.modified_tables.is_empty() {
        section(
            ui,
            "migrations_modified",
            palette.warning,
            format!("Modified tables ({})", diff.modified_tables.len()),
            |ui| {
                for td in &diff.modified_tables {
                    table_diff_row(ui, palette, td);
                }
            },
        );
    }
}

/// A coloured heading over an indented block, matching the web shell's
/// left-bordered sections. `Ui::indent` draws the rule itself, so the block
/// cannot end up with a line longer than its content.
fn section(
    ui: &mut egui::Ui,
    id_salt: &str,
    colour: egui::Color32,
    title: String,
    body: impl FnOnce(&mut egui::Ui),
) {
    ui.label(egui::RichText::new(title).strong().color(colour));
    ui.indent(id_salt, body);
    ui.add_space(8.0);
}

fn table_diff_row(ui: &mut egui::Ui, palette: &crate::theme::Palette, td: &TableDiff) {
    let header = format!(
        "{}.{}  ({} changes)",
        td.schema,
        td.name,
        change_count(td)
    );
    egui::CollapsingHeader::new(egui::RichText::new(header).color(palette.foreground))
        .id_salt(format!("migdiff:{}.{}", td.schema, td.name))
        .default_open(false)
        .show(ui, |ui| {
            let mono = |s: String, c: egui::Color32| egui::RichText::new(s).monospace().color(c);
            for c in &td.added_columns {
                ui.label(mono(
                    format!("+ column {} {}", c.name, c.data_type),
                    palette.success,
                ));
            }
            for c in &td.removed_columns {
                ui.label(mono(format!("- column {}", c.name), palette.destructive));
            }
            for c in &td.modified_columns {
                ui.label(mono(
                    format!(
                        "~ column {}: {} ➡ {}",
                        c.name, c.target.data_type, c.source.data_type
                    ),
                    palette.warning,
                ));
            }
            for i in &td.added_indexes {
                ui.label(mono(
                    format!("+ index {} ({})", i.name, i.columns.join(", ")),
                    palette.success,
                ));
            }
            for i in &td.removed_indexes {
                ui.label(mono(format!("- index {}", i.name), palette.destructive));
            }
            for f in &td.added_fks {
                ui.label(mono(
                    format!("+ fk {} ➡ {}", f.name, f.referenced_table),
                    palette.success,
                ));
            }
            for f in &td.removed_fks {
                ui.label(mono(format!("- fk {}", f.name), palette.destructive));
            }
        });
}

// --- Step 3: SQL ----------------------------------------------------------

fn sql_step(app: &mut DbHiveApp, ui: &mut egui::Ui, profiles: &[ConnectionProfile]) {
    let palette = app.mode.palette();
    let target_name = profile_of(profiles, app.screens.migrations.target_id.as_deref())
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "the target".to_string());

    let statements = split_statements(&app.screens.migrations.sql);
    let executable = executable_count(&statements);
    let edited = app.screens.migrations.sql != statements_to_text(&app.screens.migrations.generated);

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(crate::theme::text::muted(
            palette,
            format!(
                "{} statement(s) will be executed against {}.",
                executable, target_name
            ),
        ));
        if edited {
            ui.label(egui::RichText::new("edited").color(palette.warning).small());
        }
    });
    ui.add_space(4.0);

    // The editor is the standard widget; there is no read-only variant, so
    // edits are allowed and re-parsed on apply.
    let available = (ui.available_height() - 92.0).max(140.0);
    egui::Frame::none()
        .stroke(egui::Stroke::new(1.0_f32, palette.border))
        .inner_margin(egui::Margin::same(4.0))
        .show(ui, |ui| {
            ui.set_min_height(available);
            ui.set_max_height(available);
            let schema_index = sql_editor::SchemaIndex::default();
            let _ = sql_editor::show(
                ui,
                palette,
                &mut app.screens.migrations.editor,
                &schema_index,
                &mut app.screens.migrations.sql,
            );
        });

    ui.add_space(6.0);
    ui.checkbox(
        &mut app.screens.migrations.use_transaction,
        "Run inside a transaction",
    );
    ui.checkbox(
        &mut app.screens.migrations.acknowledged,
        "I understand this will modify the target database",
    );
}

// --- Footer and confirmation ---------------------------------------------

fn footer(
    app: &mut DbHiveApp,
    ui: &mut egui::Ui,
    profiles: &[ConnectionProfile],
    connected: &HashSet<String>,
) -> Option<Act> {
    let palette = app.mode.palette();
    let m = &app.screens.migrations;
    let busy = m.busy();
    let step = m.step;
    let mut act: Option<Act> = None;

    let blocked = match step {
        Step::Select => {
            let problem = selection_problem(
                m.source_id.as_deref(),
                m.target_id.as_deref(),
                &m.schema,
            );
            let offline = offline_selection(
                profile_of(profiles, m.source_id.as_deref()),
                profile_of(profiles, m.target_id.as_deref()),
                connected,
            );
            problem.map(|p| p.to_string()).or_else(|| {
                (!offline.is_empty())
                    .then(|| format!("Connect first: {}", offline.join(", ")))
            })
        }
        Step::Diff => match &m.diff {
            Some(d) if is_empty_diff(d) => Some("Nothing to generate".to_string()),
            Some(_) => None,
            None => Some("No diff".to_string()),
        },
        Step::Sql => {
            if !m.acknowledged {
                Some("Confirm the checkbox to enable Apply".to_string())
            } else if executable_count(&split_statements(&m.sql)) == 0 {
                Some("No executable statements".to_string())
            } else {
                None
            }
        }
    };

    ui.horizontal(|ui| {
        if busy {
            ui.spinner();
        }
        if let Some(reason) = &blocked {
            ui.label(crate::theme::text::muted(palette, reason.clone()));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            match step {
                Step::Select => {
                    let btn = ui.add_enabled(
                        !busy && blocked.is_none(),
                        egui::Button::new("Compute Diff"),
                    );
                    if btn.clicked() {
                        act = Some(Act::ComputeDiff);
                    }
                    if ui.button("Cancel").clicked() {
                        act = Some(Act::Close);
                    }
                }
                Step::Diff => {
                    let btn = ui
                        .add_enabled(!busy && blocked.is_none(), egui::Button::new("Generate SQL"));
                    if btn.clicked() {
                        act = Some(Act::GenerateSql);
                    }
                    if ui.button("Back").clicked() {
                        act = Some(Act::Back);
                    }
                }
                Step::Sql => {
                    let apply = ui.add_enabled(
                        !busy && blocked.is_none(),
                        egui::Button::new(
                            egui::RichText::new("Apply Migration")
                                .color(palette.destructive)
                                .strong(),
                        ),
                    );
                    if apply.clicked() {
                        act = Some(Act::RequestApply);
                    }
                    if ui.button("Back").clicked() {
                        act = Some(Act::Back);
                    }
                    if ui
                        .add_enabled(!busy, egui::Button::new("Reset to generated"))
                        .clicked()
                    {
                        act = Some(Act::ResetSql);
                    }
                }
            }
        });
    });

    act
}

/// The second gate on the only destructive operation here.
fn confirm_dialog(app: &mut DbHiveApp, ctx: &egui::Context) {
    if !app.screens.migrations.confirming {
        return;
    }
    let palette = app.mode.palette();
    let (profiles, _) = snapshot(app);
    let target_name = profile_of(profiles.as_slice(), app.screens.migrations.target_id.as_deref())
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "the target".to_string());
    let statements = split_statements(&app.screens.migrations.sql);
    let count = executable_count(&statements);
    let use_transaction = app.screens.migrations.use_transaction;

    let mut act: Option<Act> = None;
    let mut open = true;
    egui::Window::new("Apply migration?")
        .open(&mut open)
        .order(egui::Order::Foreground)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(format!(
                "{} statement(s) will run against {}.",
                count, target_name
            ))
            .color(palette.foreground));
            ui.add_space(4.0);
            ui.label(crate::theme::text::muted(
                palette,
                if use_transaction {
                    "Wrapped in a transaction: the first failure rolls the whole migration back."
                } else {
                    "No transaction: a failure part-way leaves earlier statements applied."
                },
            ));
            ui.add_space(4.0);
            ui.colored_label(palette.destructive, "This cannot be undone.");
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    act = Some(Act::CancelApply);
                }
                if ui
                    .button(
                        egui::RichText::new(format!("Apply {} statement(s)", count))
                            .color(palette.destructive)
                            .strong(),
                    )
                    .clicked()
                {
                    act = Some(Act::ConfirmApply);
                }
            });
        });
    if !open {
        act = Some(Act::CancelApply);
    }

    if let Some(act) = act {
        dispatch(app, ctx, act);
    }
}

fn dispatch(app: &mut DbHiveApp, ctx: &egui::Context, act: Act) {
    match act {
        Act::Close => {
            app.screens.migrations.open = false;
        }
        Act::Back => {
            app.screens.migrations.step = match app.screens.migrations.step {
                Step::Sql => Step::Diff,
                _ => Step::Select,
            };
            app.screens.migrations.outcome = None;
        }
        Act::ComputeDiff => {
            let (Some(source), Some(target)) = (
                app.screens.migrations.source_id.clone(),
                app.screens.migrations.target_id.clone(),
            ) else {
                return;
            };
            let schema = app.screens.migrations.schema.trim().to_string();
            let state = app.state.clone();
            let handle = app.rt.handle().clone();
            app.screens.migrations.last_error = None;
            app.screens.migrations.outcome = None;
            app.screens
                .migrations
                .diff_job
                .start(&handle, ctx, async move {
                    dbhive_core::service_migrations::compute_schema_diff(
                        &state, &source, &target, &schema,
                    )
                    .await
                });
        }
        Act::GenerateSql => {
            let Some(diff) = app.screens.migrations.diff.clone() else {
                return;
            };
            let Some(target) = app.screens.migrations.target_id.clone() else {
                return;
            };
            // Pure string work once the target's dialect is read out of
            // state, so it runs inline rather than through a Job.
            match dbhive_core::service_migrations::generate_migration(&app.state, &diff, &target) {
                Ok(statements) => {
                    app.screens.migrations.sql = statements_to_text(&statements);
                    app.screens.migrations.generated = statements;
                    app.screens.migrations.acknowledged = false;
                    app.screens.migrations.outcome = None;
                    app.screens.migrations.last_error = None;
                    app.screens.migrations.step = Step::Sql;
                }
                Err(e) => {
                    let msg = format!("Generate failed: {}", e);
                    app.screens.migrations.last_error = Some(msg.clone());
                    app.toasts.error(msg);
                }
            }
        }
        Act::ResetSql => {
            let text = statements_to_text(&app.screens.migrations.generated);
            app.screens.migrations.sql = text;
        }
        Act::RequestApply => {
            app.screens.migrations.confirming = true;
        }
        Act::CancelApply => {
            app.screens.migrations.confirming = false;
        }
        Act::ConfirmApply => {
            app.screens.migrations.confirming = false;
            let Some(target) = app.screens.migrations.target_id.clone() else {
                return;
            };
            let statements = split_statements(&app.screens.migrations.sql);
            if statements.is_empty() {
                return;
            }
            let use_transaction = app.screens.migrations.use_transaction;
            let state = app.state.clone();
            let handle = app.rt.handle().clone();
            app.screens.migrations.outcome = None;
            app.screens.migrations.last_error = None;
            app.screens
                .migrations
                .apply_job
                .start(&handle, ctx, async move {
                    dbhive_core::service_migrations::apply_migration(
                        &state,
                        &target,
                        &statements,
                        use_transaction,
                    )
                    .await
                });
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use dbhive_core::migrations::ColumnChange;
    use dbhive_core::models::{ColumnInfo, DbDriver, ForeignKeyInfo, IndexInfo, TableInfo, TableSchema};

    fn column(name: &str, ty: &str) -> ColumnInfo {
        ColumnInfo::new(name.to_string(), ty.to_string(), true)
    }

    fn table(schema: &str, name: &str, columns: Vec<ColumnInfo>) -> TableSchema {
        TableSchema::new(
            TableInfo::new(name.to_string(), schema.to_string(), "TABLE".to_string()),
            columns,
            Vec::new(),
        )
    }

    fn table_diff(schema: &str, name: &str) -> TableDiff {
        TableDiff {
            schema: schema.to_string(),
            name: name.to_string(),
            added_columns: Vec::new(),
            removed_columns: Vec::new(),
            modified_columns: Vec::new(),
            added_indexes: Vec::new(),
            removed_indexes: Vec::new(),
            added_fks: Vec::new(),
            removed_fks: Vec::new(),
        }
    }

    fn profile(id: &str, name: &str) -> ConnectionProfile {
        ConnectionProfile::new(
            id.to_string(),
            name.to_string(),
            DbDriver::Postgres,
            "localhost".to_string(),
            5432,
            "postgres".to_string(),
        )
    }

    // --- diff emptiness and counting -------------------------------------

    #[test]
    fn empty_diff_is_detected() {
        assert!(is_empty_diff(&SchemaDiff::default()));

        let mut d = SchemaDiff::default();
        d.added_tables.push(table("public", "users", vec![]));
        assert!(!is_empty_diff(&d));

        let mut d = SchemaDiff::default();
        d.modified_tables.push(table_diff("public", "users"));
        assert!(!is_empty_diff(&d));
    }

    #[test]
    fn change_count_sums_every_category() {
        let mut td = table_diff("public", "users");
        td.added_columns.push(column("a", "int"));
        td.removed_columns.push(column("b", "int"));
        td.modified_columns.push(ColumnChange {
            name: "c".into(),
            source: column("c", "bigint"),
            target: column("c", "int"),
        });
        td.added_indexes
            .push(IndexInfo::new("i1".into(), vec!["a".into()], false, false));
        td.removed_indexes
            .push(IndexInfo::new("i2".into(), vec!["b".into()], false, false));
        td.added_fks.push(ForeignKeyInfo {
            name: "f1".into(),
            table: "users".into(),
            schema: "public".into(),
            columns: vec!["a".into()],
            referenced_table: "orgs".into(),
            referenced_schema: "public".into(),
            referenced_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        });
        assert_eq!(change_count(&td), 6);
        assert_eq!(change_count(&table_diff("public", "empty")), 0);
    }

    // --- selection validation --------------------------------------------

    #[test]
    fn selection_requires_two_distinct_connections_and_a_schema() {
        assert!(selection_problem(None, Some("b"), "public").is_some());
        assert!(selection_problem(Some("a"), None, "public").is_some());
        assert!(selection_problem(Some("a"), Some("a"), "public").is_some());
        assert!(selection_problem(Some("a"), Some("b"), "   ").is_some());
        assert!(selection_problem(Some("a"), Some("b"), "public").is_none());
    }

    #[test]
    fn offline_selection_names_each_disconnected_profile_once() {
        let src = profile("1", "Prod");
        let tgt = profile("2", "Staging");
        let mut connected = HashSet::new();

        assert_eq!(
            offline_selection(Some(&src), Some(&tgt), &connected),
            vec!["Prod".to_string(), "Staging".to_string()]
        );

        connected.insert("1".to_string());
        assert_eq!(
            offline_selection(Some(&src), Some(&tgt), &connected),
            vec!["Staging".to_string()]
        );

        connected.insert("2".to_string());
        assert!(offline_selection(Some(&src), Some(&tgt), &connected).is_empty());

        // Same profile picked twice must not be listed twice.
        assert_eq!(
            offline_selection(Some(&src), Some(&src), &HashSet::new()),
            vec!["Prod".to_string()]
        );
    }

    // --- statement text round trip ---------------------------------------

    #[test]
    fn statements_render_with_terminators_and_blank_lines() {
        let text = statements_to_text(&[
            "CREATE TABLE a (id int)".to_string(),
            "DROP TABLE b".to_string(),
        ]);
        assert_eq!(text, "CREATE TABLE a (id int);\n\nDROP TABLE b;");
        assert_eq!(statements_to_text(&[]), "");
    }

    #[test]
    fn already_terminated_statements_are_not_double_terminated() {
        let text = statements_to_text(&["SELECT 1;".to_string()]);
        assert_eq!(text, "SELECT 1;");
    }

    #[test]
    fn comment_markers_render_without_a_terminator() {
        // A `;` appended here would end up inside the comment, where the
        // splitter could never see it again.
        let text = statements_to_text(&[
            "-- unsupported change".to_string(),
            "DROP TABLE b".to_string(),
        ]);
        assert_eq!(text, "-- unsupported change\n\nDROP TABLE b;");
    }

    #[test]
    fn rendering_then_splitting_is_a_round_trip() {
        let statements = vec![
            "CREATE TABLE a (id int)".to_string(),
            "ALTER TABLE a ADD COLUMN note text DEFAULT 'a;b'".to_string(),
            "-- cannot express this change".to_string(),
        ];
        let text = statements_to_text(&statements);
        assert_eq!(split_statements(&text), statements);
    }

    // --- statement splitting ---------------------------------------------

    #[test]
    fn split_ignores_semicolons_inside_string_literals() {
        let out = split_statements("INSERT INTO t VALUES ('a;b'); SELECT 1");
        assert_eq!(out, vec!["INSERT INTO t VALUES ('a;b')", "SELECT 1"]);
    }

    #[test]
    fn split_handles_doubled_and_backslash_quote_escapes() {
        let out = split_statements("SELECT 'it''s; fine'; SELECT 2");
        assert_eq!(out, vec!["SELECT 'it''s; fine'", "SELECT 2"]);

        let out = split_statements("SELECT 'a\\'; b'; SELECT 3");
        assert_eq!(out, vec!["SELECT 'a\\'; b'", "SELECT 3"]);
    }

    #[test]
    fn split_ignores_semicolons_inside_quoted_identifiers() {
        let out = split_statements("DROP TABLE \"weird;name\"; DROP TABLE `back;tick`");
        assert_eq!(
            out,
            vec!["DROP TABLE \"weird;name\"", "DROP TABLE `back;tick`"]
        );
    }

    #[test]
    fn split_ignores_semicolons_inside_comments() {
        let out = split_statements("SELECT 1 -- trailing; comment\n; SELECT 2");
        assert_eq!(out, vec!["SELECT 1 -- trailing; comment", "SELECT 2"]);

        let out = split_statements("SELECT /* a; b */ 1; SELECT 2");
        assert_eq!(out, vec!["SELECT /* a; b */ 1", "SELECT 2"]);
    }

    #[test]
    fn split_ignores_semicolons_inside_dollar_quoted_bodies() {
        let sql = "CREATE FUNCTION f() RETURNS int AS $$ BEGIN; RETURN 1; END; $$ LANGUAGE plpgsql; SELECT 1";
        let out = split_statements(sql);
        assert_eq!(out.len(), 2);
        assert!(out[0].contains("BEGIN; RETURN 1; END;"));
        assert_eq!(out[1], "SELECT 1");

        let tagged = "CREATE FUNCTION g() AS $body$ x; y $body$; SELECT 2";
        let out = split_statements(tagged);
        assert_eq!(out.len(), 2);
        assert!(out[0].contains("x; y"));
    }

    #[test]
    fn split_drops_blank_and_trailing_empty_statements() {
        assert!(split_statements("   \n\n  ").is_empty());
        assert_eq!(split_statements("SELECT 1;;;"), vec!["SELECT 1"]);
        assert_eq!(split_statements(";"), Vec::<String>::new());
    }

    #[test]
    fn lone_dollar_is_not_a_dollar_quote() {
        // `$1` is a bind placeholder, not the start of a quoted body; the
        // semicolon after it still terminates the statement.
        let out = split_statements("SELECT $1; SELECT 2");
        assert_eq!(out, vec!["SELECT $1", "SELECT 2"]);
    }

    // --- executable counting ---------------------------------------------

    #[test]
    fn comment_only_statements_do_not_count_as_executable() {
        let statements = vec![
            "-- unsupported".to_string(),
            "DROP TABLE t".to_string(),
            "  -- another".to_string(),
        ];
        assert_eq!(executable_count(&statements), 1);
        assert_eq!(executable_count(&[]), 0);
    }

    // --- diff projection --------------------------------------------------

    #[test]
    fn diff_rows_flatten_every_change() {
        let mut diff = SchemaDiff::default();
        diff.added_tables
            .push(table("public", "users", vec![column("id", "int")]));
        diff.removed_tables.push(table("public", "legacy", vec![]));

        let mut td = table_diff("public", "orders");
        td.added_columns.push(column("total", "numeric"));
        td.modified_columns.push(ColumnChange {
            name: "qty".into(),
            source: column("qty", "bigint"),
            target: column("qty", "int"),
        });
        td.removed_indexes
            .push(IndexInfo::new("idx_old".into(), vec!["qty".into()], false, false));
        diff.modified_tables.push(td);

        let result = diff_rows(&diff);
        assert_eq!(result.columns.len(), 6);
        assert_eq!(result.rows.len(), 5);

        let first = &result.rows[0];
        assert_eq!(first[0], serde_json::json!("Added"));
        assert_eq!(first[1], serde_json::json!("table"));
        assert_eq!(first[3], serde_json::json!("users"));
        assert_eq!(first[5], serde_json::json!("1 column(s)"));

        // The modified column carries the direction of the change.
        let modified = result
            .rows
            .iter()
            .find(|r| r[4] == serde_json::json!("qty"))
            .expect("qty row");
        assert_eq!(modified[0], serde_json::json!("Modified"));
        assert_eq!(modified[5], serde_json::json!("int -> bigint"));
    }

    #[test]
    fn empty_diff_projects_to_no_rows() {
        let result = diff_rows(&SchemaDiff::default());
        assert!(result.rows.is_empty());
        assert_eq!(result.columns.len(), 6);
    }

    // --- outcome formatting ----------------------------------------------

    #[test]
    fn apply_summary_reports_success_partial_and_noop() {
        let ok = ApplyResult {
            executed: 4,
            succeeded: 4,
            failed_statement: None,
            error: None,
        };
        assert_eq!(apply_summary(&ok), "Applied 4 statement(s)");

        let partial = ApplyResult {
            executed: 4,
            succeeded: 2,
            failed_statement: Some("DROP TABLE t".into()),
            error: Some("permission denied".into()),
        };
        assert_eq!(
            apply_summary(&partial),
            "Failed after 2/4 statement(s): permission denied"
        );

        let nothing = ApplyResult::default();
        assert_eq!(
            apply_summary(&nothing),
            "Nothing to apply: every statement was a comment"
        );
    }

    #[test]
    fn truncate_collapses_whitespace_and_elides() {
        assert_eq!(truncate("a\n  b", 10), "a b");
        assert_eq!(truncate("abcdefghij", 5), "abcde…");
    }

    // --- wizard state -----------------------------------------------------

    #[test]
    fn reset_clears_derived_state_but_keeps_choices() {
        let mut m = Migrations::default();
        m.source_id = Some("1".into());
        m.target_id = Some("2".into());
        m.schema = "app".into();
        m.step = Step::Sql;
        m.diff = Some(SchemaDiff::default());
        m.generated = vec!["DROP TABLE t".into()];
        m.sql = "DROP TABLE t;".into();
        m.acknowledged = true;
        m.confirming = true;
        m.outcome = Some(ApplyResult::default());

        m.reset();

        assert_eq!(m.step, Step::Select);
        assert!(m.diff.is_none());
        assert!(m.generated.is_empty());
        assert!(m.sql.is_empty());
        assert!(!m.acknowledged);
        assert!(!m.confirming);
        assert!(m.outcome.is_none());
        // Choices survive, so reopening after a failure is not a retype.
        assert_eq!(m.source_id.as_deref(), Some("1"));
        assert_eq!(m.target_id.as_deref(), Some("2"));
        assert_eq!(m.schema, "app");
    }

    #[test]
    fn defaults_match_the_web_shell() {
        let m = Migrations::default();
        assert_eq!(m.schema, "public");
        assert!(m.use_transaction);
        assert!(!m.acknowledged);
        assert_eq!(m.step, Step::Select);
    }
}
