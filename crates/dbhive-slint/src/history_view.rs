//! Maps the History / Snippets / Templates panel's plain Rust view types
//! (`dbhive_ui_model::history_panel`) to the Slint structs the panel draws.
//!
//! Kept apart from the controller so the snapshot tool can build the same
//! models without a controller behind them.

use dbhive_ui_model::history::{ParamType, TemplateParameter};
use dbhive_ui_model::history_panel::{RowView, RunField};
use slint::{ModelRc, SharedString, VecModel};

use crate::{FormParam, PanelRow, RunParam};

pub fn to_row(r: RowView) -> PanelRow {
    PanelRow {
        id: r.id.into(),
        title: r.title.into(),
        subtitle: r.subtitle.into(),
        note: r.note.into(),
        tags: strings_model(&r.tags),
        has_status: r.status.is_some(),
        ok: r.status.unwrap_or(true),
    }
}

pub fn rows_model(rows: Vec<RowView>) -> ModelRc<PanelRow> {
    ModelRc::new(VecModel::from(
        rows.into_iter().map(to_row).collect::<Vec<_>>(),
    ))
}

pub fn strings_model(items: &[String]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        items
            .iter()
            .map(|s| SharedString::from(s.as_str()))
            .collect::<Vec<_>>(),
    ))
}

/// "1 snippet", "3 snippets".
pub fn count_label(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", n, if n == 1 { one } else { many })
}

/// The run form's inputs. A boolean gets a true/false picker; a date shows
/// the format it expects.
pub fn run_params(fields: Vec<RunField>) -> ModelRc<RunParam> {
    ModelRc::new(VecModel::from(
        fields
            .into_iter()
            .map(|f| RunParam {
                label: f.label.into(),
                description: f.description.into(),
                type_label: f.param_type.as_str().into(),
                required: f.required,
                kind: if f.param_type == ParamType::Boolean {
                    1
                } else {
                    0
                },
                placeholder: match f.param_type {
                    ParamType::Date => "YYYY-MM-DD",
                    ParamType::Datetime => "YYYY-MM-DD HH:MM:SS",
                    _ => "",
                }
                .into(),
                initial: f.initial.into(),
            })
            .collect::<Vec<_>>(),
    ))
}

pub fn form_params(params: &[TemplateParameter]) -> ModelRc<FormParam> {
    ModelRc::new(VecModel::from(
        params
            .iter()
            .map(|d| FormParam {
                name: d.name.as_str().into(),
                type_label: d.param_type.as_str().into(),
                required: d.required,
                default_value: d.default_value.clone().unwrap_or_default().into(),
            })
            .collect::<Vec<_>>(),
    ))
}
