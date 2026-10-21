//! Editing rows of a table: pending changes and the SQL they become.
//!
//! Nothing here touches a database. The grid records what the user typed as
//! pending changes; [`EditSession::statements`] turns them into INSERT, UPDATE
//! and DELETE statements for the shell to run in one transaction.
//!
//! Safety rules, each with a test:
//! - A row is identified by its primary key, so a table without one cannot have
//!   rows updated or deleted (inserting is still fine).
//! - Primary key cells of existing rows cannot be edited.
//! - What the user types is coerced to the column's type; a value that does not
//!   fit is refused at once, with a reason, rather than sent to the server.
//! - Every identifier goes through the quoter and every literal through it as
//!   well; user text is never concatenated into SQL.
//! - A change that puts the original value back removes itself.
//! - Changes are keyed by primary key, not by row position, so they survive
//!   paging, sorting and filtering.

use std::collections::BTreeMap;

use dbhive_core::models::ColumnInfo;
use serde_json::Value;

/// Quoting for one driver. The shell implements it with the driver's own
/// `quote_identifier` and `escape_string_literal`.
pub trait SqlQuoter {
    fn ident(&self, name: &str) -> String;
    /// A complete, quoted string literal.
    fn literal(&self, text: &str) -> String;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeFamily {
    Integer,
    Float,
    /// Exact numbers (`numeric`, `decimal`, `money`): kept as text so no digit
    /// is lost to a float.
    Decimal,
    Bool,
    Json,
    Text,
}

pub fn family(data_type: &str) -> TypeFamily {
    let t = data_type.to_ascii_lowercase();
    // `tinyint(1)` is MySQL's boolean.
    if t == "tinyint(1)" || t.contains("bool") {
        return TypeFamily::Bool;
    }
    if t.contains("serial") || t.contains("int") {
        // `interval` and `point` contain "int" but are not integers.
        if t.starts_with("interval") || t.starts_with("point") {
            return TypeFamily::Text;
        }
        return TypeFamily::Integer;
    }
    if t.contains("numeric") || t.contains("decimal") || t.contains("money") {
        return TypeFamily::Decimal;
    }
    if t.contains("float") || t.contains("double") || t.contains("real") {
        return TypeFamily::Float;
    }
    if t.contains("json") {
        return TypeFamily::Json;
    }
    TypeFamily::Text
}

fn is_plain_number(s: &str) -> bool {
    let s = s.trim();
    let s = s.strip_prefix('-').unwrap_or(s);
    let mut parts = s.splitn(2, '.');
    let whole = parts.next().unwrap_or("");
    let frac = parts.next();
    !whole.is_empty()
        && whole.chars().all(|c| c.is_ascii_digit())
        && frac.map_or(true, |f| !f.is_empty() && f.chars().all(|c| c.is_ascii_digit()))
}

/// Turn what the user typed into a value for `col`.
///
/// An empty box means NULL when the column allows it. A column that does not
/// allow NULL accepts empty text only if it is a text column.
pub fn coerce(text: &str, col: &ColumnInfo) -> Result<Value, String> {
    let fam = family(&col.data_type);
    if text.trim().is_empty() {
        if col.nullable {
            return Ok(Value::Null);
        }
        if fam == TypeFamily::Text {
            return Ok(Value::String(String::new()));
        }
        return Err(format!("{} cannot be empty", col.name));
    }
    let t = text.trim();
    match fam {
        TypeFamily::Integer => t
            .parse::<i64>()
            .map(|n| Value::Number(n.into()))
            .map_err(|_| format!("\"{t}\" is not a whole number")),
        TypeFamily::Float => t
            .parse::<f64>()
            .ok()
            .filter(|f| f.is_finite())
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| format!("\"{t}\" is not a number")),
        TypeFamily::Decimal => {
            if is_plain_number(t) {
                Ok(Value::String(t.to_string()))
            } else {
                Err(format!("\"{t}\" is not a number"))
            }
        }
        TypeFamily::Bool => match t.to_ascii_lowercase().as_str() {
            "true" | "t" | "yes" | "y" | "on" | "1" => Ok(Value::Bool(true)),
            "false" | "f" | "no" | "n" | "off" | "0" => Ok(Value::Bool(false)),
            _ => Err(format!("\"{t}\" is not true or false")),
        },
        TypeFamily::Json => serde_json::from_str::<Value>(t)
            .map(|_| Value::String(t.to_string()))
            .map_err(|e| format!("Not valid JSON: {e}")),
        TypeFamily::Text => Ok(Value::String(text.to_string())),
    }
}

