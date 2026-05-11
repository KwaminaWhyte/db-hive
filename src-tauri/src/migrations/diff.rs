//! Schema diff data structures and algorithm.
//!
//! Pure functions — no IO. Tables are matched by `(schema, name)`; columns,
//! indexes, and foreign keys are matched by name (case-sensitive).

use serde::{Deserialize, Serialize};

use crate::models::metadata::{ColumnInfo, ForeignKeyInfo, IndexInfo, TableSchema};

/// Column-level modification reported in `TableDiff::modified_columns`.
///
/// Holds both source (desired) and target (current) column definitions so
/// downstream SQL generators can decide which ALTER operations to emit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnChange {
    pub name: String,
    pub source: ColumnInfo,
    pub target: ColumnInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexDiff {
    pub added: Vec<IndexInfo>,
    pub removed: Vec<IndexInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKeyDiff {
    pub added: Vec<ForeignKeyInfo>,
    pub removed: Vec<ForeignKeyInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableDiff {
    pub schema: String,
    pub name: String,
    pub added_columns: Vec<ColumnInfo>,
    pub removed_columns: Vec<ColumnInfo>,
    pub modified_columns: Vec<ColumnChange>,
    pub added_indexes: Vec<IndexInfo>,
    pub removed_indexes: Vec<IndexInfo>,
    pub added_fks: Vec<ForeignKeyInfo>,
    pub removed_fks: Vec<ForeignKeyInfo>,
}

impl TableDiff {
    pub fn is_empty(&self) -> bool {
        self.added_columns.is_empty()
            && self.removed_columns.is_empty()
            && self.modified_columns.is_empty()
            && self.added_indexes.is_empty()
            && self.removed_indexes.is_empty()
            && self.added_fks.is_empty()
            && self.removed_fks.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SchemaDiff {
    pub added_tables: Vec<TableSchema>,
    pub removed_tables: Vec<TableSchema>,
    pub modified_tables: Vec<TableDiff>,
}

/// Pair of schemas passed into `compute_diff`. Each element corresponds to one
/// table with its columns, indexes, and foreign keys populated.
#[derive(Debug, Clone)]
pub struct TableWithFks {
    pub schema: TableSchema,
    pub foreign_keys: Vec<ForeignKeyInfo>,
}

fn key(ts: &TableSchema) -> (String, String) {
    (ts.table.schema.clone(), ts.table.name.clone())
}

/// Compute a structured diff between `source` (desired) and `target` (current).
///
/// Applying the resulting SQL to `target` should produce `source`.
pub fn compute_diff(source: &[TableWithFks], target: &[TableWithFks]) -> SchemaDiff {
    let mut diff = SchemaDiff::default();

    for src in source {
        match target.iter().find(|t| key(&t.schema) == key(&src.schema)) {
            None => diff.added_tables.push(src.schema.clone()),
            Some(tgt) => {
                let td = diff_table(src, tgt);
                if !td.is_empty() {
                    diff.modified_tables.push(td);
                }
            }
        }
    }

    for tgt in target {
        if !source.iter().any(|s| key(&s.schema) == key(&tgt.schema)) {
            diff.removed_tables.push(tgt.schema.clone());
        }
    }

    diff
}

fn diff_table(src: &TableWithFks, tgt: &TableWithFks) -> TableDiff {
    let mut added_columns = Vec::new();
    let mut modified_columns = Vec::new();
    for sc in &src.schema.columns {
        match tgt.schema.columns.iter().find(|c| c.name == sc.name) {
            None => added_columns.push(sc.clone()),
            Some(tc) => {
                if !columns_equal(sc, tc) {
                    modified_columns.push(ColumnChange {
                        name: sc.name.clone(),
                        source: sc.clone(),
                        target: tc.clone(),
                    });
                }
            }
        }
    }

    let removed_columns: Vec<ColumnInfo> = tgt
        .schema
        .columns
        .iter()
        .filter(|tc| !src.schema.columns.iter().any(|sc| sc.name == tc.name))
        .cloned()
        .collect();

    let added_indexes: Vec<IndexInfo> = src
        .schema
        .indexes
        .iter()
        .filter(|si| !si.is_primary)
        .filter(|si| !tgt.schema.indexes.iter().any(|ti| ti.name == si.name))
        .cloned()
        .collect();
    let removed_indexes: Vec<IndexInfo> = tgt
        .schema
        .indexes
        .iter()
        .filter(|ti| !ti.is_primary)
        .filter(|ti| !src.schema.indexes.iter().any(|si| si.name == ti.name))
        .cloned()
        .collect();

    let added_fks: Vec<ForeignKeyInfo> = src
        .foreign_keys
        .iter()
        .filter(|sf| !tgt.foreign_keys.iter().any(|tf| tf.name == sf.name))
        .cloned()
        .collect();
    let removed_fks: Vec<ForeignKeyInfo> = tgt
        .foreign_keys
        .iter()
        .filter(|tf| !src.foreign_keys.iter().any(|sf| sf.name == tf.name))
        .cloned()
        .collect();

    TableDiff {
        schema: src.schema.table.schema.clone(),
        name: src.schema.table.name.clone(),
        added_columns,
        removed_columns,
        modified_columns,
        added_indexes,
        removed_indexes,
        added_fks,
        removed_fks,
    }
}

fn columns_equal(a: &ColumnInfo, b: &ColumnInfo) -> bool {
    a.data_type.eq_ignore_ascii_case(&b.data_type)
        && a.nullable == b.nullable
        && a.default_value == b.default_value
        && a.is_primary_key == b.is_primary_key
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::metadata::TableInfo;

    fn mk(name: &str, cols: Vec<ColumnInfo>) -> TableWithFks {
        TableWithFks {
            schema: TableSchema::new(
                TableInfo::new(name.to_string(), "public".to_string(), "TABLE".to_string()),
                cols,
                vec![],
            ),
            foreign_keys: vec![],
        }
    }

    #[test]
    fn detects_added_and_removed_tables() {
        let src = vec![mk("a", vec![]), mk("b", vec![])];
        let tgt = vec![mk("b", vec![]), mk("c", vec![])];
        let d = compute_diff(&src, &tgt);
        assert_eq!(d.added_tables.len(), 1);
        assert_eq!(d.added_tables[0].table.name, "a");
        assert_eq!(d.removed_tables.len(), 1);
        assert_eq!(d.removed_tables[0].table.name, "c");
    }

    #[test]
    fn detects_column_changes() {
        let src = vec![mk(
            "a",
            vec![ColumnInfo::new("id".into(), "INTEGER".into(), false)],
        )];
        let tgt = vec![mk(
            "a",
            vec![ColumnInfo::new("id".into(), "BIGINT".into(), true)],
        )];
        let d = compute_diff(&src, &tgt);
        assert_eq!(d.modified_tables.len(), 1);
        assert_eq!(d.modified_tables[0].modified_columns.len(), 1);
    }
}
