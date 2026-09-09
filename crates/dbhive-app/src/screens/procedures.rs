//! Stored procedures and functions.
//!
//! Parity target: the React `StoredProceduresPanel.tsx`, reached from the
//! connected sidebar's "Procedures" entry.
//!
//! The screen lists the routines the server knows about, shows one's source,
//! and runs it with arguments. All three operations live in
//! `dbhive_core::service_procedures`, which owns the per-dialect catalogue
//! SQL, the identifier quoting and the CALL/SELECT/EXEC syntax; this file
//! only draws and collects input.
//!
//! Two rules shape it, the same two as the table inspector:
//!
//! * Nothing blocks the frame. The list, the definition and the execution are
//!   three independent `Job`s and the UI draws whatever has landed.
//! * Nothing reaches SQL unquoted. Arguments are rendered as escaped literals
//!   and names are quoted by the live driver, both inside the core, so a
//!   routine called `select` or an argument containing a quote is fine.
//!
//! The statement that will run is shown before it runs. A stored procedure
//! can do anything, including delete everything, and "Execute" with no
//! visible statement is a blind click.

use dbhive_core::drivers::QueryResult;
use dbhive_core::models::DbError;
use dbhive_core::service_procedures::{self as svc, ProcedureInfo};

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::theme::Palette;
use crate::widgets::{results_grid, sql_editor};

// ---------------------------------------------------------------------------
// Pure logic
// ---------------------------------------------------------------------------

