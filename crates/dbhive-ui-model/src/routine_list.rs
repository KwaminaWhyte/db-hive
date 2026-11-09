//! The schema browser's "Procedures & Functions" list: flat rows to draw
//! (a header per schema, then its routines) and which routine a row is.
//!
//! The catalogue, the search match and the grouping are
//! `dbhive_core::service_procedures` and `crate::procedures`; this only
//! flattens them for a list that cannot hold a tree, and decides which of the
//! panel's states is showing.

use dbhive_core::service_procedures::{ProcedureInfo, RoutineKind};

use crate::procedures::grouped;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutineRow {
    /// A schema heading rather than a routine.
    pub header: bool,
    pub label: String,
    /// The signature and return type, or for a header the routine count.
    pub detail: String,
    /// "function" or "procedure"; empty on a header.
    pub kind: &'static str,
    pub selected: bool,
}

/// Which of the panel's states is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutineStatus {
    Loading,
    Error,
    /// The driver has no stored routines at all.
    Unsupported,
    /// The schema has none.
    Empty,
    /// A search matched none.
    NoMatches,
    Populated,
}

pub fn status(
    loading: bool,
    error: &str,
    unsupported: bool,
    all: &[ProcedureInfo],
    query: &str,
) -> RoutineStatus {
    if unsupported {
        RoutineStatus::Unsupported
    } else if loading && all.is_empty() {
        RoutineStatus::Loading
    } else if !error.is_empty() && all.is_empty() {
        RoutineStatus::Error
    } else if all.is_empty() {
        RoutineStatus::Empty
    } else if grouped(all, query).is_empty() {
        RoutineStatus::NoMatches
    } else {
        RoutineStatus::Populated
    }
}

/// `(a integer, b integer) returns integer`. The routine summary uses
/// an arrow glyph the Slint font does not carry, so this spells it out.
pub fn signature_line(p: &ProcedureInfo) -> String {
    match &p.return_type {
        Some(r) => format!("{} returns {}", p.argument_signature, r),
        None => p.argument_signature.clone(),
    }
}

/// The rows for `all` under `query`, and for each row the routine it stands
/// for (`None` on a header). Both vectors have one entry per row.
pub fn rows(
    all: &[ProcedureInfo],
    query: &str,
    selected: Option<&ProcedureInfo>,
) -> (Vec<RoutineRow>, Vec<Option<ProcedureInfo>>) {
    let mut rows = Vec::new();
    let mut items = Vec::new();
    for (schema, list) in grouped(all, query) {
        rows.push(RoutineRow {
            header: true,
            label: if schema.is_empty() {
                "(default)".to_string()
            } else {
                schema
            },
            detail: list.len().to_string(),
            kind: "",
            selected: false,
        });
        items.push(None);
        for p in list {
            rows.push(RoutineRow {
                header: false,
                label: p.name.clone(),
                detail: signature_line(&p),
                kind: match p.kind {
                    RoutineKind::Procedure => "procedure",
                    RoutineKind::Function => "function",
                },
                selected: selected == Some(&p),
            });
            items.push(Some(p));
        }
    }
    (rows, items)
}

/// The text a "Copy" or "Open in editor" gives for a definition: it as read,
/// ending in a semicolon so it can be run as it stands.
pub fn definition_for_editor(definition: &str) -> String {
    let d = definition.trim_end();
    if d.is_empty() || d.ends_with(';') {
        d.to_string()
    } else {
        format!("{d};")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(
        schema: &str,
        name: &str,
        kind: RoutineKind,
        sig: &str,
        ret: Option<&str>,
    ) -> ProcedureInfo {
        ProcedureInfo {
            schema: schema.into(),
            name: name.into(),
            kind,
            language: Some("plpgsql".into()),
            return_type: ret.map(str::to_string),
            argument_signature: sig.into(),
        }
    }

    fn sample() -> Vec<ProcedureInfo> {
        vec![
            p("billing", "close_month", RoutineKind::Procedure, "()", None),
            p(
                "public",
                "add_numbers",
                RoutineKind::Function,
                "(a integer, b integer)",
                Some("integer"),
            ),
            p(
                "public",
                "slugify",
                RoutineKind::Function,
                "(t text)",
                Some("text"),
            ),
        ]
    }

    #[test]
    fn each_schema_gets_a_header_with_its_count() {
        let (rows, items) = rows(&sample(), "", None);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(
            labels,
            ["billing", "close_month", "public", "add_numbers", "slugify"]
        );
        assert!(rows[0].header && rows[2].header && !rows[1].header);
        assert_eq!(rows[0].detail, "1");
        assert_eq!(rows[2].detail, "2");
        assert_eq!(items.len(), rows.len());
        assert!(items[0].is_none() && items[1].is_some());
    }

    #[test]
    fn a_row_says_what_it_takes_and_returns() {
        let (rows, _) = rows(&sample(), "", None);
        assert_eq!(rows[3].detail, "(a integer, b integer) returns integer");
        assert_eq!(rows[3].kind, "function");
        assert_eq!(rows[1].kind, "procedure");
        assert_eq!(rows[1].detail, "()");
    }

    #[test]
    fn search_keeps_matching_routines_under_their_headers() {
        let (rows, _) = rows(&sample(), "slug", None);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, ["public", "slugify"]);
        assert_eq!(rows[0].detail, "1");
    }

    #[test]
    fn search_matches_a_schema_name_too() {
        let (rows, _) = rows(&sample(), "billing", None);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].label, "close_month");
    }

    #[test]
    fn the_selected_routine_is_marked() {
        let all = sample();
        let (rows, _) = rows(&all, "", Some(&all[1]));
        let marked: Vec<&str> = rows
            .iter()
            .filter(|r| r.selected)
            .map(|r| r.label.as_str())
            .collect();
        assert_eq!(marked, ["add_numbers"]);
    }

    #[test]
    fn rows_map_back_to_their_routine() {
        let all = sample();
        let (rows, items) = rows(&all, "", None);
        let at = rows.iter().position(|r| r.label == "slugify").unwrap();
        assert_eq!(items[at].as_ref(), Some(&all[2]));
    }

    #[test]
    fn the_panel_states() {
        let all = sample();
        assert_eq!(status(true, "", false, &[], ""), RoutineStatus::Loading);
        assert_eq!(status(false, "boom", false, &[], ""), RoutineStatus::Error);
        assert_eq!(status(false, "", true, &[], ""), RoutineStatus::Unsupported);
        assert_eq!(status(false, "", false, &[], ""), RoutineStatus::Empty);
        assert_eq!(
            status(false, "", false, &all, "zzz"),
            RoutineStatus::NoMatches
        );
        assert_eq!(status(false, "", false, &all, ""), RoutineStatus::Populated);
        // A refresh keeps showing what it has while it reloads or fails.
        assert_eq!(status(true, "", false, &all, ""), RoutineStatus::Populated);
        assert_eq!(
            status(false, "boom", false, &all, ""),
            RoutineStatus::Populated
        );
    }

    #[test]
    fn an_unsupported_driver_wins_over_everything() {
        assert_eq!(
            status(true, "x", true, &sample(), ""),
            RoutineStatus::Unsupported
        );
    }

    #[test]
    fn a_definition_for_the_editor_ends_in_a_semicolon_once() {
        assert_eq!(
            definition_for_editor("CREATE FUNCTION f() ...\n"),
            "CREATE FUNCTION f() ...;"
        );
        assert_eq!(
            definition_for_editor("CREATE FUNCTION f() ...;"),
            "CREATE FUNCTION f() ...;"
        );
        assert_eq!(definition_for_editor(""), "");
    }
}
