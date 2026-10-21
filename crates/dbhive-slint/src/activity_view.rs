//! Maps the Activity Monitor's plain view types
//! (`dbhive_ui_model::activity_panel`) to the Slint structs the panel draws.
//!
//! Kept apart from the controller so the snapshot tool builds the same models
//! without a controller behind them.

use std::rc::Rc;

use dbhive_core::drivers::QueryResult;
use dbhive_ui_model::activity_monitor::StatusTone;
use dbhive_ui_model::activity_panel::{BarRow, ChartView, LogDetail, Tile};
use slint::{ModelRc, VecModel};

use crate::grid_model::{layout_columns, LazyRows};
use crate::workspace_model::to_grid_rows;
use crate::{
    ChartLine, GridColumn, GridRow, MonitorBar, MonitorChart, MonitorChip, MonitorFact,
    MonitorTile, Tone,
};

/// The palette role a status is drawn in, as the kit's `Tone`.
pub fn tone(t: StatusTone) -> Tone {
    match t {
        StatusTone::Success => Tone::Success,
        StatusTone::Destructive => Tone::Danger,
        StatusTone::Primary => Tone::Accent,
        StatusTone::Muted => Tone::Neutral,
    }
}

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

pub fn tiles(items: &[Tile]) -> ModelRc<MonitorTile> {
    model(
        items
            .iter()
            .map(|t| MonitorTile {
                label: t.label.into(),
                value: t.value.as_str().into(),
                tone: tone(t.tone),
            })
            .collect(),
    )
}

pub fn bars(items: &[BarRow]) -> ModelRc<MonitorBar> {
    model(
        items
            .iter()
            .map(|b| MonitorBar {
                label: b.label.as_str().into(),
                value: format!("{}  {}", b.count, b.percent_label).into(),
                fraction: b.fraction,
                tone: tone(b.tone),
            })
            .collect(),
    )
}

pub fn facts(detail: &LogDetail) -> ModelRc<MonitorFact> {
    model(
        detail
            .facts
            .iter()
            .map(|(k, v)| MonitorFact {
                label: k.as_str().into(),
                value: v.as_str().into(),
            })
            .collect(),
    )
}

pub fn chips(items: &[(&str, bool)]) -> ModelRc<MonitorChip> {
    model(
        items
            .iter()
            .map(|(label, selected)| MonitorChip {
                label: (*label).into(),
                selected: *selected,
            })
            .collect(),
    )
}

/// A chart with nothing to draw: the kit shows its "collecting" message.
pub fn empty_chart() -> MonitorChart {
    MonitorChart {
        lines: model(Vec::<ChartLine>::new()),
        y_top: "".into(),
        y_mid: "".into(),
        x_start: "".into(),
        x_end: "".into(),
    }
}

pub fn chart(view: Option<&ChartView>) -> MonitorChart {
    let Some(v) = view else { return empty_chart() };
    MonitorChart {
        lines: model(
            v.lines
                .iter()
                .map(|l| ChartLine {
                    name: l.name.into(),
                    commands: l.commands.as_str().into(),
                    area: l.area.as_str().into(),
                    last: l.last.as_str().into(),
                    tone: tone(l.tone),
                })
                .collect(),
        ),
        y_top: v.y_top.as_str().into(),
        y_mid: v.y_mid.as_str().into(),
        x_start: v.x_start.as_str().into(),
        x_end: v.x_end.as_str().into(),
    }
}

/// A grid's columns, rows and total width. Rows go through the lazy model the
/// results grid expects, so a page of 250 logs costs no more than a 50k row
/// result.
pub fn grid(
    result: &QueryResult,
    widths: &[f32],
) -> (ModelRc<GridColumn>, ModelRc<GridRow>, f32) {
    let (columns, total) = layout_columns(&result.columns, widths);
    let rows = ModelRc::new(LazyRows::new(Rc::new(to_grid_rows(result))));
    (columns, rows, total)
}