/// Split `(a integer, b numeric(10,2))` into its arguments.
///
/// The split is on top-level commas only, so a parameterised type keeps its
/// own comma instead of being torn into two arguments. Nothing here parses
/// SQL properly; it only needs to produce one input box per argument.
pub fn parse_signature(signature: &str) -> Vec<String> {
    let inner = signature.trim();
    let inner = inner.strip_prefix('(').unwrap_or(inner);
    let inner = inner.strip_suffix(')').unwrap_or(inner);
    let inner = inner.trim();
    if inner.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for ch in inner.chars() {
        match ch {
            '(' => {
                depth += 1;
                current.push(ch);
            }
            ')' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                if !current.trim().is_empty() {
                    out.push(current.trim().to_string());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

/// Read one argument box.
///
/// A number, boolean, array or object typed as JSON arrives as that type;
/// anything else is a string, which is what makes `hello` work without
/// quoting it. An empty box is NULL, since there is no other way to say so.
pub fn parse_arg(raw: &str) -> serde_json::Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return serde_json::Value::Null;
    }
    serde_json::from_str(trimmed)
        .unwrap_or_else(|_| serde_json::Value::String(trimmed.to_string()))
}

/// Whether a routine matches the search box. Name and schema both count, so
/// typing a schema narrows to it.
pub fn matches(p: &ProcedureInfo, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    p.name.to_lowercase().contains(&q) || p.schema.to_lowercase().contains(&q)
}

/// The routines matching `query`, grouped by schema, schemas in name order
/// and routines in the order the catalogue returned them (already sorted).
pub fn grouped(procedures: &[ProcedureInfo], query: &str) -> Vec<(String, Vec<ProcedureInfo>)> {
    let mut out: Vec<(String, Vec<ProcedureInfo>)> = Vec::new();
    for p in procedures.iter().filter(|p| matches(p, query)) {
        match out.iter_mut().find(|(s, _)| *s == p.schema) {
            Some((_, items)) => items.push(p.clone()),
            None => out.push((p.schema.clone(), vec![p.clone()])),
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// A one-line summary for the list row.
pub fn summary(p: &ProcedureInfo) -> String {
    match &p.return_type {
        Some(r) => format!("{} ➡ {}", p.argument_signature, r),
        None => p.argument_signature.clone(),
    }
}

// ---------------------------------------------------------------------------
// Screen state
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct Procedures {
    pub open: bool,
    pub connection_id: Option<String>,
    /// The schema to list. Empty lists every schema the driver reports.
    pub schema: String,

    pub search: String,
    pub procedures: Vec<ProcedureInfo>,
    pub selected: Option<ProcedureInfo>,
    /// Raw text of each argument box, one per parsed argument.
    pub args: Vec<String>,

    pub definition: String,
    pub definition_editor: sql_editor::EditorState,
    pub result: Option<QueryResult>,
    pub grid: results_grid::GridState,
    pub elapsed_ms: Option<u128>,
    pub last_error: Option<String>,

    pub list_job: Job<Result<Vec<ProcedureInfo>, DbError>>,
    pub definition_job: Job<Result<String, DbError>>,
    pub run_job: Job<(Result<QueryResult, DbError>, u128)>,

    /// Set when the list needs (re)loading. The load itself is started after
    /// drawing, once every borrow of the screen has ended.
    needs_list: bool,
    /// Set when the connection's driver has no stored routines at all, so the
    /// empty list is explained rather than looking like a failure.
    unsupported: bool,
}

impl Procedures {
    /// Point the screen at a connection and schema. Reopening the same target
    /// keeps the loaded list, the selection and the typed arguments.
    pub fn open_for(&mut self, connection_id: impl Into<String>, schema: impl Into<String>) {
        let (c, s) = (connection_id.into(), schema.into());
        self.open = true;
        if self.connection_id.as_deref() == Some(c.as_str()) && self.schema == s {
            return;
        }
        self.connection_id = Some(c);
        self.schema = s;
        self.procedures.clear();
        self.selected = None;
        self.args.clear();
        self.definition.clear();
        self.result = None;
        self.elapsed_ms = None;
        self.last_error = None;
        self.unsupported = false;
        self.needs_list = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn refresh(&mut self) {
        self.needs_list = true;
        self.last_error = None;
    }

    /// Select a routine: reset the argument boxes to its signature and clear
    /// the previous one's definition and result, which belong to it and not
    /// to this one.
    fn select(&mut self, p: ProcedureInfo) {
        if self.selected.as_ref() == Some(&p) {
            return;
        }
        self.args = vec![String::new(); parse_signature(&p.argument_signature).len()];
        self.definition.clear();
        self.result = None;
        self.elapsed_ms = None;
        self.grid = results_grid::GridState::default();
        self.selected = Some(p);
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    if !app.screens.procedures.open {
        return;
    }
    let Some(connection_id) = app.screens.procedures.connection_id.clone() else {
        return;
    };

    // Drain finished work before drawing, so this frame shows the newest data.
    {
        let p = &mut app.screens.procedures;
        if let Some(result) = p.list_job.take_result() {
            match result {
                Ok(list) => {
                    p.procedures = list;
                    p.last_error = None;
                }
                Err(e) => p.last_error = Some(e.to_string()),
            }
        }
        if let Some(result) = p.definition_job.take_result() {
            match result {
                Ok(text) => p.definition = text,
                // Keeping the driver's message: "PROCEDURE does not exist"
                // is the whole diagnosis when a name is overloaded.
                Err(e) => p.definition = format!("-- Could not read the definition: {}", e),
            }
        }
    }
    if let Some((result, elapsed)) = app.screens.procedures.run_job.take_result() {
        app.screens.procedures.elapsed_ms = Some(elapsed);
        match result {
            Ok(r) => {
                let rows = r.rows.len();
                app.screens.procedures.grid = results_grid::GridState::default();
                app.screens.procedures.result = Some(r);
                app.screens.procedures.last_error = None;
                app.toasts.success(format!("Executed, {} row(s)", rows));
            }
            Err(e) => {
                app.screens.procedures.result = None;
                let msg = e.to_string();
                app.screens.procedures.last_error = Some(msg.clone());
                app.toasts.error(msg);
            }
        }
    }

    header(app, ui, palette);
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);

    if let Some(err) = app.screens.procedures.last_error.clone() {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(palette.destructive, "Failed:");
            ui.label(err);
            if ui.button("Dismiss").clicked() {
                app.screens.procedures.last_error = None;
            }
        });
        ui.add_space(4.0);
    }

    if app.screens.procedures.unsupported {
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            ui.label(crate::theme::text::muted(
                palette,
                "This database has no stored procedures or functions.",
            ));
        });
        return;
    }

    ui.columns(2, |cols| {
        list_pane(app, &mut cols[0], palette);
        detail_pane(app, &mut cols[1], palette);
    });

    // Started after every borrow of the screen has ended.
    start_pending(app, ui.ctx(), &connection_id);
}

fn header(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let p = &mut app.screens.procedures;
    let busy = p.list_job.is_running();
    let count = p.procedures.len();
    let schema = p.schema.clone();

    let mut refresh = false;
    let mut close = false;

    ui.horizontal(|ui| {
        ui.label(crate::theme::text::heading(palette, "Procedures & functions"));
        if !schema.is_empty() {
            ui.label(crate::theme::text::muted(palette, schema));
        }
        if count > 0 {
            ui.label(crate::theme::text::muted(palette, format!("{}", count)));
        }
        if busy {
            ui.add(egui::Spinner::new().size(14.0));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                close = true;
            }
            if ui.add_enabled(!busy, egui::Button::new("Refresh")).clicked() {
                refresh = true;
            }
        });
    });

    ui.add_space(4.0);
    ui.add(
        egui::TextEdit::singleline(&mut p.search)
            .hint_text("Search by name or schema...")
            .desired_width(260.0),
    );

    if refresh {
        p.refresh();
    }
    if close {
        p.close();
    }
}