/// A value as the SQL literal for `col`.
fn literal(value: &Value, col: &ColumnInfo, q: &dyn SqlQuoter) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            if family(&col.data_type) == TypeFamily::Decimal && is_plain_number(s) {
                s.trim().to_string()
            } else {
                q.literal(s)
            }
        }
        other => q.literal(&other.to_string()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatementKind {
    Update,
    Insert,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub kind: StatementKind,
    pub sql: String,
    /// One line for the review list: what it does, in words.
    pub summary: String,
    /// Rows the statement should affect. Anything else means the table changed
    /// under the user (someone else edited or deleted the row).
    pub expect_rows: u64,
}

/// Why a table cannot have its rows updated or deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NoPrimaryKey,
}

impl Refusal {
    pub fn message(&self) -> &'static str {
        match self {
            Refusal::NoPrimaryKey => {
                "This table has no primary key, so its rows cannot be edited or deleted safely. You can still add rows."
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct RowUpdate {
    key: Vec<Value>,
    changes: BTreeMap<usize, Value>,
}

#[derive(Debug, Clone, PartialEq)]
struct RowDelete {
    key: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewRow {
    /// Only the columns the user filled in; the rest take the database default.
    pub values: BTreeMap<usize, Value>,
}

/// What happened to a cell when its text was entered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellEdit {
    /// The cell now holds a pending change.
    Changed,
    /// The text put the original value back, so the change was removed.
    Reverted,
    /// The text was the same as what the cell already showed.
    Unchanged,
}

#[derive(Debug, Clone, Default)]
pub struct EditSession {
    columns: Vec<ColumnInfo>,
    pk: Vec<usize>,
    updates: Vec<RowUpdate>,
    deletes: Vec<RowDelete>,
    inserts: Vec<NewRow>,
}

impl EditSession {
    pub fn new(columns: Vec<ColumnInfo>) -> Self {
        let pk = columns
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_primary_key)
            .map(|(i, _)| i)
            .collect();
        Self {
            columns,
            pk,
            ..Self::default()
        }
    }

    pub fn columns(&self) -> &[ColumnInfo] {
        &self.columns
    }

    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }

    pub fn is_primary_key(&self, col: usize) -> bool {
        self.pk.contains(&col)
    }

    /// `None` when existing rows can be edited and deleted.
    pub fn refusal(&self) -> Option<Refusal> {
        self.pk.is_empty().then_some(Refusal::NoPrimaryKey)
    }

    /// The row's primary key values, in key-column order.
    fn key_of(&self, row: &[Value]) -> Option<Vec<Value>> {
        if self.pk.is_empty() {
            return None;
        }
        self.pk.iter().map(|&i| row.get(i).cloned()).collect()
    }

    fn update_index(&self, key: &[Value]) -> Option<usize> {
        self.updates.iter().position(|u| u.key == key)
    }

    fn delete_index(&self, key: &[Value]) -> Option<usize> {
        self.deletes.iter().position(|d| d.key == key)
    }

    // ----- existing rows -------------------------------------------------

    /// Enter `text` into column `col` of an existing row (`row` holds the
    /// values as loaded from the database, in column order).
    pub fn set_cell(&mut self, row: &[Value], col: usize, text: &str) -> Result<CellEdit, String> {
        if let Some(r) = self.refusal() {
            return Err(r.message().to_string());
        }
        let column = self.columns.get(col).ok_or("No such column")?.clone();
        if self.is_primary_key(col) {
            return Err("Primary key cells cannot be edited".to_string());
        }
        let key = self.key_of(row).ok_or("This row has no primary key value")?;
        if self.delete_index(&key).is_some() {
            return Err("This row is marked for deletion".to_string());
        }
        let new = coerce(text, &column)?;
        let original = row.get(col).cloned().unwrap_or(Value::Null);
        let idx = self.update_index(&key);
        let current = idx
            .and_then(|i| self.updates[i].changes.get(&col).cloned())
            .unwrap_or_else(|| original.clone());
        if values_equal(&new, &current) {
            return Ok(CellEdit::Unchanged);
        }
        if values_equal(&new, &original) {
            // Back to what the database has: drop the change, and the row's
            // entry too if it has no other changes.
            if let Some(i) = idx {
                self.updates[i].changes.remove(&col);
                if self.updates[i].changes.is_empty() {
                    self.updates.remove(i);
                }
            }
            return Ok(CellEdit::Reverted);
        }
        match idx {
            Some(i) => {
                self.updates[i].changes.insert(col, new);
            }
            None => {
                let mut changes = BTreeMap::new();
                changes.insert(col, new);
                self.updates.push(RowUpdate { key, changes });
            }
        }
        Ok(CellEdit::Changed)
    }

    /// What the grid should show in a cell: the pending value if there is one.
    pub fn effective(&self, row: &[Value], col: usize) -> Value {
        if let Some(key) = self.key_of(row) {
            if let Some(i) = self.update_index(&key) {
                if let Some(v) = self.updates[i].changes.get(&col) {
                    return v.clone();
                }
            }
        }
        row.get(col).cloned().unwrap_or(Value::Null)
    }

    pub fn is_modified(&self, row: &[Value], col: usize) -> bool {
        self.key_of(row)
            .and_then(|k| self.update_index(&k))
            .map(|i| self.updates[i].changes.contains_key(&col))
            .unwrap_or(false)
    }

    pub fn is_deleted(&self, row: &[Value]) -> bool {
        self.key_of(row)
            .map(|k| self.delete_index(&k).is_some())
            .unwrap_or(false)
    }

    /// Mark an existing row for deletion, or unmark it. Marking drops any
    /// pending edits to that row, since they would never apply.
    pub fn toggle_delete(&mut self, row: &[Value]) -> Result<bool, String> {
        if let Some(r) = self.refusal() {
            return Err(r.message().to_string());
        }
        let key = self.key_of(row).ok_or("This row has no primary key value")?;
        if let Some(i) = self.delete_index(&key) {
            self.deletes.remove(i);
            return Ok(false);
        }
        if let Some(i) = self.update_index(&key) {
            self.updates.remove(i);
        }
        self.deletes.push(RowDelete { key });
        Ok(true)
    }

    // ----- new rows ------------------------------------------------------

    pub fn new_rows(&self) -> &[NewRow] {
        &self.inserts
    }

    /// Add an empty new row; returns its index.
    pub fn add_row(&mut self) -> usize {
        self.inserts.push(NewRow::default());
        self.inserts.len() - 1
    }

    pub fn set_new_cell(&mut self, row: usize, col: usize, text: &str) -> Result<(), String> {
        let column = self.columns.get(col).ok_or("No such column")?.clone();
        let value = coerce(text, &column)?;
        let r = self.inserts.get_mut(row).ok_or("No such new row")?;
        if text.trim().is_empty() {
            // An empty box leaves the column to its default, which is what
            // people mean by not filling it in.
            r.values.remove(&col);
        } else {
            r.values.insert(col, value);
        }
        Ok(())
    }

    pub fn remove_new_row(&mut self, row: usize) {
        if row < self.inserts.len() {
            self.inserts.remove(row);
        }
    }

    // ----- the whole set -------------------------------------------------

    /// Number of pending changes: edited rows, deleted rows, new rows.
    pub fn pending_count(&self) -> usize {
        self.updates.len() + self.deletes.len() + self.inserts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending_count() == 0
    }

    /// Throw every pending change away.
    pub fn discard(&mut self) {
        self.updates.clear();
        self.deletes.clear();
        self.inserts.clear();
    }

    /// Why the pending changes cannot be applied yet, if they cannot. Checks
    /// that every new row fills in the columns the database will not.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (n, r) in self.inserts.iter().enumerate() {
            if r.values.is_empty() {
                out.push(format!("New row {} is empty: fill in at least one column", n + 1));
                continue;
            }
            for (i, c) in self.columns.iter().enumerate() {
                let needs = !c.nullable
                    && c.default_value.is_none()
                    && !c.is_auto_increment
                    && !r.values.contains_key(&i);
                if needs {
                    out.push(format!("New row {} needs a value for {}", n + 1, c.name));
                }
            }
        }
        out
    }

    /// The statements for every pending change: updates, then deletes, then
    /// inserts, so a row replaced by delete and insert never collides with
    /// itself. `table_sql` is the already quoted and qualified table name.
    pub fn statements(&self, table_sql: &str, q: &dyn SqlQuoter) -> Result<Vec<Statement>, String> {
        if let Some(p) = self.problems().into_iter().next() {
            return Err(p);
        }
        let mut out = Vec::new();

        let where_for = |key: &[Value]| -> String {
            self.pk
                .iter()
                .zip(key)
                .map(|(&i, v)| {
                    let col = &self.columns[i];
                    if v.is_null() {
                        format!("{} IS NULL", q.ident(&col.name))
                    } else {
                        format!("{} = {}", q.ident(&col.name), literal(v, col, q))
                    }
                })
                .collect::<Vec<_>>()
                .join(" AND ")
        };
        let key_label = |key: &[Value]| -> String {
            self.pk
                .iter()
                .zip(key)
                .map(|(&i, v)| format!("{} = {}", self.columns[i].name, short(v)))
                .collect::<Vec<_>>()
                .join(", ")
        };

        for u in &self.updates {
            let sets: Vec<String> = u
                .changes
                .iter()
                .map(|(&i, v)| {
                    let col = &self.columns[i];
                    format!("{} = {}", q.ident(&col.name), literal(v, col, q))
                })
                .collect();
            let names: Vec<&str> = u
                .changes
                .keys()
                .map(|&i| self.columns[i].name.as_str())
                .collect();
            out.push(Statement {
                kind: StatementKind::Update,
                sql: format!(
                    "UPDATE {} SET {} WHERE {}",
                    table_sql,
                    sets.join(", "),
                    where_for(&u.key)
                ),
                summary: format!("Update {} ({})", names.join(", "), key_label(&u.key)),
                expect_rows: 1,
            });
        }
        for d in &self.deletes {
            out.push(Statement {
                kind: StatementKind::Delete,
                sql: format!("DELETE FROM {} WHERE {}", table_sql, where_for(&d.key)),
                summary: format!("Delete row ({})", key_label(&d.key)),
                expect_rows: 1,
            });
        }
        for (n, r) in self.inserts.iter().enumerate() {
            let cols: Vec<String> = r
                .values
                .keys()
                .map(|&i| q.ident(&self.columns[i].name))
                .collect();
            let vals: Vec<String> = r
                .values
                .iter()
                .map(|(&i, v)| literal(v, &self.columns[i], q))
                .collect();
            let names: Vec<&str> = r
                .values
                .keys()
                .map(|&i| self.columns[i].name.as_str())
                .collect();
            out.push(Statement {
                kind: StatementKind::Insert,
                sql: format!(
                    "INSERT INTO {} ({}) VALUES ({})",
                    table_sql,
                    cols.join(", "),
                    vals.join(", ")
                ),
                summary: format!("Add row {} ({})", n + 1, names.join(", ")),
                expect_rows: 1,
            });
        }
        Ok(out)
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        // 3 and 3.0 are the same cell.
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        // A decimal typed as "3.50" equals a stored 3.5.
        (Value::String(x), Value::Number(y)) | (Value::Number(y), Value::String(x)) => {
            x.trim().parse::<f64>().ok() == y.as_f64()
        }
        _ => a == b,
    }
}

