//! EXPLAIN output as a flat, indented tree the results panel can draw.
//!
//! Each database reports plans differently, so the dialect query and the
//! parser live together here:
//!
//! * PostgreSQL and MySQL return JSON (PostgreSQL via `EXPLAIN (FORMAT JSON)`,
//!   MySQL via `EXPLAIN FORMAT=JSON`). Both are parsed into the same rows.
//! * SQLite returns a table (`EXPLAIN QUERY PLAN`) whose `parent` column
//!   builds the tree.
//!
//! The statement is explained, never analysed and never run: `EXPLAIN
//! (ANALYZE)` would execute the query, which is not what a "show the plan"
//! button may do.
//!
//! SQL Server, MongoDB, Redis and Turso have no shape here that maps onto the
//! same tree cheaply, so they report that plainly instead of showing a
//! misleading empty plan.

use dbhive_core::models::DbDriver;
use serde_json::Value;

/// One line of the plan tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRow {
    /// Nesting level, 0 for the root.
    pub depth: i32,
    pub title: String,
    pub detail: String,
    /// Formatted estimated cost, empty when the driver reports none.
    pub cost: String,
    /// Formatted estimated rows, empty when the driver reports none.
    pub rows: String,
    /// A step worth noticing: a full scan or a filesort.
    pub warn: bool,
}

/// The parsed plan and its one-line summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryPlan {
    pub rows: Vec<PlanRow>,
    pub summary: String,
}

/// The statement that asks the server for a plan, or why it cannot.
pub fn explain_sql(driver: DbDriver, sql: &str) -> Result<String, String> {
    let statement = sql.trim().trim_end_matches(';').trim();
    if statement.is_empty() {
        return Err("Write a statement first".to_string());
    }
    match driver {
        DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon => {
            Ok(format!("EXPLAIN (FORMAT JSON) {statement}"))
        }
        DbDriver::MySql => Ok(format!("EXPLAIN FORMAT=JSON {statement}")),
        DbDriver::Sqlite => Ok(format!("EXPLAIN QUERY PLAN {statement}")),
        DbDriver::SqlServer => {
            Err("SQL Server needs a separate SHOWPLAN session; not supported yet".to_string())
        }
        DbDriver::MongoDb => Err("MongoDB has no SQL plan view".to_string()),
        DbDriver::Redis => Err("Redis has no query plans".to_string()),
        DbDriver::Turso => Err("Turso has no query plans".to_string()),
    }
}

/// Turn the result of [`explain_sql`] into the tree.
///
/// `columns` and `rows` are the driver result as plain text: one JSON cell for
/// PostgreSQL and MySQL, the four `EXPLAIN QUERY PLAN` columns for SQLite.
pub fn parse(
    driver: DbDriver,
    columns: &[String],
    rows: &[Vec<String>],
) -> Result<QueryPlan, String> {
    match driver {
        DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon => parse_postgres(rows),
        DbDriver::MySql => parse_mysql(rows),
        DbDriver::Sqlite => parse_sqlite(columns, rows),
        _ => Err(explain_sql(driver, "x").err().unwrap_or_default()),
    }
}

fn parse_postgres(rows: &[Vec<String>]) -> Result<QueryPlan, String> {
    let text = rows
        .first()
        .and_then(|r| r.first())
        .ok_or_else(|| "the server returned no plan".to_string())?;
    let value: Value = serde_json::from_str(text).map_err(|e| format!("unreadable plan: {e}"))?;
    let plan = value
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.get("Plan"))
        .ok_or_else(|| "the plan had no 'Plan' object".to_string())?;

    let mut out = Vec::new();
    push_postgres(plan, 0, &mut out);

    let cost = plan.get("Total Cost").and_then(Value::as_f64);
    let rows = plan.get("Plan Rows").and_then(Value::as_u64);
    let mut parts = vec![format!("{} steps", out.len())];
    if let Some(r) = rows {
        parts.push(format!("~{} rows", crate::format::thousands(r as usize)));
    }
    if let Some(c) = cost {
        parts.push(format!("cost {}", fmt_cost(c)));
    }
    Ok(QueryPlan {
        rows: out,
        summary: parts.join(" · "),
    })
}