fn list_pane(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let (groups, selected, loading) = {
        let p = &app.screens.procedures;
        (
            grouped(&p.procedures, &p.search),
            p.selected.clone(),
            p.list_job.is_running(),
        )
    };

    if groups.is_empty() {
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            if loading {
                ui.add(egui::Spinner::new());
                ui.label(crate::theme::text::muted(palette, "Loading routines"));
            } else if app.screens.procedures.search.trim().is_empty() {
                ui.label(crate::theme::text::muted(
                    palette,
                    "No user-defined routines in this database",
                ));
            } else {
                ui.label(crate::theme::text::muted(palette, "Nothing matches"));
            }
        });
        return;
    }

    // Immediate mode: record the click, act once the list is no longer
    // borrowed.
    let mut to_select: Option<ProcedureInfo> = None;

    egui::ScrollArea::vertical()
        .id_salt("procedures_list")
        .show(ui, |ui| {
            for (schema, items) in &groups {
                ui.label(crate::theme::text::muted(palette, schema.to_uppercase()));
                for p in items {
                    let is_selected = selected.as_ref() == Some(p);
                    let label = format!("{}  {}", p.name, p.kind.label());
                    if ui.selectable_label(is_selected, label).clicked() {
                        to_select = Some(p.clone());
                    }
                    ui.label(crate::theme::text::muted(palette, summary(p)));
                    ui.add_space(2.0);
                }
                ui.add_space(6.0);
            }
        });

    if let Some(p) = to_select {
        app.screens.procedures.select(p);
    }
}

fn detail_pane(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let Some(selected) = app.screens.procedures.selected.clone() else {
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            ui.label(crate::theme::text::muted(
                palette,
                "Select a routine to see its source and run it",
            ));
        });
        return;
    };

    ui.label(crate::theme::text::strong(palette, selected.qualified()));
    ui.label(crate::theme::text::muted(
        palette,
        match &selected.language {
            Some(l) => format!("{} · {}", selected.kind.label(), l),
            None => selected.kind.label().to_string(),
        },
    ));
    ui.add_space(4.0);

    arguments(app, ui, palette, &selected);
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);
    definition(app, ui, palette);
    ui.add_space(6.0);
    result(app, ui, palette);
}

