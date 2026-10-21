//! Lazy row model for the results grid.
//!
//! Slint asks a model for rows by index. Building every row up front would
//! allocate one Slint struct (and a nested cell model) per row, which for a
//! 50,000 row result is exactly the retained-memory problem the web shell
//! had. This model holds the raw strings once and builds a row only when the
//! ListView asks for it, which it does only for rows on screen.

use std::rc::Rc;

use slint::{Model, ModelNotify, ModelRc, ModelTracker, VecModel};

use crate::{GridCell, GridColumn, GridRow};

/// A cell value, with NULL kept distinct from the text "NULL".
pub type Cell = Option<String>;

/// What the editing layer knows about one row beyond its text: pending edits,
/// a pending delete, a row that is not in the table yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowMark {
    pub deleted: bool,
    pub added: bool,
    /// Per cell: holds an uncommitted edit.
    pub modified: Vec<bool>,
    /// Per cell: shown as a hint ("default"), not a value.
    pub placeholder: Vec<bool>,
}

pub struct LazyRows {
    rows: Rc<Vec<Vec<Cell>>>,
    marks: Option<Rc<Vec<RowMark>>>,
    notify: ModelNotify,
}

impl LazyRows {
    pub fn new(rows: Rc<Vec<Vec<Cell>>>) -> Self {
        Self {
            rows,
            marks: None,
            notify: ModelNotify::default(),
        }
    }

    /// The same rows with their edit marks, one `RowMark` per row.
    pub fn with_marks(rows: Rc<Vec<Vec<Cell>>>, marks: Rc<Vec<RowMark>>) -> Self {
        Self {
            rows,
            marks: Some(marks),
            notify: ModelNotify::default(),
        }
    }
}

impl Model for LazyRows {
    type Data = GridRow;

    fn row_count(&self) -> usize {
        self.rows.len()
    }

    fn row_data(&self, row: usize) -> Option<GridRow> {
        let src = self.rows.get(row)?;
        let mark = self.marks.as_ref().and_then(|m| m.get(row));
        let flag = |v: Option<&Vec<bool>>, i: usize| v.and_then(|v| v.get(i)).copied().unwrap_or(false);
        let cells: Vec<GridCell> = src
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let modified = flag(mark.map(|m| &m.modified), i);
                let placeholder = flag(mark.map(|m| &m.placeholder), i);
                match c {
                    Some(t) => GridCell {
                        text: t.as_str().into(),
                        is_null: false,
                        modified,
                        placeholder,
                    },
                    None => GridCell {
                        text: "".into(),
                        is_null: true,
                        modified,
                        placeholder,
                    },
                }
            })
            .collect();
        Some(GridRow {
            cells: ModelRc::new(VecModel::from(cells)),
            deleted: mark.is_some_and(|m| m.deleted),
            added: mark.is_some_and(|m| m.added),
        })
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
}

/// Lay columns out left to right. Returns the columns and the total width.
pub fn layout_columns(titles: &[String], widths: &[f32]) -> (ModelRc<GridColumn>, f32) {
    let mut x = 0.0_f32;
    let cols: Vec<GridColumn> = titles
        .iter()
        .zip(widths)
        .map(|(t, w)| {
            let col = GridColumn {
                title: t.to_uppercase().into(),
                x,
                width: *w,
            };
            x += w;
            col
        })
        .collect();
    (ModelRc::new(VecModel::from(cols)), x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_count_matches_without_building_rows() {
        let data = Rc::new(vec![vec![Some("a".to_string())]; 50_000]);
        let m = LazyRows::new(data);
        assert_eq!(m.row_count(), 50_000);
    }

    #[test]
    fn null_is_flagged_and_distinct_from_the_text_null() {
        let data = Rc::new(vec![vec![None, Some("NULL".to_string())]]);
        let row = LazyRows::new(data).row_data(0).unwrap();
        let a = row.cells.row_data(0).unwrap();
        let b = row.cells.row_data(1).unwrap();
        assert!(a.is_null);
        assert!(!b.is_null);
        assert_eq!(b.text, "NULL");
    }

    #[test]
    fn out_of_range_rows_are_none() {
        let data = Rc::new(vec![vec![Some("a".to_string())]]);
        assert!(LazyRows::new(data).row_data(5).is_none());
    }

    #[test]
    fn columns_are_laid_out_left_to_right() {
        let (cols, total) = layout_columns(&["a".into(), "b".into()], &[100.0, 50.0]);
        assert_eq!(cols.row_data(1).unwrap().x, 100.0);
        assert_eq!(total, 150.0);
    }
}