fn short(v: &Value) -> String {
    let s = match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if s.chars().count() > 24 {
        format!("{}…", s.chars().take(24).collect::<String>())
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Q;
    impl SqlQuoter for Q {
        fn ident(&self, name: &str) -> String {
            format!("\"{}\"", name.replace('"', "\"\""))
        }
        fn literal(&self, text: &str) -> String {
            format!("'{}'", text.replace('\'', "''"))
        }
    }

    fn col(name: &str, ty: &str, nullable: bool, pk: bool) -> ColumnInfo {
        let mut c = ColumnInfo::new(name.to_string(), ty.to_string(), nullable);
        c.is_primary_key = pk;
        c
    }

    fn cols() -> Vec<ColumnInfo> {
        let mut id = col("id", "integer", false, true);
        id.is_auto_increment = true;
        vec![
            id,
            col("name", "text", false, false),
            col("note", "text", true, false),
            col("qty", "integer", true, false),
            col("price", "numeric(10,2)", true, false),
            col("active", "boolean", true, false),
        ]
    }

    fn row(id: i64, name: &str) -> Vec<Value> {
        vec![json!(id), json!(name), Value::Null, json!(5), json!("9.50"), json!(true)]
    }

    #[test]
    fn column_types_map_to_families() {
        assert_eq!(family("INTEGER"), TypeFamily::Integer);
        assert_eq!(family("bigserial"), TypeFamily::Integer);
        assert_eq!(family("tinyint(1)"), TypeFamily::Bool);
        assert_eq!(family("numeric(10,2)"), TypeFamily::Decimal);
        assert_eq!(family("double precision"), TypeFamily::Float);
        assert_eq!(family("jsonb"), TypeFamily::Json);
        assert_eq!(family("interval"), TypeFamily::Text);
        assert_eq!(family("varchar(255)"), TypeFamily::Text);
    }

    #[test]
    fn typed_text_is_coerced_and_bad_text_is_refused_with_a_reason() {
        let c = cols();
        assert_eq!(coerce("42", &c[3]), Ok(json!(42)));
        assert!(coerce("4.2", &c[3]).unwrap_err().contains("whole number"));
        assert_eq!(coerce("yes", &c[5]), Ok(json!(true)));
        assert_eq!(coerce("0", &c[5]), Ok(json!(false)));
        assert!(coerce("maybe", &c[5]).is_err());
        assert_eq!(coerce("12.50", &c[4]), Ok(json!("12.50")));
        assert!(coerce("12,5", &c[4]).is_err());
        let j = col("meta", "jsonb", true, false);
        assert!(coerce("{\"a\":1}", &j).is_ok());
        assert!(coerce("{oops", &j).unwrap_err().contains("JSON"));
    }

    #[test]
    fn empty_text_is_null_when_allowed_and_an_error_when_not() {
        let c = cols();
        assert_eq!(coerce("  ", &c[2]), Ok(Value::Null)); // nullable text
        assert_eq!(coerce("", &c[3]), Ok(Value::Null)); // nullable integer
        assert!(coerce("", &c[0]).is_err()); // not null integer
        // NOT NULL text may be the empty string.
        assert_eq!(coerce("", &c[1]), Ok(json!("")));
    }

    #[test]
    fn editing_a_cell_records_a_change_shown_in_place() {
        let mut s = EditSession::new(cols());
        let r = row(7, "Ada");
        assert_eq!(s.set_cell(&r, 1, "Grace"), Ok(CellEdit::Changed));
        assert_eq!(s.effective(&r, 1), json!("Grace"));
        assert!(s.is_modified(&r, 1));
        assert!(!s.is_modified(&r, 2));
        assert_eq!(s.pending_count(), 1);
    }

    #[test]
    fn putting_the_original_back_removes_the_change() {
        let mut s = EditSession::new(cols());
        let r = row(7, "Ada");
        s.set_cell(&r, 1, "Grace").unwrap();
        assert_eq!(s.set_cell(&r, 1, "Ada"), Ok(CellEdit::Reverted));
        assert!(s.is_empty());
        assert!(!s.is_modified(&r, 1));
        // The same text as shown is not a change either.
        assert_eq!(s.set_cell(&r, 1, "Ada"), Ok(CellEdit::Unchanged));
        // 5 typed over a stored 5 is not a change.
        assert_eq!(s.set_cell(&r, 3, "5"), Ok(CellEdit::Unchanged));
    }

    #[test]
    fn primary_key_cells_cannot_be_edited() {
        let mut s = EditSession::new(cols());
        let err = s.set_cell(&row(7, "Ada"), 0, "8").unwrap_err();
        assert!(err.contains("Primary key"), "{err}");
        assert!(s.is_empty());
    }

    #[test]
    fn a_table_without_a_primary_key_refuses_edits_and_deletes_but_allows_inserts() {
        let mut c = cols();
        c[0].is_primary_key = false;
        let mut s = EditSession::new(c);
        assert_eq!(s.refusal(), Some(Refusal::NoPrimaryKey));
        assert!(s.set_cell(&row(1, "a"), 1, "b").unwrap_err().contains("primary key"));
        assert!(s.toggle_delete(&row(1, "a")).is_err());
        let n = s.add_row();
        assert!(s.set_new_cell(n, 1, "fresh").is_ok());
    }

    #[test]
    fn a_bad_value_changes_nothing() {
        let mut s = EditSession::new(cols());
        assert!(s.set_cell(&row(7, "Ada"), 3, "lots").is_err());
        assert!(s.is_empty());
    }

    #[test]
    fn deleting_marks_a_row_toggles_off_and_drops_its_edits() {
        let mut s = EditSession::new(cols());
        let r = row(7, "Ada");
        s.set_cell(&r, 1, "Grace").unwrap();
        assert_eq!(s.toggle_delete(&r), Ok(true));
        assert!(s.is_deleted(&r));
        assert_eq!(s.pending_count(), 1, "the edit is gone, the delete remains");
        assert!(s.set_cell(&r, 1, "x").unwrap_err().contains("deletion"));
        assert_eq!(s.toggle_delete(&r), Ok(false));
        assert!(s.is_empty());
    }

    #[test]
    fn changes_follow_the_primary_key_not_the_row_position() {
        let mut s = EditSession::new(cols());
        s.set_cell(&row(7, "Ada"), 1, "Grace").unwrap();
        // The same row appears again (another page, another sort): still modified.
        let again = row(7, "Ada");
        assert!(s.is_modified(&again, 1));
        // A different row is not.
        assert!(!s.is_modified(&row(8, "Ada"), 1));
    }

    #[test]
    fn statements_are_updates_then_deletes_then_inserts_with_quoted_everything() {
        let mut s = EditSession::new(cols());
        s.set_cell(&row(7, "Ada"), 1, "O'Brien").unwrap();
        s.set_cell(&row(7, "Ada"), 3, "9").unwrap();
        s.toggle_delete(&row(8, "Bob")).unwrap();
        let n = s.add_row();
        s.set_new_cell(n, 1, "New").unwrap();
        s.set_new_cell(n, 5, "false").unwrap();
        let st = s.statements("\"public\".\"people\"", &Q).unwrap();
        let sql: Vec<&str> = st.iter().map(|x| x.sql.as_str()).collect();
        assert_eq!(
            sql,
            vec![
                "UPDATE \"public\".\"people\" SET \"name\" = 'O''Brien', \"qty\" = 9 WHERE \"id\" = 7",
                "DELETE FROM \"public\".\"people\" WHERE \"id\" = 8",
                "INSERT INTO \"public\".\"people\" (\"name\", \"active\") VALUES ('New', FALSE)",
            ]
        );
        assert!(st.iter().all(|x| x.expect_rows == 1));
        assert!(st[0].summary.contains("name, qty") && st[0].summary.contains("id = 7"));
    }

    #[test]
    fn user_text_cannot_break_out_of_a_literal_or_an_identifier() {
        let mut s = EditSession::new(cols());
        let evil = "x'; DROP TABLE people; --";
        s.set_cell(&row(7, "Ada"), 1, evil).unwrap();
        let st = s.statements("\"people\"", &Q).unwrap();
        assert_eq!(
            st[0].sql,
            "UPDATE \"people\" SET \"name\" = 'x''; DROP TABLE people; --' WHERE \"id\" = 7"
        );
        // A hostile column name is quoted as one identifier too.
        let mut c = cols();
        c[1].name = "na\"me".to_string();
        let mut s = EditSession::new(c);
        s.set_cell(&row(7, "Ada"), 1, "v").unwrap();
        let st = s.statements("\"people\"", &Q).unwrap();
        assert!(st[0].sql.contains("\"na\"\"me\" = 'v'"), "{}", st[0].sql);
    }

    #[test]
    fn decimals_are_emitted_exactly_not_through_a_float() {
        let mut s = EditSession::new(cols());
        s.set_cell(&row(7, "Ada"), 4, "123456789012345.67").unwrap();
        let st = s.statements("t", &Q).unwrap();
        assert!(st[0].sql.contains("\"price\" = 123456789012345.67"), "{}", st[0].sql);
    }

    #[test]
    fn setting_a_cell_to_null_is_emitted_as_null() {
        let mut s = EditSession::new(cols());
        s.set_cell(&row(7, "Ada"), 3, "").unwrap();
        let st = s.statements("t", &Q).unwrap();
        assert!(st[0].sql.contains("\"qty\" = NULL"), "{}", st[0].sql);
    }

    #[test]
    fn a_new_row_must_fill_the_columns_the_database_will_not() {
        let mut s = EditSession::new(cols());
        s.add_row();
        // Empty row.
        assert!(s.problems()[0].contains("empty"));
        // `id` is auto-increment, `name` is NOT NULL with no default.
        s.set_new_cell(0, 3, "4").unwrap();
        let p = s.problems();
        assert_eq!(p.len(), 1);
        assert!(p[0].contains("name"), "{p:?}");
        assert!(s.statements("t", &Q).is_err());
        s.set_new_cell(0, 1, "ok").unwrap();
        assert!(s.problems().is_empty());
    }

    #[test]
    fn clearing_a_new_cell_leaves_it_to_the_default() {
        let mut s = EditSession::new(cols());
        let n = s.add_row();
        s.set_new_cell(n, 1, "x").unwrap();
        s.set_new_cell(n, 3, "5").unwrap();
        s.set_new_cell(n, 3, "").unwrap();
        let st = s.statements("t", &Q).unwrap();
        assert_eq!(st[0].sql, "INSERT INTO t (\"name\") VALUES ('x')");
    }

    #[test]
    fn removing_a_new_row_and_discarding_empty_the_session() {
        let mut s = EditSession::new(cols());
        let n = s.add_row();
        s.set_new_cell(n, 1, "x").unwrap();
        s.set_cell(&row(7, "Ada"), 1, "Grace").unwrap();
        s.toggle_delete(&row(9, "Z")).unwrap();
        assert_eq!(s.pending_count(), 3);
        s.remove_new_row(n);
        assert_eq!(s.pending_count(), 2);
        s.discard();
        assert!(s.is_empty());
        assert!(s.statements("t", &Q).unwrap().is_empty());
    }

    #[test]
    fn a_composite_key_identifies_the_row_by_every_key_column() {
        let c = vec![
            col("a", "integer", false, true),
            col("b", "text", false, true),
            col("v", "text", true, false),
        ];
        let mut s = EditSession::new(c);
        let r = vec![json!(1), json!("x"), Value::Null];
        s.set_cell(&r, 2, "hello").unwrap();
        let st = s.statements("t", &Q).unwrap();
        assert_eq!(st[0].sql, "UPDATE t SET \"v\" = 'hello' WHERE \"a\" = 1 AND \"b\" = 'x'");
    }
}