fn push_postgres(node: &Value, depth: i32, out: &mut Vec<PlanRow>) {
    let mut title = node
        .get("Node Type")
        .and_then(Value::as_str)
        .unwrap_or("Plan")
        .to_string();
    if let Some(relation) = node.get("Relation Name").and_then(Value::as_str) {
        title.push_str(" on ");
        title.push_str(relation);
    }
    let mut details = Vec::new();
    for (key, label) in [
        ("Join Type", "join"),
        ("Index Name", "index"),
        ("Filter", "filter"),
        ("Index Cond", "index cond"),
        ("Hash Cond", "hash cond"),
        ("Sort Key", "sort"),
        ("Group Key", "group"),
        ("Recheck Cond", "recheck"),
    ] {
        if let Some(v) = node.get(key) {
            let text = match v {
                Value::String(s) => s.clone(),
                Value::Array(a) => a
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", "),
                other => other.to_string(),
            };
            if !text.is_empty() {
                details.push(format!("{label}: {text}"));
            }
        }
    }
    let node_type = node.get("Node Type").and_then(Value::as_str).unwrap_or("");
    let warn = matches!(node_type, "Seq Scan" | "Parallel Seq Scan")
        || details.iter().any(|d| d.contains("Sort Key"));
    out.push(PlanRow {
        depth,
        title,
        detail: details.join(" · "),
        cost: node
            .get("Total Cost")
            .and_then(Value::as_f64)
            .map(fmt_cost)
            .unwrap_or_default(),
        rows: node
            .get("Plan Rows")
            .and_then(Value::as_u64)
            .map(|r| crate::format::thousands(r as usize))
            .unwrap_or_default(),
        warn,
    });
    if let Some(children) = node.get("Plans").and_then(Value::as_array) {
        for child in children {
            push_postgres(child, depth + 1, out);
        }
    }
}

fn parse_mysql(rows: &[Vec<String>]) -> Result<QueryPlan, String> {
    let text = rows
        .first()
        .and_then(|r| r.first())
        .ok_or_else(|| "the server returned no plan".to_string())?;
    let value: Value = serde_json::from_str(text).map_err(|e| format!("unreadable plan: {e}"))?;
    let block = value
        .get("query_block")
        .ok_or_else(|| "the plan had no query_block".to_string())?;

    let mut out = Vec::new();
    push_mysql(block, 0, &mut out);
    if out.is_empty() {
        return Err("the plan had no steps".to_string());
    }
    let mut parts = vec![format!("{} steps", out.len())];
    if let Some(cost) = block
        .get("cost_info")
        .and_then(|c| c.get("query_cost"))
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<f64>().ok())
    {
        parts.push(format!("cost {}", fmt_cost(cost)));
    }
    Ok(QueryPlan {
        rows: out,
        summary: parts.join(" · "),
    })
}

fn push_mysql(node: &Value, depth: i32, out: &mut Vec<PlanRow>) {
    // A table access is the only node worth a line; the wrappers
    // (ordering_operation, grouping_operation, nested_loop) only contribute
    // their own note.
    if let Some(table) = node.get("table") {
        push_mysql_table(table, depth, out);
    }
    if let Some(table) = node.get("table_name").and_then(Value::as_str) {
        let _ = table;
    }
    for key in [
        "nested_loop",
        "ordering_operation",
        "grouping_operation",
        "duplicates_removal",
        "union_result",
        "query_specifications",
        "materialized_from_subquery",
        "attached_subqueries",
    ] {
        if let Some(child) = node.get(key) {
            match child {
                Value::Array(items) => {
                    for item in items {
                        push_mysql(item, depth, out);
                    }
                }
                Value::Object(_) => push_mysql(child, depth, out),
                _ => {}
            }
        }
    }
}