fn arguments(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette, selected: &ProcedureInfo) {
    let names = parse_signature(&selected.argument_signature);
    let running = app.screens.procedures.run_job.is_running();

    {
        let p = &mut app.screens.procedures;
        // A signature that came back later than the selection, or a list
        // refresh that changed it, must not leave stale boxes behind.
        if p.args.len() != names.len() {
            p.args.resize(names.len(), String::new());
        }
        if names.is_empty() {
            ui.label(crate::theme::text::muted(palette, "Takes no arguments"));
        }
        for (i, name) in names.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(name).monospace().color(palette.muted_foreground));
                ui.add(
                    egui::TextEdit::singleline(&mut p.args[i])
                        .hint_text("42, \"text\", true, or empty for NULL")
                        .desired_width(200.0),
                );
            });
        }
    }

    // The statement is built by the core, from the live driver's quoting, so
    // what is shown is exactly what will run.
    let args: Vec<serde_json::Value> = app
        .screens
        .procedures
        .args
        .iter()
        .map(|a| parse_arg(a))
        .collect();
    let preview = app
        .screens
        .procedures
        .connection_id
        .clone()
        .map(|id| {
            svc::preview_call(
                &app.state,
                &id,
                &selected.schema,
                &selected.name,
                selected.kind,
                &args,
            )
        })
        .unwrap_or_else(|| Err(DbError::NotFound("No connection".into())));

    ui.add_space(4.0);
    match &preview {
        Ok(sql) => {
            ui.label(crate::theme::text::muted(palette, "Statement"));
            egui::Frame::none()
                .fill(palette.card)
                .stroke(egui::Stroke::new(1.0_f32, palette.border))
                .rounding(egui::Rounding::same(6.0))
                .inner_margin(egui::Margin::same(6.0))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(sql).monospace().color(palette.foreground));
                });
        }
        Err(e) => {
            ui.colored_label(palette.destructive, e.to_string());
        }
    }

    ui.add_space(4.0);
    let mut run = false;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                preview.is_ok() && !running,
                egui::Button::new(format!("Run {}", selected.kind.label())),
            )
            .clicked()
        {
            run = true;
        }
        if running {
            ui.add(egui::Spinner::new());
        }
        if let Some(ms) = app.screens.procedures.elapsed_ms {
            ui.label(crate::theme::text::muted(palette, format!("{} ms", ms)));
        }
    });

    if run {
        start_run(app, ui.ctx(), selected, args);
    }
}

fn definition(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let loading = app.screens.procedures.definition_job.is_running();
    let empty = app.screens.procedures.definition.is_empty();

    let mut load = false;
    ui.horizontal(|ui| {
        ui.label(crate::theme::text::strong(palette, "Source"));
        if loading {
            ui.add(egui::Spinner::new().size(14.0));
        } else if ui
            .small_button(if empty { "Show" } else { "Reload" })
            .clicked()
        {
            load = true;
        }
    });

    if !empty {
        // The SQL editor, so the source is highlighted and searchable. It is
        // editable, but nothing writes it back: a routine is changed by
        // running a CREATE OR REPLACE, not by typing over the listing.
        let schema_index = sql_editor::SchemaIndex::default();
        let p = &mut app.screens.procedures;
        let mut text = std::mem::take(&mut p.definition);
        sql_editor::show(ui, palette, &mut p.definition_editor, &schema_index, &mut text);
        p.definition = text;
    }

    if load {
        start_definition(app, ui.ctx());
    }
}

fn result(app: &mut DbHiveApp, ui: &mut egui::Ui, palette: &Palette) {
    let p = &mut app.screens.procedures;
    let Some(result) = p.result.take() else {
        return;
    };
    ui.label(crate::theme::text::strong(palette, "Result"));
    results_grid::show(ui, palette, &result, &mut p.grid);
    p.result = Some(result);
}

// ---------------------------------------------------------------------------
// Async work
// ---------------------------------------------------------------------------

fn start_pending(app: &mut DbHiveApp, ctx: &egui::Context, connection_id: &str) {
    if !app.screens.procedures.needs_list || app.screens.procedures.list_job.is_running() {
        return;
    }
    app.screens.procedures.needs_list = false;

    // A driver with no routines is answered here rather than with an empty
    // list that looks like a failed load.
    let supported = {
        let guard = app.state.lock().unwrap();
        match guard.get_profile(connection_id) {
            Some(profile) => svc::supports_routines(&profile.driver),
            None => {
                drop(guard);
                app.screens.procedures.last_error = Some("Connection is no longer open".into());
                return;
            }
        }
    };
    app.screens.procedures.unsupported = !supported;
    if !supported {
        app.screens.procedures.procedures.clear();
        return;
    }

    let state = app.state.clone();
    let id = connection_id.to_string();
    let schema = app.screens.procedures.schema.clone();
    let handle = app.rt.handle().clone();
    app.screens
        .procedures
        .list_job
        .start(&handle, ctx, async move {
            let schema = if schema.trim().is_empty() {
                None
            } else {
                Some(schema)
            };
            svc::list_procedures(&state, &id, schema.as_deref()).await
        });
}

