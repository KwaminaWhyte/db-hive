//! Lazy line model for the results area's JSON and Text views.
//!
//! A 2,000 row JSON view is about 8,000 lines. The view only draws the lines
//! on screen, so this model keeps the plain strings once and colours a line
//! (splits it into runs) when Slint asks for it, like `LazyRows` does for the
//! grid.

use std::rc::Rc;

use dbhive_ui_model::result_export::{json_runs, JsonKind};
use slint::{Model, ModelNotify, ModelRc, ModelTracker, VecModel};

use crate::{ResultViewLine, ResultViewRun};

/// How a line is coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStyle {
    /// Pretty JSON: keys, strings, numbers, literals and punctuation.
    Json,
    /// Aligned text: the first line is a header, the rest plain.
    Text,
}

pub struct LazyViewLines {
    lines: Rc<Vec<String>>,
    style: LineStyle,
    notify: ModelNotify,
}

impl LazyViewLines {
    pub fn new(lines: Rc<Vec<String>>, style: LineStyle) -> Self {
        Self {
            lines,
            style,
            notify: ModelNotify::default(),
        }
    }
}

fn run(text: &str, kind: i32, col: i32) -> ResultViewRun {
    ResultViewRun {
        text: text.into(),
        kind,
        col,
    }
}

impl Model for LazyViewLines {
    type Data = ResultViewLine;

    fn row_count(&self) -> usize {
        self.lines.len()
    }

    fn row_data(&self, row: usize) -> Option<ResultViewLine> {
        let line = self.lines.get(row)?;
        let runs: Vec<ResultViewRun> = match self.style {
            LineStyle::Json => {
                let mut col = 0i32;
                json_runs(line)
                    .into_iter()
                    .map(|(kind, text)| {
                        let r = run(&text, kind as i32, col);
                        // Columns count characters, not bytes.
                        col += text.chars().count() as i32;
                        r
                    })
                    .collect()
            }
            LineStyle::Text if row == 0 => vec![run(line, JsonKind::Key as i32, 0)],
            LineStyle::Text => vec![run(line, JsonKind::Plain as i32, 0)],
        };
        Some(ResultViewLine {
            runs: ModelRc::new(VecModel::from(runs)),
        })
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
}

/// An empty model, for when a view has nothing to draw.
pub fn no_lines() -> ModelRc<ResultViewLine> {
    ModelRc::new(VecModel::from(Vec::<ResultViewLine>::new()))
}

pub fn lines_model(lines: Vec<String>, style: LineStyle) -> ModelRc<ResultViewLine> {
    ModelRc::new(LazyViewLines::new(Rc::new(lines), style))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(l: &ResultViewLine) -> Vec<(String, i32, i32)> {
        l.runs
            .iter()
            .map(|r| (r.text.to_string(), r.kind, r.col))
            .collect()
    }

    #[test]
    fn json_lines_are_coloured_lazily_with_character_columns() {
        let m = LazyViewLines::new(Rc::new(vec!["  \"é\": 1,".to_string()]), LineStyle::Json);
        assert_eq!(m.row_count(), 1);
        let l = m.row_data(0).unwrap();
        let t = texts(&l);
        assert_eq!(t[0], ("  ".into(), 0, 0));
        assert_eq!(t[1], ("\"é\"".into(), 1, 2));
        // The colon starts after the three-character key: columns are chars.
        assert_eq!(t[2], (":".into(), 6, 5));
        assert_eq!(t.last().unwrap(), &(",".to_string(), 6, 8));
    }

    #[test]
    fn text_view_marks_only_the_header() {
        let m = LazyViewLines::new(
            Rc::new(vec!["id  name".into(), "1   a".into()]),
            LineStyle::Text,
        );
        assert_eq!(
            texts(&m.row_data(0).unwrap()),
            vec![("id  name".to_string(), 1, 0)]
        );
        assert_eq!(
            texts(&m.row_data(1).unwrap()),
            vec![("1   a".to_string(), 0, 0)]
        );
        assert!(m.row_data(2).is_none());
    }
}