fn push_mysql_table(table: &Value, depth: i32, out: &mut Vec<PlanRow>) {
    let name = table
        .get("table_name")
        .and_then(Value::as_str)
        .unwrap_or("table");
    let access = table
        .get("access_type")
        .and_then(Value::as_str)
        .unwrap_or("");
    let mut title = name.to_string();
    if !access.is_empty() {
        title.push_str(" (");
        title.push_str(access);
        title.push(')');
    }
    let mut details = Vec::new();
    for (key, label) in [
        ("key", "index"),
        ("possible_keys", "possible"),
        ("attached_condition", "filter"),
        ("using_index", "covering"),
    ] {
        if let Some(v) = table.get(key).and_then(Value::as_str) {
            if !v.is_empty() {
                details.push(format!("{label}: {v}"));
            }
        }
    }
    if let Some(join) = table.get("using_join_buffer").and_then(Value::as_str) {
        details.push(format!("join buffer: {join}"));
    }
    let warn = access.eq_ignore_ascii_case("ALL")
        || details.iter().any(|d| d.starts_with("filter"))
        || table
            .get("using_filesort")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    out.push(PlanRow {
        depth,
        title,
        detail: details.join(" · "),
        cost: table
            .get("cost_info")
            .and_then(|c| c.get("read_cost").or_else(|| c.get("prefix_cost")))
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .unwrap_or_default(),
        rows: table
            .get("rows_examined_per_scan")
            .and_then(Value::as_u64)
            .map(|r| crate::format::thousands(r as usize))
            .unwrap_or_default(),
        warn,
    });
    if let Some(children) = table.get("attached_subqueries").and_then(Value::as_array) {
        for child in children {
            push_mysql(child, depth + 1, out);
        }
    }
}

fn parse_sqlite(columns: &[String], rows: &[Vec<String>]) -> Result<QueryPlan, String> {
    let index = |name: &str| columns.iter().position(|c| c.eq_ignore_ascii_case(name));
    let (Some(id_i), Some(parent_i), Some(detail_i)) =
        (index("id"), index("parent"), index("detail"))
    else {
        return Err("EXPLAIN QUERY PLAN returned unexpected columns".to_string());
    };

    // id -> depth, filled as parents are seen. SQLite emits parents before
    // children.
    let mut depths: std::collections::HashMap<i64, i32> = std::collections::HashMap::new();
    let mut out = Vec::new();
    for row in rows {
        let id = row.get(id_i).and_then(|v| v.trim().parse::<i64>().ok());
        let parent = row
            .get(parent_i)
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(0);
        let detail = row.get(detail_i).cloned().unwrap_or_default();
        let depth = if parent == 0 {
            0
        } else {
            depths.get(&parent).map(|d| d + 1).unwrap_or(1)
        };
        if let Some(id) = id {
            depths.insert(id, depth);
        }
        let warn = detail.starts_with("SCAN") && !detail.contains("USING");
        out.push(PlanRow {
            depth,
            title: detail,
            detail: String::new(),
            cost: String::new(),
            rows: String::new(),
            warn,
        });
    }
    if out.is_empty() {
        return Err("the plan had no steps".to_string());
    }
    Ok(QueryPlan {
        summary: format!("{} steps", out.len()),
        rows: out,
    })
}