fn start_definition(app: &mut DbHiveApp, ctx: &egui::Context) {
    let (Some(id), Some(selected)) = (
        app.screens.procedures.connection_id.clone(),
        app.screens.procedures.selected.clone(),
    ) else {
        return;
    };
    let state = app.state.clone();
    let handle = app.rt.handle().clone();
    app.screens
        .procedures
        .definition_job
        .start(&handle, ctx, async move {
            svc::get_procedure_definition(
                &state,
                &id,
                &selected.schema,
                &selected.name,
                Some(selected.kind),
            )
            .await
        });
}

fn start_run(
    app: &mut DbHiveApp,
    ctx: &egui::Context,
    selected: &ProcedureInfo,
    args: Vec<serde_json::Value>,
) {
    let Some(id) = app.screens.procedures.connection_id.clone() else {
        return;
    };
    let state = app.state.clone();
    let handle = app.rt.handle().clone();
    let (schema, name, kind) = (
        selected.schema.clone(),
        selected.name.clone(),
        selected.kind,
    );
    app.screens
        .procedures
        .run_job
        .start(&handle, ctx, async move {
            let started = std::time::Instant::now();
            let out = svc::execute_procedure(&state, &id, &schema, &name, kind, &args).await;
            (out, started.elapsed().as_millis())
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbhive_core::service_procedures::RoutineKind;

    fn proc(schema: &str, name: &str, kind: RoutineKind, sig: &str) -> ProcedureInfo {
        ProcedureInfo {
            schema: schema.into(),
            name: name.into(),
            kind,
            language: None,
            return_type: None,
            argument_signature: sig.into(),
        }
    }

    // ── signature parsing ────────────────────────────────────────────────

    #[test]
    fn a_routine_with_no_arguments_has_no_boxes() {
        assert!(parse_signature("()").is_empty());
        assert!(parse_signature("").is_empty());
        assert!(parse_signature("(   )").is_empty());
    }

    #[test]
    fn arguments_split_on_top_level_commas() {
        assert_eq!(
            parse_signature("(a integer, b text)"),
            vec!["a integer".to_string(), "b text".to_string()]
        );
    }

    #[test]
    fn a_parameterised_type_keeps_its_own_comma() {
        // numeric(10,2) is one argument, not two.
        assert_eq!(
            parse_signature("(amount numeric(10,2), note text)"),
            vec!["amount numeric(10,2)".to_string(), "note text".to_string()]
        );
    }

    #[test]
    fn an_unparenthesised_signature_still_parses() {
        assert_eq!(parse_signature("a integer"), vec!["a integer".to_string()]);
    }

    #[test]
    fn a_trailing_comma_does_not_add_an_empty_argument() {
        assert_eq!(parse_signature("(a integer, )"), vec!["a integer".to_string()]);
    }

    // ── argument parsing ─────────────────────────────────────────────────

    #[test]
    fn an_empty_box_is_null() {
        assert_eq!(parse_arg(""), serde_json::Value::Null);
        assert_eq!(parse_arg("   "), serde_json::Value::Null);
    }

    #[test]
    fn json_scalars_keep_their_type() {
        assert_eq!(parse_arg("42"), serde_json::json!(42));
        assert_eq!(parse_arg("1.5"), serde_json::json!(1.5));
        assert_eq!(parse_arg("true"), serde_json::json!(true));
        assert_eq!(parse_arg("\"hi\""), serde_json::json!("hi"));
        assert_eq!(parse_arg("[1,2]"), serde_json::json!([1, 2]));
        assert_eq!(parse_arg("{\"a\":1}"), serde_json::json!({"a": 1}));
    }

    #[test]
    fn unquoted_text_is_a_string_not_an_error() {
        assert_eq!(parse_arg("hello"), serde_json::json!("hello"));
        assert_eq!(parse_arg("2024-01-01"), serde_json::json!("2024-01-01"));
        // Including text that would end a SQL literal, which the core escapes
        // when it renders the argument.
        assert_eq!(parse_arg("o'brien"), serde_json::json!("o'brien"));
    }

    /// The whole point of `parse_arg`: what the boxes produce has to survive
    /// the core's rendering as a literal.
    #[test]
    fn parsed_arguments_render_as_safe_literals() {
        let escape = |s: &str| s.replace('\'', "''");
        assert_eq!(svc::render_arg(&parse_arg(""), &escape), "NULL");
        assert_eq!(svc::render_arg(&parse_arg("42"), &escape), "42");
        assert_eq!(
            svc::render_arg(&parse_arg("o'brien"), &escape),
            "'o''brien'"
        );
    }

    // ── filtering and grouping ───────────────────────────────────────────

    #[test]
    fn an_empty_search_matches_everything() {
        let p = proc("public", "add", RoutineKind::Function, "()");
        assert!(matches(&p, ""));
        assert!(matches(&p, "   "));
    }

    #[test]
    fn search_covers_name_and_schema_case_insensitively() {
        let p = proc("Billing", "chargeCard", RoutineKind::Procedure, "()");
        assert!(matches(&p, "charge"));
        assert!(matches(&p, "CHARGECARD"));
        assert!(matches(&p, "billing"));
        assert!(!matches(&p, "refund"));
    }

    #[test]
    fn routines_group_by_schema_in_name_order() {
        let list = vec![
            proc("public", "b", RoutineKind::Function, "()"),
            proc("audit", "a", RoutineKind::Procedure, "()"),
            proc("public", "a", RoutineKind::Function, "()"),
        ];
        let groups = grouped(&list, "");
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, "audit");
        assert_eq!(groups[1].0, "public");
        // Within a schema the catalogue's order is kept.
        assert_eq!(groups[1].1.len(), 2);
        assert_eq!(groups[1].1[0].name, "b");
    }

    #[test]
    fn grouping_applies_the_search() {
        let list = vec![
            proc("public", "add", RoutineKind::Function, "()"),
            proc("audit", "log", RoutineKind::Procedure, "()"),
        ];
        let groups = grouped(&list, "add");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].1[0].name, "add");
    }

    #[test]
    fn the_summary_shows_the_return_type_when_there_is_one() {
        let mut p = proc("public", "add", RoutineKind::Function, "(a int)");
        assert_eq!(summary(&p), "(a int)");
        p.return_type = Some("integer".into());
        assert_eq!(summary(&p), "(a int) ➡ integer");
    }

    // ── selection ────────────────────────────────────────────────────────

    #[test]
    fn selecting_a_routine_sizes_the_argument_boxes() {
        let mut screen = Procedures::default();
        screen.select(proc(
            "public",
            "add",
            RoutineKind::Function,
            "(a integer, b integer)",
        ));
        assert_eq!(screen.args.len(), 2);
    }

    #[test]
    fn selecting_another_routine_discards_the_previous_answers() {
        let mut screen = Procedures::default();
        screen.select(proc("public", "add", RoutineKind::Function, "(a integer)"));
        screen.args[0] = "42".into();
        screen.definition = "CREATE FUNCTION add ...".into();

        screen.select(proc("public", "sub", RoutineKind::Function, "(a int, b int)"));
        assert_eq!(screen.args, vec![String::new(), String::new()]);
        assert!(
            screen.definition.is_empty(),
            "the previous routine's source stayed on screen"
        );
    }

    #[test]
    fn reselecting_the_same_routine_keeps_the_typed_arguments() {
        let mut screen = Procedures::default();
        let p = proc("public", "add", RoutineKind::Function, "(a integer)");
        screen.select(p.clone());
        screen.args[0] = "42".into();
        screen.select(p);
        assert_eq!(screen.args[0], "42");
    }

    #[test]
    fn reopening_the_same_target_keeps_the_loaded_list() {
        let mut screen = Procedures::default();
        screen.open_for("conn", "public");
        screen.procedures = vec![proc("public", "add", RoutineKind::Function, "()")];
        screen.open_for("conn", "public");
        assert_eq!(screen.procedures.len(), 1, "a no-op reopen threw the list away");

        // A different schema is a different list, so it does reset.
        screen.open_for("conn", "audit");
        assert!(screen.procedures.is_empty());
    }
}