fn fmt_cost(cost: f64) -> String {
    if cost >= 1000.0 {
        format!("{:.0}", cost)
    } else if cost >= 10.0 {
        format!("{:.1}", cost)
    } else {
        format!("{:.2}", cost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn row(cells: &[&str]) -> Vec<String> {
        cells.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn every_supported_driver_gets_a_read_only_explain() {
        assert_eq!(
            explain_sql(DbDriver::Postgres, "select 1;").unwrap(),
            "EXPLAIN (FORMAT JSON) select 1"
        );
        assert_eq!(
            explain_sql(DbDriver::MySql, "select 1").unwrap(),
            "EXPLAIN FORMAT=JSON select 1"
        );
        assert_eq!(
            explain_sql(DbDriver::Sqlite, "select 1").unwrap(),
            "EXPLAIN QUERY PLAN select 1"
        );
        assert!(explain_sql(DbDriver::Redis, "PING").is_err());
        assert!(explain_sql(DbDriver::Postgres, "   ").is_err());
    }

    #[test]
    fn postgres_json_becomes_an_indented_tree() {
        let json = r#"[{"Plan":{"Node Type":"Hash Join","Join Type":"Inner","Total Cost":21.5,"Plan Rows":40,"Plans":[{"Node Type":"Seq Scan","Relation Name":"widgets","Total Cost":1.2,"Plan Rows":4},{"Node Type":"Hash","Total Cost":2.0,"Plan Rows":40,"Plans":[{"Node Type":"Seq Scan","Relation Name":"orders","Total Cost":1.9,"Plan Rows":40}]}]}}]"#;
        let plan = parse(DbDriver::Postgres, &cols(&["QUERY PLAN"]), &[row(&[json])]).unwrap();
        assert_eq!(plan.rows.len(), 4);
        assert_eq!(plan.rows[0].title, "Hash Join");
        assert_eq!(plan.rows[0].depth, 0);
        assert_eq!(plan.rows[1].title, "Seq Scan on widgets");
        assert_eq!(plan.rows[1].depth, 1);
        assert!(plan.rows[1].warn, "a sequential scan is flagged");
        assert_eq!(plan.rows[2].depth, 1);
        assert_eq!(plan.rows[3].depth, 2);
        assert!(plan.summary.contains("~40 rows"), "{}", plan.summary);
    }

    #[test]
    fn mysql_json_flattens_wrappers_and_tables() {
        let json = r#"{"query_block":{"select_id":1,"cost_info":{"query_cost":"3.45"},"ordering_operation":{"using_filesort":true,"table":{"table_name":"customers","access_type":"ALL","rows_examined_per_scan":120,"attached_condition":"(customers.plan = 'pro')","cost_info":{"read_cost":"1.20"}}}}}"#;
        let plan = parse(DbDriver::MySql, &cols(&["EXPLAIN"]), &[row(&[json])]).unwrap();
        assert_eq!(plan.rows.len(), 1);
        assert_eq!(plan.rows[0].title, "customers (ALL)");
        assert!(plan.rows[0].warn);
        assert!(plan.rows[0].detail.contains("filter"));
        assert_eq!(plan.rows[0].rows, "120");
        assert!(plan.summary.contains("cost 3.45"), "{}", plan.summary);
    }

    #[test]
    fn sqlite_rows_get_depth_from_their_parent() {
        let plan = parse(
            DbDriver::Sqlite,
            &cols(&["id", "parent", "notused", "detail"]),
            &[
                row(&["2", "0", "0", "SCAN widgets"]),
                row(&["6", "0", "0", "SEARCH orders USING INDEX idx_orders"]),
                row(&["10", "6", "0", "USE TEMP B-TREE FOR ORDER BY"]),
            ],
        )
        .unwrap();
        assert_eq!(plan.rows[0].depth, 0);
        assert!(plan.rows[0].warn);
        assert!(!plan.rows[1].warn);
        assert_eq!(plan.rows[2].depth, 1);
        assert_eq!(plan.summary, "3 steps");
    }

    #[test]
    fn broken_plans_report_an_error_not_an_empty_tree() {
        assert!(parse(
            DbDriver::Postgres,
            &cols(&["QUERY PLAN"]),
            &[row(&["not json"])]
        )
        .is_err());
        assert!(parse(DbDriver::MySql, &cols(&["EXPLAIN"]), &[]).is_err());
        assert!(parse(DbDriver::Sqlite, &cols(&["a", "b"]), &[]).is_err());
    }
}
