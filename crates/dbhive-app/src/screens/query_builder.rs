//! Visual query builder: compose a SELECT without writing SQL.
//!
//! Parity target: the React `QueryBuilder.tsx` route plus the seven panels
//! under `src/components/queryBuilder/` and the pure generator in
//! `src/lib/sqlGenerator.ts` (see 2.8 in
//! docs/rust-ui-migration/01-frontend-inventory.md).
//!
//! Three rules shape this file:
//!
//! * **Everything that reaches SQL is quoted or escaped by the driver.**
//!   Identifiers go through `quote_identifier`, values through
//!   `escape_string_literal`. A builder that concatenates the user's text
//!   into a statement is an injection hole, and a table called
//!   `order details` or a column called `select` has to work. The only text
//!   that ever lands unquoted is a number this file has itself proved to be
//!   numeric.
//! * **Partial state produces no SQL, never broken SQL.** A half-typed
//!   condition, a join missing its columns, a table nothing joins to: each is
//!   dropped from generation and reported as a warning, so the preview is
//!   always something the database would accept.
//! * **Nothing blocks the frame.** The table list, per-table schema and the
//!   query run are separate `Job`s; the driver `Arc` is cloned out of state
//!   and the lock dropped before any await.
//!
//! The generator is split from the drawing on purpose: `generate_sql` and its
//! helpers are pure functions over `BuilderState`, and that is where the
//! tests live.

use std::sync::Arc;

use dbhive_core::drivers::{DatabaseDriver, QueryResult};
use dbhive_core::models::{DbDriver, DbError, ForeignKeyInfo, TableInfo, TableSchema};

use crate::app::DbHiveApp;
use crate::task::Job;
use crate::widgets::{results_grid, sql_editor};

/// What the builder wants the host screen to do after this frame.
///
/// The screen cannot reach the workspace's tab list itself, and the React
/// original likewise handed its SQL off rather than running it in a tab, so
/// the generated statement travels back to the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuilderAction {
    None,
    /// Put this statement in a SQL editor tab. Running it stays the user's
    /// explicit act there, as it is with the AI assistant.
    UseSql(String),
}

/// How deep AND/OR groups may nest. Matches the React builder's limit; past
/// three levels the UI is unreadable and the SQL unmaintainable.
pub const MAX_GROUP_DEPTH: usize = 3;

// ---------------------------------------------------------------------------
// Pure model
// ---------------------------------------------------------------------------

/// Aggregate applied to one output column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aggregate {
    None,
    Count,
    Sum,
    Avg,
    Min,
    Max,
    CountDistinct,
}

impl Aggregate {
    pub const ALL: [Aggregate; 7] = [
        Aggregate::None,
        Aggregate::Count,
        Aggregate::Sum,
        Aggregate::Avg,
        Aggregate::Min,
        Aggregate::Max,
        Aggregate::CountDistinct,
    ];

    /// Aggregates that make sense in a HAVING clause.
    pub const FOR_HAVING: [Aggregate; 6] = [
        Aggregate::Count,
        Aggregate::Sum,
        Aggregate::Avg,
        Aggregate::Min,
        Aggregate::Max,
        Aggregate::CountDistinct,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Aggregate::None => "no aggregate",
            Aggregate::Count => "COUNT",
            Aggregate::Sum => "SUM",
            Aggregate::Avg => "AVG",
            Aggregate::Min => "MIN",
            Aggregate::Max => "MAX",
            Aggregate::CountDistinct => "COUNT DISTINCT",
        }
    }

    pub fn is_some(self) -> bool {
        !matches!(self, Aggregate::None)
    }

    /// The function name as written in SQL. `COUNT DISTINCT` is spelled
    /// `COUNT(DISTINCT x)`, so it shares COUNT's name.
    fn func(self) -> &'static str {
        match self {
            Aggregate::None => "",
            Aggregate::Count | Aggregate::CountDistinct => "COUNT",
            Aggregate::Sum => "SUM",
            Aggregate::Avg => "AVG",
            Aggregate::Min => "MIN",
            Aggregate::Max => "MAX",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinType {
    Inner,
    Left,
    Right,
    FullOuter,
    Cross,
}

impl JoinType {
    pub const ALL: [JoinType; 5] = [
        JoinType::Inner,
        JoinType::Left,
        JoinType::Right,
        JoinType::FullOuter,
        JoinType::Cross,
    ];

    pub fn sql(self) -> &'static str {
        match self {
            JoinType::Inner => "INNER",
            JoinType::Left => "LEFT",
            JoinType::Right => "RIGHT",
            JoinType::FullOuter => "FULL OUTER",
            JoinType::Cross => "CROSS",
        }
    }

    /// CROSS JOIN takes no ON clause; every other type requires one.
    pub fn takes_on(self) -> bool {
        !matches!(self, JoinType::Cross)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    Ne,
    Gt,
    Lt,
    Gte,
    Lte,
    Like,
    NotLike,
    In,
    NotIn,
    IsNull,
    IsNotNull,
    Between,
}

impl CompareOp {
    pub const ALL: [CompareOp; 13] = [
        CompareOp::Eq,
        CompareOp::Ne,
        CompareOp::Gt,
        CompareOp::Lt,
        CompareOp::Gte,
        CompareOp::Lte,
        CompareOp::Like,
        CompareOp::NotLike,
        CompareOp::In,
        CompareOp::NotIn,
        CompareOp::IsNull,
        CompareOp::IsNotNull,
        CompareOp::Between,
    ];

    /// HAVING compares an aggregate against one scalar, so the list operators
    /// and null checks are left out, as they were in the React panel.
    pub const FOR_HAVING: [CompareOp; 6] = [
        CompareOp::Eq,
        CompareOp::Ne,
        CompareOp::Gt,
        CompareOp::Lt,
        CompareOp::Gte,
        CompareOp::Lte,
    ];

    pub fn sql(self) -> &'static str {
        match self {
            CompareOp::Eq => "=",
            CompareOp::Ne => "!=",
            CompareOp::Gt => ">",
            CompareOp::Lt => "<",
            CompareOp::Gte => ">=",
            CompareOp::Lte => "<=",
            CompareOp::Like => "LIKE",
            CompareOp::NotLike => "NOT LIKE",
            CompareOp::In => "IN",
            CompareOp::NotIn => "NOT IN",
            CompareOp::IsNull => "IS NULL",
            CompareOp::IsNotNull => "IS NOT NULL",
            CompareOp::Between => "BETWEEN",
        }
    }

    /// Which value editor this operator needs.
    pub fn arity(self) -> Arity {
        match self {
            CompareOp::IsNull | CompareOp::IsNotNull => Arity::None,
            CompareOp::In | CompareOp::NotIn => Arity::List,
            CompareOp::Between => Arity::Range,
            _ => Arity::One,
        }
    }
}

/// Shape of the value editor an operator needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arity {
    /// No operand at all (IS NULL).
    None,
    One,
    /// Two operands (BETWEEN).
    Range,
    /// Comma separated list (IN).
    List,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Logical {
    And,
    Or,
}

impl Logical {
    pub fn sql(self) -> &'static str {
        match self {
            Logical::And => "AND",
            Logical::Or => "OR",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

impl SortDir {
    pub fn sql(self) -> &'static str {
        match self {
            SortDir::Asc => "ASC",
            SortDir::Desc => "DESC",
        }
    }

    pub fn toggled(self) -> SortDir {
        match self {
            SortDir::Asc => SortDir::Desc,
            SortDir::Desc => SortDir::Asc,
        }
    }
}

/// How a typed value should be rendered into the statement.
///
/// Derived once from the column's declared type when the user picks it, so
/// the generator never has to re-parse a type name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValueKind {
    #[default]
    Text,
    Number,
    Bool,
}

impl ValueKind {
    /// Classify a driver-reported type name.
    ///
    /// Only three buckets matter here: whether the value can be written bare
    /// as a number, bare as a boolean, or has to be a quoted literal. Dates,
    /// JSON, UUIDs and blobs are all quoted text, which every dialect casts
    /// from an untyped literal.
    pub fn from_data_type(data_type: &str) -> ValueKind {
        let t = data_type.to_ascii_lowercase();
        if t.contains("bool") || t == "bit" {
            return ValueKind::Bool;
        }
        const NUMERIC: [&str; 12] = [
            "int", "serial", "numeric", "decimal", "real", "double", "float", "money", "number",
            "long", "short", "byte",
        ];
        // "interval" and "point" contain no numeric marker; "timestamp" must
        // not match "stamp"-free numeric words, hence the explicit list.
        if NUMERIC.iter().any(|n| t.contains(n)) && !t.contains("interval") {
            return ValueKind::Number;
        }
        ValueKind::Text
    }
}

/// A table brought into the query, with the columns the user can pick from.
///
/// `alias` is what every reference in the statement uses, which is what makes
/// selecting the same table twice (a self-join) work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryTable {
    pub alias: String,
    pub schema: String,
    pub name: String,
    /// (column name, declared type).
    pub columns: Vec<(String, String)>,
}

impl QueryTable {
    pub fn new(alias: impl Into<String>, schema: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            alias: alias.into(),
            schema: schema.into(),
            name: name.into(),
            columns: Vec::new(),
        }
    }

    pub fn with_columns(mut self, columns: Vec<(String, String)>) -> Self {
        self.columns = columns;
        self
    }
}

/// The pseudo-column standing for "every column of this table".
pub const STAR: &str = "*";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedColumn {
    pub table_alias: String,
    pub column: String,
    /// Output name. Empty means no `AS`.
    pub alias: String,
    pub aggregate: Aggregate,
    /// Per-column DISTINCT, honoured only on the first output column (see
    /// `select_clause`).
    pub distinct: bool,
}

impl SelectedColumn {
    pub fn new(table_alias: impl Into<String>, column: impl Into<String>) -> Self {
        Self {
            table_alias: table_alias.into(),
            column: column.into(),
            alias: String::new(),
            aggregate: Aggregate::None,
            distinct: false,
        }
    }

    fn is_star(&self) -> bool {
        self.column == STAR
    }

    /// `*` only carries meaning bare or under COUNT. `SUM(t.*)` is not a
    /// statement any dialect accepts, so such a row is dropped rather than
    /// emitted.
    fn is_usable(&self) -> bool {
        if self.table_alias.is_empty() || self.column.is_empty() {
            return false;
        }
        if self.is_star() {
            return matches!(self.aggregate, Aggregate::None | Aggregate::Count);
        }
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Join {
    pub join_type: JoinType,
    /// Alias of the already-joined table on the left of the ON condition.
    pub left_table: String,
    pub left_column: String,
    /// Alias of the table this join brings into the query.
    pub right_table: String,
    pub right_column: String,
}

impl Join {
    pub fn new(left_table: impl Into<String>, right_table: impl Into<String>) -> Self {
        Self {
            join_type: JoinType::Inner,
            left_table: left_table.into(),
            left_column: String::new(),
            right_table: right_table.into(),
            right_column: String::new(),
        }
    }

    /// A join is only written once it names both sides, and, unless it is a
    /// CROSS JOIN, both ON columns.
    pub fn is_complete(&self) -> bool {
        if self.left_table.is_empty() || self.right_table.is_empty() {
            return false;
        }
        if !self.join_type.takes_on() {
            return true;
        }
        !self.left_column.is_empty() && !self.right_column.is_empty()
    }
}

/// One comparison in a WHERE group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    pub table_alias: String,
    pub column: String,
    pub op: CompareOp,
    pub kind: ValueKind,
    pub value: String,
    /// Upper bound for BETWEEN.
    pub value2: String,
}

impl Condition {
    pub fn new(table_alias: impl Into<String>, column: impl Into<String>, kind: ValueKind) -> Self {
        Self {
            table_alias: table_alias.into(),
            column: column.into(),
            op: CompareOp::Eq,
            kind,
            value: String::new(),
            value2: String::new(),
        }
    }

    /// Whether this row has enough in it to become SQL.
    ///
    /// A half-typed row is skipped rather than emitted as `col = ''`, which
    /// silently means something quite different from "not filled in yet".
    pub fn is_complete(&self) -> bool {
        if self.table_alias.is_empty() || self.column.is_empty() {
            return false;
        }
        match self.op.arity() {
            Arity::None => true,
            Arity::One => !self.value.trim().is_empty(),
            Arity::Range => !self.value.trim().is_empty() && !self.value2.trim().is_empty(),
            Arity::List => !parse_list(&self.value).is_empty(),
        }
    }
}

/// Conditions joined by one logical operator, plus nested groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionGroup {
    pub operator: Logical,
    pub conditions: Vec<Condition>,
    pub groups: Vec<ConditionGroup>,
}

impl Default for ConditionGroup {
    fn default() -> Self {
        Self {
            operator: Logical::And,
            conditions: Vec::new(),
            groups: Vec::new(),
        }
    }
}

impl ConditionGroup {
    fn is_empty(&self) -> bool {
        self.conditions.is_empty() && self.groups.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupByClause {
    pub table_alias: String,
    pub column: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HavingCondition {
    pub table_alias: String,
    pub column: String,
    pub aggregate: Aggregate,
    pub op: CompareOp,
    pub kind: ValueKind,
    pub value: String,
}

impl HavingCondition {
    pub fn new(table_alias: impl Into<String>, column: impl Into<String>) -> Self {
        Self {
            table_alias: table_alias.into(),
            column: column.into(),
            aggregate: Aggregate::Count,
            op: CompareOp::Gt,
            kind: ValueKind::Number,
            value: String::new(),
        }
    }

    fn is_complete(&self) -> bool {
        !self.table_alias.is_empty() && !self.column.is_empty() && !self.value.trim().is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderByClause {
    pub table_alias: String,
    pub column: String,
    pub direction: SortDir,
}

/// The whole builder, independent of any UI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuilderState {
    pub tables: Vec<QueryTable>,
    pub columns: Vec<SelectedColumn>,
    pub joins: Vec<Join>,
    pub filters: ConditionGroup,
    pub group_by: Vec<GroupByClause>,
    pub having: Vec<HavingCondition>,
    pub order_by: Vec<OrderByClause>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub distinct: bool,
}

impl BuilderState {
    /// Remove a table and everything that referenced it, so no clause is left
    /// pointing at an alias the statement no longer declares.
    pub fn remove_table(&mut self, alias: &str) {
        self.tables.retain(|t| t.alias != alias);
        self.columns.retain(|c| c.table_alias != alias);
        self.joins
            .retain(|j| j.left_table != alias && j.right_table != alias);
        self.group_by.retain(|g| g.table_alias != alias);
        self.having.retain(|h| h.table_alias != alias);
        self.order_by.retain(|o| o.table_alias != alias);
        prune_group(&mut self.filters, alias);
    }

    /// An alias not yet used, derived from the table name so the SQL reads
    /// naturally. A second copy of `users` becomes `users_2`, which is what
    /// makes self-joins possible.
    pub fn free_alias(&self, table: &str) -> String {
        let base = if table.is_empty() { "t" } else { table };
        if !self.tables.iter().any(|t| t.alias == base) {
            return base.to_string();
        }
        for n in 2..1000 {
            let candidate = format!("{}_{}", base, n);
            if !self.tables.iter().any(|t| t.alias == candidate) {
                return candidate;
            }
        }
        format!("{}_{}", base, self.tables.len() + 1)
    }

    pub fn columns_of(&self, alias: &str) -> &[(String, String)] {
        self.tables
            .iter()
            .find(|t| t.alias == alias)
            .map(|t| t.columns.as_slice())
            .unwrap_or(&[])
    }

    fn value_kind(&self, alias: &str, column: &str) -> ValueKind {
        self.columns_of(alias)
            .iter()
            .find(|(n, _)| n == column)
            .map(|(_, ty)| ValueKind::from_data_type(ty))
            .unwrap_or_default()
    }
}

/// Drop every condition referencing `alias`, at any depth, and collapse the
/// groups that empties.
fn prune_group(group: &mut ConditionGroup, alias: &str) {
    group.conditions.retain(|c| c.table_alias != alias);
    for g in group.groups.iter_mut() {
        prune_group(g, alias);
    }
    group.groups.retain(|g| !g.is_empty());
}

// ---------------------------------------------------------------------------
// Dialect
// ---------------------------------------------------------------------------

/// How a dialect spells row limiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitStyle {
    /// `LIMIT n` and `OFFSET m`, each usable on its own (Postgres family).
    Standard,
    /// MySQL rejects a bare OFFSET, so one has to be paired with a limit
    /// large enough to mean "the rest". This is the documented idiom.
    MySql,
    /// SQLite also rejects a bare OFFSET but spells "no limit" as `LIMIT -1`.
    Sqlite,
    /// T-SQL: `OFFSET m ROWS FETCH NEXT n ROWS ONLY`, which additionally
    /// requires an ORDER BY.
    OffsetFetch,
}

/// The parts of a dialect the generator has to know about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialectRules {
    pub limit_style: LimitStyle,
    pub supports_full_outer: bool,
    pub supports_right_join: bool,
}

impl Default for DialectRules {
    fn default() -> Self {
        Self {
            limit_style: LimitStyle::Standard,
            supports_full_outer: true,
            supports_right_join: true,
        }
    }
}

pub fn rules_for(driver: &DbDriver) -> DialectRules {
    match driver {
        DbDriver::Postgres | DbDriver::Supabase | DbDriver::Neon => DialectRules::default(),
        // MySQL has had no FULL OUTER JOIN in any released version.
        DbDriver::MySql => DialectRules {
            limit_style: LimitStyle::MySql,
            supports_full_outer: false,
            supports_right_join: true,
        },
        // RIGHT and FULL joins only arrived in SQLite 3.39 (2022); warn
        // rather than block, since the shipped runtime may well be newer.
        DbDriver::Sqlite | DbDriver::Turso => DialectRules {
            limit_style: LimitStyle::Sqlite,
            supports_full_outer: false,
            supports_right_join: false,
        },
        DbDriver::SqlServer => DialectRules {
            limit_style: LimitStyle::OffsetFetch,
            supports_full_outer: true,
            supports_right_join: true,
        },
        // Neither speaks SQL; the builder refuses them before it gets here.
        DbDriver::MongoDb | DbDriver::Redis => DialectRules::default(),
    }
}

/// Drivers this screen can build a SELECT for at all.
pub fn supports_builder(driver: &DbDriver) -> bool {
    !matches!(driver, DbDriver::MongoDb | DbDriver::Redis)
}

// ---------------------------------------------------------------------------
// SQL generation
// ---------------------------------------------------------------------------

/// Split a comma separated IN list, dropping blank entries.
pub fn parse_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Whether `s` can be written into SQL bare, as a number.
///
/// The character whitelist is what makes this safe: a string passing it
/// cannot contain a quote, a space or a semicolon, so there is nothing to
/// smuggle. Anything else becomes a quoted, escaped literal instead.
fn is_numeric_literal(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c == '+')
        && s.parse::<f64>().is_ok()
}

/// Render one operand.
///
/// Numbers and booleans are written bare where the column's type says they
/// belong; everything else is a single-quoted literal escaped by the driver.
pub fn format_value(escape: &impl Fn(&str) -> String, raw: &str, kind: ValueKind) -> String {
    let t = raw.trim();
    match kind {
        ValueKind::Bool => {
            let truthy = matches!(
                t.to_ascii_lowercase().as_str(),
                "true" | "t" | "1" | "yes" | "y"
            );
            if truthy { "TRUE" } else { "FALSE" }.to_string()
        }
        ValueKind::Number if is_numeric_literal(t) => t.to_string(),
        _ => format!("'{}'", escape(t)),
    }
}

/// `schema.table`, both quoted. Drivers with no schema concept pass an empty
/// schema and get the bare table.
pub fn qualified_name(quote: &impl Fn(&str) -> String, schema: &str, table: &str) -> String {
    if schema.is_empty() {
        quote(table)
    } else {
        format!("{}.{}", quote(schema), quote(table))
    }
}

/// `alias.column`, both quoted. `*` is a token, not an identifier, so it is
/// the one thing that is never quoted.
fn column_ref(quote: &impl Fn(&str) -> String, alias: &str, column: &str) -> String {
    if column == STAR {
        format!("{}.*", quote(alias))
    } else {
        format!("{}.{}", quote(alias), quote(column))
    }
}

/// Aliases actually present in the statement, in the order they appear.
///
/// The first table is the FROM; every other table only enters through a
/// complete join whose left side is already present. That rule is what stops
/// the generator ever emitting a reference to an alias the statement never
/// declares.
pub fn active_aliases(state: &BuilderState) -> Vec<String> {
    let Some(first) = state.tables.first() else {
        return Vec::new();
    };
    let mut active = vec![first.alias.clone()];
    // Repeated passes so joins written out of order still resolve: a join
    // onto a table that a later join brings in is picked up next round.
    loop {
        let mut added = false;
        for join in &state.joins {
            if !join.is_complete() {
                continue;
            }
            if active.iter().any(|a| a == &join.right_table) {
                continue;
            }
            if !active.iter().any(|a| a == &join.left_table) {
                continue;
            }
            if !state.tables.iter().any(|t| t.alias == join.right_table) {
                continue;
            }
            active.push(join.right_table.clone());
            added = true;
        }
        if !added {
            break;
        }
    }
    active
}

/// Indices of the joins that are written, in statement order.
fn resolved_joins(state: &BuilderState, active: &[String]) -> Vec<usize> {
    let mut placed: Vec<String> = state
        .tables
        .first()
        .map(|t| vec![t.alias.clone()])
        .unwrap_or_default();
    let mut out = Vec::new();
    loop {
        let mut added = false;
        for (i, join) in state.joins.iter().enumerate() {
            if out.contains(&i) || !join.is_complete() {
                continue;
            }
            if !active.iter().any(|a| a == &join.right_table) {
                continue;
            }
            if placed.iter().any(|a| a == &join.right_table) {
                continue;
            }
            if !placed.iter().any(|a| a == &join.left_table) {
                continue;
            }
            out.push(i);
            placed.push(join.right_table.clone());
            added = true;
        }
        if !added {
            break;
        }
    }
    out
}

/// `SELECT [DISTINCT] ...`, or `SELECT *` when nothing is picked.
fn select_clause(quote: &impl Fn(&str) -> String, state: &BuilderState, active: &[String]) -> String {
    let usable: Vec<&SelectedColumn> = state
        .columns
        .iter()
        .filter(|c| c.is_usable() && active.iter().any(|a| a == &c.table_alias))
        .collect();

    let head = if state.distinct { "SELECT DISTINCT " } else { "SELECT " };
    if usable.is_empty() {
        return format!("{}*", head);
    }

    let parts: Vec<String> = usable
        .iter()
        .enumerate()
        .map(|(i, col)| {
            let base = column_ref(quote, &col.table_alias, &col.column);
            let mut expr = match col.aggregate {
                Aggregate::None => {
                    // DISTINCT inside a select list is a statement-level
                    // prefix, not a per-expression one: `a, DISTINCT b` is
                    // not valid SQL anywhere. It is therefore honoured on
                    // the leading column only, and `validate` says so.
                    if col.distinct && !state.distinct && i == 0 {
                        format!("DISTINCT {}", base)
                    } else {
                        base
                    }
                }
                Aggregate::Count if col.is_star() => "COUNT(*)".to_string(),
                Aggregate::CountDistinct => format!("COUNT(DISTINCT {})", base),
                agg => format!("{}({})", agg.func(), base),
            };
            if !col.alias.is_empty() {
                expr.push_str(&format!(" AS {}", quote(&col.alias)));
            }
            expr
        })
        .collect();

    format!("{}{}", head, parts.join(", "))
}

fn from_clause(quote: &impl Fn(&str) -> String, table: &QueryTable) -> String {
    format!(
        "FROM {} AS {}",
        qualified_name(quote, &table.schema, &table.name),
        quote(&table.alias)
    )
}

fn join_clauses(
    quote: &impl Fn(&str) -> String,
    state: &BuilderState,
    active: &[String],
) -> Vec<String> {
    resolved_joins(state, active)
        .into_iter()
        .filter_map(|i| {
            let join = &state.joins[i];
            let right = state.tables.iter().find(|t| t.alias == join.right_table)?;
            let target = format!(
                "{} JOIN {} AS {}",
                join.join_type.sql(),
                qualified_name(quote, &right.schema, &right.name),
                quote(&right.alias)
            );
            if !join.join_type.takes_on() {
                return Some(target);
            }
            Some(format!(
                "{} ON {} = {}",
                target,
                column_ref(quote, &join.left_table, &join.left_column),
                column_ref(quote, &join.right_table, &join.right_column)
            ))
        })
        .collect()
}

fn condition_sql(
    quote: &impl Fn(&str) -> String,
    escape: &impl Fn(&str) -> String,
    cond: &Condition,
) -> String {
    let col = column_ref(quote, &cond.table_alias, &cond.column);
    // A pattern is text whatever the column holds, so `LIKE 5` is never
    // emitted against a numeric column.
    let kind = match cond.op {
        CompareOp::Like | CompareOp::NotLike => ValueKind::Text,
        _ => cond.kind,
    };
    let one = |raw: &str| format_value(escape, raw, kind);

    match cond.op {
        CompareOp::IsNull => format!("{} IS NULL", col),
        CompareOp::IsNotNull => format!("{} IS NOT NULL", col),
        CompareOp::In | CompareOp::NotIn => {
            let items: Vec<String> = parse_list(&cond.value).iter().map(|v| one(v)).collect();
            format!("{} {} ({})", col, cond.op.sql(), items.join(", "))
        }
        CompareOp::Between => format!(
            "{} BETWEEN {} AND {}",
            col,
            one(&cond.value),
            one(&cond.value2)
        ),
        op => format!("{} {} {}", col, op.sql(), one(&cond.value)),
    }
}

/// One group's conditions and nested groups, joined by its operator.
///
/// Nested groups are parenthesised so AND and OR never silently rebind.
fn group_sql(
    quote: &impl Fn(&str) -> String,
    escape: &impl Fn(&str) -> String,
    group: &ConditionGroup,
    active: &[String],
) -> String {
    let mut parts: Vec<String> = group
        .conditions
        .iter()
        .filter(|c| c.is_complete() && active.iter().any(|a| a == &c.table_alias))
        .map(|c| condition_sql(quote, escape, c))
        .collect();

    for nested in &group.groups {
        let inner = group_sql(quote, escape, nested, active);
        if !inner.is_empty() {
            parts.push(format!("({})", inner));
        }
    }

    parts.join(&format!(" {} ", group.operator.sql()))
}

fn where_clause(
    quote: &impl Fn(&str) -> String,
    escape: &impl Fn(&str) -> String,
    state: &BuilderState,
    active: &[String],
) -> String {
    let body = group_sql(quote, escape, &state.filters, active);
    if body.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", body)
    }
}

fn group_by_clause(
    quote: &impl Fn(&str) -> String,
    state: &BuilderState,
    active: &[String],
) -> String {
    let cols: Vec<String> = state
        .group_by
        .iter()
        .filter(|g| {
            !g.column.is_empty() && g.column != STAR && active.iter().any(|a| a == &g.table_alias)
        })
        .map(|g| column_ref(quote, &g.table_alias, &g.column))
        .collect();
    if cols.is_empty() {
        String::new()
    } else {
        format!("GROUP BY {}", cols.join(", "))
    }
}

fn having_expr(quote: &impl Fn(&str) -> String, h: &HavingCondition) -> String {
    let base = column_ref(quote, &h.table_alias, &h.column);
    match h.aggregate {
        Aggregate::None => base,
        Aggregate::Count if h.column == STAR => "COUNT(*)".to_string(),
        Aggregate::CountDistinct => format!("COUNT(DISTINCT {})", base),
        agg => format!("{}({})", agg.func(), base),
    }
}

fn having_clause(
    quote: &impl Fn(&str) -> String,
    escape: &impl Fn(&str) -> String,
    state: &BuilderState,
    active: &[String],
) -> String {
    let parts: Vec<String> = state
        .having
        .iter()
        .filter(|h| h.is_complete() && active.iter().any(|a| a == &h.table_alias))
        .map(|h| {
            format!(
                "{} {} {}",
                having_expr(quote, h),
                h.op.sql(),
                format_value(escape, &h.value, h.kind)
            )
        })
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!("HAVING {}", parts.join(" AND "))
    }
}

fn order_by_clause(
    quote: &impl Fn(&str) -> String,
    state: &BuilderState,
    active: &[String],
) -> String {
    let cols: Vec<String> = state
        .order_by
        .iter()
        .filter(|o| {
            !o.column.is_empty() && o.column != STAR && active.iter().any(|a| a == &o.table_alias)
        })
        .map(|o| {
            format!(
                "{} {}",
                column_ref(quote, &o.table_alias, &o.column),
                o.direction.sql()
            )
        })
        .collect();
    if cols.is_empty() {
        String::new()
    } else {
        format!("ORDER BY {}", cols.join(", "))
    }
}

/// The dialect's spelling of LIMIT and OFFSET.
///
/// The awkward cases are all "offset without limit": MySQL and SQLite both
/// reject a bare OFFSET, and T-SQL spells the whole thing differently.
pub fn limit_offset_clause(rules: DialectRules, limit: Option<u64>, offset: Option<u64>) -> String {
    // An offset of zero is the default; writing it out is noise.
    let offset = offset.filter(|o| *o > 0);
    match (limit, offset) {
        (None, None) => String::new(),
        (Some(n), None) => match rules.limit_style {
            LimitStyle::OffsetFetch => format!("OFFSET 0 ROWS FETCH NEXT {} ROWS ONLY", n),
            _ => format!("LIMIT {}", n),
        },
        (None, Some(m)) => match rules.limit_style {
            LimitStyle::Standard => format!("OFFSET {}", m),
            // 2^64-1: MySQL's own documented "all remaining rows" sentinel.
            LimitStyle::MySql => format!("LIMIT 18446744073709551615 OFFSET {}", m),
            LimitStyle::Sqlite => format!("LIMIT -1 OFFSET {}", m),
            LimitStyle::OffsetFetch => format!("OFFSET {} ROWS", m),
        },
        (Some(n), Some(m)) => match rules.limit_style {
            LimitStyle::OffsetFetch => {
                format!("OFFSET {} ROWS FETCH NEXT {} ROWS ONLY", m, n)
            }
            _ => format!("LIMIT {} OFFSET {}", n, m),
        },
    }
}

/// Plain-English description of the LIMIT/OFFSET pair.
pub fn pagination_summary(limit: Option<u64>, offset: Option<u64>) -> String {
    match (limit, offset.filter(|o| *o > 0)) {
        (None, None) => "Every matching row".to_string(),
        (Some(n), None) => format!("The first {} row(s)", n),
        (None, Some(m)) => format!("Every row after the first {}", m),
        (Some(n), Some(m)) => format!(
            "Rows {} to {}",
            m.saturating_add(1),
            m.saturating_add(n)
        ),
    }
}

/// The whole statement, or `None` when there is not yet enough to build one.
///
/// Returning `None` rather than a comment or a fragment is deliberate: the
/// preview then shows a hint instead of something that looks runnable but is
/// not.
pub fn generate_sql(
    quote: &impl Fn(&str) -> String,
    escape: &impl Fn(&str) -> String,
    rules: DialectRules,
    state: &BuilderState,
) -> Option<String> {
    let first = state.tables.first()?;
    let active = active_aliases(state);

    let mut parts = vec![
        select_clause(quote, state, &active),
        from_clause(quote, first),
    ];
    parts.extend(join_clauses(quote, state, &active));

    for clause in [
        where_clause(quote, escape, state, &active),
        group_by_clause(quote, state, &active),
        having_clause(quote, escape, state, &active),
        order_by_clause(quote, state, &active),
        limit_offset_clause(rules, state.limit, state.offset),
    ] {
        if !clause.is_empty() {
            parts.push(clause);
        }
    }

    Some(format!("{};", parts.join("\n")))
}

/// Everything wrong or surprising about the current state, in the order the
/// user would want to fix it.
///
/// These are warnings, not errors: generation already drops anything that
/// would produce invalid SQL, so the point of the list is to explain why a
/// clause the user built is not in the preview.
pub fn validate(state: &BuilderState, rules: DialectRules) -> Vec<String> {
    let mut out = Vec::new();
    if state.tables.is_empty() {
        out.push("Add a table to start building a query.".to_string());
        return out;
    }
    let active = active_aliases(state);

    for t in &state.tables {
        if !active.iter().any(|a| a == &t.alias) {
            out.push(format!(
                "{} is not joined to the query yet, so it and everything using it is left out.",
                t.alias
            ));
        }
    }

    for (i, join) in state.joins.iter().enumerate() {
        if !join.is_complete() {
            out.push(format!("Join {} is missing a column and is left out.", i + 1));
            continue;
        }
        if join.join_type == JoinType::Cross {
            out.push(format!(
                "CROSS JOIN on {} produces every combination of rows; the result can be very large.",
                join.right_table
            ));
        }
        if join.join_type == JoinType::FullOuter && !rules.supports_full_outer {
            out.push("This database has no FULL OUTER JOIN.".to_string());
        }
        if join.join_type == JoinType::Right && !rules.supports_right_join {
            out.push("RIGHT JOIN needs SQLite 3.39 or newer.".to_string());
        }
    }

    for c in &state.columns {
        if !c.is_usable() && c.column == STAR && c.aggregate.is_some() {
            out.push(format!(
                "{}.* cannot be used with {}; only COUNT works on all columns.",
                c.table_alias,
                c.aggregate.label()
            ));
        }
    }
    if state
        .columns
        .iter()
        .enumerate()
        .any(|(i, c)| c.distinct && !c.aggregate.is_some() && !state.distinct && i > 0)
    {
        out.push(
            "Per-column DISTINCT only applies to the first output column; use COUNT DISTINCT or the query-level DISTINCT instead."
                .to_string(),
        );
    }

    // Duplicate output names are legal but make the result grid ambiguous.
    let mut names: Vec<String> = Vec::new();
    for c in state.columns.iter().filter(|c| !c.alias.is_empty()) {
        if names.contains(&c.alias) {
            out.push(format!("Two output columns are both named {}.", c.alias));
        } else {
            names.push(c.alias.clone());
        }
    }

    // The classic GROUP BY mistake: an ungrouped, unaggregated column.
    if !state.group_by.is_empty() {
        for c in state.columns.iter().filter(|c| c.is_usable()) {
            if c.aggregate.is_some() {
                continue;
            }
            let grouped = state
                .group_by
                .iter()
                .any(|g| g.table_alias == c.table_alias && g.column == c.column);
            if !grouped {
                out.push(format!(
                    "{}.{} must be in GROUP BY or use an aggregate.",
                    c.table_alias, c.column
                ));
            }
        }
    }
    if !state.having.is_empty() && state.group_by.is_empty() {
        out.push("HAVING without GROUP BY applies to the whole result set.".to_string());
    }

    if rules.limit_style == LimitStyle::OffsetFetch
        && (state.limit.is_some() || state.offset.is_some())
        && state.order_by.is_empty()
    {
        out.push("SQL Server needs an ORDER BY before it will accept OFFSET/FETCH.".to_string());
    }

    out
}

/// Table and column names for the preview editor's completion.
fn schema_index(state: &BuilderState) -> sql_editor::SchemaIndex {
    let mut index = sql_editor::SchemaIndex::default();
    for t in &state.tables {
        index.tables.insert(t.name.clone());
        index.tables.insert(t.alias.clone());
        for (name, _) in &t.columns {
            index.columns.insert(name.clone());
        }
    }
    index
}

// ---------------------------------------------------------------------------
// Screen state
// ---------------------------------------------------------------------------

/// Which sections are expanded. All open initially, as in the React builder.
#[derive(Debug, Clone, Copy)]
pub struct Sections {
    pub tables: bool,
    pub joins: bool,
    pub columns: bool,
    pub filters: bool,
    pub group_by: bool,
    pub order_by: bool,
    pub limit: bool,
}

impl Default for Sections {
    fn default() -> Self {
        Self {
            tables: true,
            joins: true,
            columns: true,
            filters: true,
            group_by: false,
            order_by: false,
            limit: false,
        }
    }
}

/// The table list and the schema's foreign keys, loaded together so the join
/// suggestions arrive with the tables they describe.
type Catalog = (Vec<TableInfo>, Vec<ForeignKeyInfo>);

pub struct QueryBuilder {
    pub open: bool,
    pub connection_id: Option<String>,
    pub schema: String,

    pub state: BuilderState,
    pub sections: Sections,

    /// Tables offered in the picker, and the schema's foreign keys.
    pub catalog: Vec<TableInfo>,
    pub foreign_keys: Vec<ForeignKeyInfo>,
    pub table_search: String,

    /// The generated statement, kept so hand edits survive until the builder
    /// state next changes.
    pub preview: String,
    generated: Option<String>,
    pub editor: sql_editor::EditorState,

    pub result: Option<QueryResult>,
    pub grid: results_grid::GridState,
    pub elapsed_ms: Option<u128>,
    pub last_error: Option<String>,

    pub catalog_job: Job<Result<Catalog, DbError>>,
    pub add_job: Job<Result<TableSchema, DbError>>,
    pub run_job: Job<(Result<QueryResult, DbError>, u128)>,
    /// Alias reserved for the table whose schema is loading.
    pending_add: Option<(String, String)>,
    needs_catalog: bool,
}

impl Default for QueryBuilder {
    fn default() -> Self {
        Self {
            open: false,
            connection_id: None,
            schema: String::new(),
            state: BuilderState::default(),
            sections: Sections::default(),
            catalog: Vec::new(),
            foreign_keys: Vec::new(),
            table_search: String::new(),
            preview: String::new(),
            generated: None,
            editor: sql_editor::EditorState::default(),
            result: None,
            grid: results_grid::GridState::default(),
            elapsed_ms: None,
            last_error: None,
            catalog_job: Job::default(),
            add_job: Job::default(),
            run_job: Job::default(),
            pending_add: None,
            needs_catalog: false,
        }
    }
}

impl QueryBuilder {
    /// Point the builder at a connection and schema.
    ///
    /// Reopening the same target keeps the query the user was building;
    /// switching target starts clean, since aliases and columns from another
    /// schema would not resolve.
    pub fn open_for(&mut self, connection_id: impl Into<String>, schema: impl Into<String>) {
        let (c, s) = (connection_id.into(), schema.into());
        if self.connection_id.as_deref() == Some(c.as_str()) && self.schema == s {
            self.open = true;
            return;
        }
        *self = Self {
            open: true,
            connection_id: Some(c),
            schema: s,
            needs_catalog: true,
            ..Self::default()
        };
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// Throw the query away, keeping the loaded catalog.
    pub fn reset(&mut self) {
        self.state = BuilderState::default();
        self.result = None;
        self.grid = results_grid::GridState::default();
        self.elapsed_ms = None;
        self.last_error = None;
        self.preview.clear();
        self.generated = None;
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// Clone the driver out of state so the lock is dropped before any await.
fn driver(app: &DbHiveApp, connection_id: &str) -> Option<Arc<dyn DatabaseDriver>> {
    app.state
        .lock()
        .unwrap()
        .get_connection(connection_id)
        .cloned()
}

fn profile_driver(app: &DbHiveApp, connection_id: &str) -> Option<DbDriver> {
    let guard = app.state.lock().unwrap();
    guard.get_profile(connection_id).map(|p| p.driver.clone())
}

/// Intent recorded while drawing, applied once the borrows have ended.
enum Intent {
    AddTable(String),
    RemoveTable(String),
    AddJoin(Join),
    Run,
    UseSql,
    Reset,
    Close,
    Reload,
}

pub fn show(app: &mut DbHiveApp, ui: &mut egui::Ui) -> BuilderAction {
    let palette = app.mode.palette();
    let mut action = BuilderAction::None;

    let Some(connection_id) = app.screens.query_builder.connection_id.clone() else {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(crate::theme::text::muted(
                palette,
                "Connect to a database to build a query",
            ));
        });
        return action;
    };

    poll(app);

    let db_driver = profile_driver(app, &connection_id);
    if let Some(d) = &db_driver {
        if !supports_builder(d) {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.label(crate::theme::text::strong(
                    palette,
                    "This connection does not speak SQL",
                ));
                ui.label(crate::theme::text::muted(
                    palette,
                    "The visual builder composes SELECT statements, which MongoDB and Redis do not accept.",
                ));
                if ui.button("Close").clicked() {
                    app.screens.query_builder.close();
                }
            });
            return action;
        }
    }
    let rules = db_driver.as_ref().map(rules_for).unwrap_or_default();

    let Some(d) = driver(app, &connection_id) else {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.colored_label(palette.destructive, "Connection is no longer open");
            if ui.button("Close").clicked() {
                app.screens.query_builder.close();
            }
        });
        return action;
    };
    let quote = {
        let d = d.clone();
        move |s: &str| d.quote_identifier(s)
    };
    let escape = {
        let d = d.clone();
        move |s: &str| d.escape_string_literal(s)
    };

    // Regenerate before drawing so the preview reflects this frame's state.
    // Hand edits are kept until the builder state itself changes, so tweaking
    // the statement and running it is possible without losing the tweak.
    {
        let qb = &mut app.screens.query_builder;
        let generated = generate_sql(&quote, &escape, rules, &qb.state);
        if generated != qb.generated {
            qb.preview = generated.clone().unwrap_or_default();
            qb.generated = generated;
        }
    }

    let warnings = validate(&app.screens.query_builder.state, rules);
    let mut intents: Vec<Intent> = Vec::new();

    header(app, ui, &mut intents, &warnings);
    ui.add_space(4.0);
    ui.separator();

    egui::SidePanel::left("query_builder_sections")
        .resizable(true)
        .default_width(430.0)
        .min_width(320.0)
        .frame(
            egui::Frame::none()
                .fill(palette.sidebar)
                .inner_margin(egui::Margin::same(10.0)),
        )
        .show_inside(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("qb_sections_scroll")
                .show(ui, |ui| {
                    tables_section(app, ui, &mut intents);
                    joins_section(app, ui, &mut intents);
                    columns_section(app, ui);
                    filters_section(app, ui);
                    group_by_section(app, ui);
                    order_by_section(app, ui);
                    limit_section(app, ui, rules);
                });
        });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        preview_and_results(app, ui, &mut intents, &warnings);
    });

    for intent in intents {
        match intent {
            Intent::AddTable(name) => start_add_table(app, ui.ctx(), &connection_id, name),
            Intent::RemoveTable(alias) => app.screens.query_builder.state.remove_table(&alias),
            Intent::AddJoin(join) => app.screens.query_builder.state.joins.push(join),
            Intent::Run => run(app, ui.ctx(), &connection_id),
            Intent::UseSql => {
                let sql = app.screens.query_builder.preview.clone();
                if sql.trim().is_empty() {
                    app.toasts.info("Nothing to send yet");
                } else {
                    action = BuilderAction::UseSql(sql);
                }
            }
            Intent::Reset => app.screens.query_builder.reset(),
            Intent::Close => app.screens.query_builder.close(),
            Intent::Reload => {
                app.screens.query_builder.needs_catalog = true;
            }
        }
    }

    start_pending(app, ui.ctx(), &connection_id);
    action
}

fn header(app: &mut DbHiveApp, ui: &mut egui::Ui, intents: &mut Vec<Intent>, warnings: &[String]) {
    let palette = app.mode.palette();
    let qb = &app.screens.query_builder;
    let schema = qb.schema.clone();
    let busy = qb.catalog_job.is_running() || qb.add_job.is_running();
    let running = qb.run_job.is_running();
    let has_sql = !qb.preview.trim().is_empty();

    ui.horizontal(|ui| {
        ui.label(crate::theme::text::heading(palette, "Query Builder"));
        if !schema.is_empty() {
            ui.label(crate::theme::text::muted(palette, schema));
        }
        if busy {
            ui.add(egui::Spinner::new().size(14.0));
        }
        if !warnings.is_empty() {
            ui.label(
                egui::RichText::new(format!("{} note(s)", warnings.len()))
                    .small()
                    .color(palette.warning),
            );
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() {
                intents.push(Intent::Close);
            }
            if ui
                .add_enabled(has_sql, egui::Button::new("Open in SQL editor"))
                .on_hover_text("Send this statement to a query tab")
                .clicked()
            {
                intents.push(Intent::UseSql);
            }
            if ui
                .add_enabled(has_sql && !running, egui::Button::new("Run"))
                .on_hover_text("Execute the generated statement here")
                .clicked()
            {
                intents.push(Intent::Run);
            }
            if running {
                ui.add(egui::Spinner::new().size(14.0));
            }
            if ui
                .add_enabled(!busy, egui::Button::new("Reset"))
                .on_hover_text("Clear every clause")
                .clicked()
            {
                intents.push(Intent::Reset);
            }
        });
    });
}

/// A collapsible section header with a count badge, matching the web layout.
fn section<R>(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    id: &str,
    title: &str,
    count: usize,
    open: &mut bool,
    body: impl FnOnce(&mut egui::Ui) -> R,
) {
    ui.horizontal(|ui| {
        let arrow = if *open { "▾" } else { "▸" };
        let label = if count == 0 {
            format!("{}  {}", arrow, title)
        } else {
            format!("{}  {}  ({})", arrow, title, count)
        };
        if ui
            .add(
                egui::Label::new(crate::theme::text::strong(palette, label))
                    .sense(egui::Sense::click()),
            )
            .clicked()
        {
            *open = !*open;
        }
    });
    if *open {
        egui::Frame::none()
            .fill(palette.card)
            .stroke(egui::Stroke::new(1.0_f32, palette.border))
            .rounding(egui::Rounding::same(6.0))
            .inner_margin(egui::Margin::same(8.0))
            .show(ui, |ui| {
                ui.push_id(id, |ui| {
                    ui.set_width(ui.available_width());
                    body(ui);
                });
            });
    }
    ui.add_space(8.0);
}

fn tables_section(app: &mut DbHiveApp, ui: &mut egui::Ui, intents: &mut Vec<Intent>) {
    let palette = app.mode.palette();
    let qb = &mut app.screens.query_builder;
    let mut open = qb.sections.tables;
    let count = qb.state.tables.len();
    let loading = qb.catalog_job.is_running();
    let catalog: Vec<String> = qb.catalog.iter().map(|t| t.name.clone()).collect();
    let chosen: Vec<(String, String, usize)> = qb
        .state
        .tables
        .iter()
        .map(|t| (t.alias.clone(), t.name.clone(), t.columns.len()))
        .collect();
    let adding = qb.add_job.is_running();

    section(ui, palette, "qb_tables", "Tables", count, &mut open, |ui| {
        for (i, (alias, name, cols)) in chosen.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(crate::theme::text::strong(palette, name.clone()));
                if alias != name {
                    ui.label(crate::theme::text::muted(palette, format!("AS {}", alias)));
                }
                if i == 0 {
                    ui.label(crate::theme::text::muted(palette, "FROM"));
                }
                ui.label(crate::theme::text::muted(
                    palette,
                    format!("{} column(s)", cols),
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("Remove")
                        .on_hover_text("Also removes its columns, joins, grouping and filters")
                        .clicked()
                    {
                        intents.push(Intent::RemoveTable(alias.clone()));
                    }
                });
            });
        }
        if !chosen.is_empty() {
            ui.add_space(6.0);
            ui.separator();
        }

        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut qb.table_search)
                    .hint_text("Search tables...")
                    .desired_width(180.0),
            );
            if ui.small_button("Reload").clicked() {
                intents.push(Intent::Reload);
            }
            if loading {
                ui.add(egui::Spinner::new().size(12.0));
            }
        });

        if catalog.is_empty() {
            ui.label(crate::theme::text::muted(
                palette,
                if loading {
                    "Loading tables"
                } else {
                    "No tables in this schema"
                },
            ));
            return;
        }

        let needle = qb.table_search.to_lowercase();
        egui::ScrollArea::vertical()
            .id_salt("qb_table_list")
            .max_height(180.0)
            .show(ui, |ui| {
                for name in catalog
                    .iter()
                    .filter(|n| needle.is_empty() || n.to_lowercase().contains(&needle))
                {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!adding, egui::Button::new("+").small())
                            .on_hover_text("Add to the query")
                            .clicked()
                        {
                            intents.push(Intent::AddTable(name.clone()));
                        }
                        ui.label(name);
                    });
                }
            });
        ui.label(crate::theme::text::muted(
            palette,
            "Adding the same table twice gives it a second alias, for self-joins.",
        ));
    });

    app.screens.query_builder.sections.tables = open;
}

fn joins_section(app: &mut DbHiveApp, ui: &mut egui::Ui, intents: &mut Vec<Intent>) {
    let palette = app.mode.palette();
    let qb = &mut app.screens.query_builder;
    if qb.state.tables.len() < 2 {
        return;
    }
    let mut open = qb.sections.joins;
    let count = qb.state.joins.len();

    // Snapshots so the pickers can be drawn while the joins are borrowed.
    let aliases: Vec<String> = qb.state.tables.iter().map(|t| t.alias.clone()).collect();
    let columns: Vec<(String, Vec<String>)> = qb
        .state
        .tables
        .iter()
        .map(|t| {
            (
                t.alias.clone(),
                t.columns.iter().map(|(n, _)| n.clone()).collect(),
            )
        })
        .collect();
    let suggestions = join_suggestions(&qb.state, &qb.foreign_keys);

    let mut remove: Option<usize> = None;
    section(ui, palette, "qb_joins", "Joins", count, &mut open, |ui| {
        for (i, join) in qb.state.joins.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt(("qb_join_type", i))
                    .selected_text(join.join_type.sql())
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for t in JoinType::ALL {
                            ui.selectable_value(&mut join.join_type, t, t.sql());
                        }
                    });
                egui::ComboBox::from_id_salt(("qb_join_right", i))
                    .selected_text(join.right_table.clone())
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        for a in &aliases {
                            ui.selectable_value(&mut join.right_table, a.clone(), a);
                        }
                    });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                });
            });

            if join.join_type.takes_on() {
                ui.horizontal(|ui| {
                    ui.label(crate::theme::text::muted(palette, "ON"));
                    alias_column_picker(
                        ui,
                        ("qb_join_lt", i),
                        &aliases,
                        &columns,
                        &mut join.left_table,
                        &mut join.left_column,
                    );
                    ui.label("=");
                    let right_alias = join.right_table.clone();
                    column_picker(
                        ui,
                        ("qb_join_rc", i),
                        columns_for(&columns, &right_alias),
                        &mut join.right_column,
                    );
                });
            } else {
                ui.label(
                    egui::RichText::new(
                        "CROSS JOIN pairs every row with every row; no ON condition.",
                    )
                    .small()
                    .color(palette.warning),
                );
            }
            ui.add_space(4.0);
            ui.separator();
        }

        ui.horizontal(|ui| {
            if ui.button("Add join").clicked() {
                let left = aliases.first().cloned().unwrap_or_default();
                let right = aliases
                    .iter()
                    .find(|a| !qb.state.joins.iter().any(|j| &&j.right_table == a) && **a != left)
                    .cloned()
                    .unwrap_or_default();
                intents.push(Intent::AddJoin(Join::new(left, right)));
            }
        });

        if !suggestions.is_empty() {
            ui.add_space(4.0);
            ui.label(crate::theme::text::muted(
                palette,
                "Suggested from foreign keys",
            ));
            for join in &suggestions {
                ui.horizontal(|ui| {
                    if ui.small_button("Use").clicked() {
                        intents.push(Intent::AddJoin(join.clone()));
                    }
                    ui.label(crate::theme::text::muted(
                        palette,
                        format!(
                            "{}.{} = {}.{}",
                            join.left_table, join.left_column, join.right_table, join.right_column
                        ),
                    ));
                });
            }
        }
    });

    if let Some(i) = remove {
        app.screens.query_builder.state.joins.remove(i);
    }
    app.screens.query_builder.sections.joins = open;
}

/// Joins the schema's foreign keys imply between tables already chosen, minus
/// the ones already written.
fn join_suggestions(state: &BuilderState, keys: &[ForeignKeyInfo]) -> Vec<Join> {
    let mut out: Vec<Join> = Vec::new();
    for fk in keys {
        let (Some(from), Some(to)) = (fk.columns.first(), fk.referenced_columns.first()) else {
            continue;
        };
        for left in &state.tables {
            if left.name != fk.table {
                continue;
            }
            for right in &state.tables {
                if right.name != fk.referenced_table || right.alias == left.alias {
                    continue;
                }
                let already = state.joins.iter().any(|j| {
                    j.left_table == left.alias
                        && j.right_table == right.alias
                        && j.left_column == *from
                });
                if already {
                    continue;
                }
                let mut join = Join::new(left.alias.clone(), right.alias.clone());
                join.left_column = from.clone();
                join.right_column = to.clone();
                if !out.contains(&join) {
                    out.push(join);
                }
            }
        }
    }
    out.truncate(8);
    out
}

fn columns_for<'a>(columns: &'a [(String, Vec<String>)], alias: &str) -> &'a [String] {
    columns
        .iter()
        .find(|(a, _)| a == alias)
        .map(|(_, c)| c.as_slice())
        .unwrap_or(&[])
}

fn column_picker(
    ui: &mut egui::Ui,
    id: (&str, usize),
    columns: &[String],
    target: &mut String,
) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(if target.is_empty() {
            "column".to_string()
        } else {
            target.clone()
        })
        .width(140.0)
        .show_ui(ui, |ui| {
            for c in columns {
                ui.selectable_value(target, c.clone(), c);
            }
        });
}

/// Alias picker followed by a column picker scoped to that alias.
fn alias_column_picker(
    ui: &mut egui::Ui,
    id: (&str, usize),
    aliases: &[String],
    columns: &[(String, Vec<String>)],
    alias: &mut String,
    column: &mut String,
) {
    let before = alias.clone();
    egui::ComboBox::from_id_salt((id.0, id.1, "a"))
        .selected_text(if alias.is_empty() {
            "table".to_string()
        } else {
            alias.clone()
        })
        .width(110.0)
        .show_ui(ui, |ui| {
            for a in aliases {
                ui.selectable_value(alias, a.clone(), a);
            }
        });
    // A column from the old table would not exist on the new one.
    if *alias != before {
        column.clear();
    }
    column_picker(ui, (id.0, id.1), columns_for(columns, alias), column);
}

fn columns_section(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    let qb = &mut app.screens.query_builder;
    if qb.state.tables.is_empty() {
        return;
    }
    let mut open = qb.sections.columns;
    let count = qb.state.columns.len();

    let aliases: Vec<String> = qb.state.tables.iter().map(|t| t.alias.clone()).collect();
    let available: Vec<(String, Vec<String>)> = qb
        .state
        .tables
        .iter()
        .map(|t| {
            let mut cols = vec![STAR.to_string()];
            cols.extend(t.columns.iter().map(|(n, _)| n.clone()));
            (t.alias.clone(), cols)
        })
        .collect();

    let mut remove: Option<usize> = None;
    let mut add: Option<SelectedColumn> = None;
    let global_distinct = qb.state.distinct;

    section(ui, palette, "qb_columns", "Columns", count, &mut open, |ui| {
        if qb.state.columns.is_empty() {
            ui.label(crate::theme::text::muted(
                palette,
                "No columns picked, so the query selects every column.",
            ));
        }

        for (i, col) in qb.state.columns.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                alias_column_picker(
                    ui,
                    ("qb_col", i),
                    &aliases,
                    &available,
                    &mut col.table_alias,
                    &mut col.column,
                );
                egui::ComboBox::from_id_salt(("qb_col_agg", i))
                    .selected_text(col.aggregate.label())
                    .width(130.0)
                    .show_ui(ui, |ui| {
                        for a in Aggregate::ALL {
                            ui.selectable_value(&mut col.aggregate, a, a.label());
                        }
                    });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut col.alias)
                        .hint_text("output name (optional)")
                        .desired_width(180.0),
                );
                ui.add_enabled(
                    !col.aggregate.is_some() && !global_distinct,
                    egui::Checkbox::new(&mut col.distinct, "DISTINCT"),
                )
                .on_hover_text("Applies to the first output column only");
            });
            ui.add_space(4.0);
            ui.separator();
        }

        ui.label(crate::theme::text::muted(palette, "Add a column"));
        egui::ScrollArea::vertical()
            .id_salt("qb_col_available")
            .max_height(150.0)
            .show(ui, |ui| {
                for (alias, cols) in &available {
                    for c in cols {
                        let already = qb
                            .state
                            .columns
                            .iter()
                            .any(|s| &s.table_alias == alias && &s.column == c);
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(!already, egui::Button::new("+").small())
                                .clicked()
                            {
                                add = Some(SelectedColumn::new(alias.clone(), c.clone()));
                            }
                            let label = if c == STAR {
                                format!("{}.* (all columns)", alias)
                            } else {
                                format!("{}.{}", alias, c)
                            };
                            ui.label(crate::theme::text::muted(palette, label));
                        });
                    }
                }
            });
    });

    let qb = &mut app.screens.query_builder;
    if let Some(i) = remove {
        qb.state.columns.remove(i);
    }
    if let Some(c) = add {
        qb.state.columns.push(c);
    }
    qb.sections.columns = open;
}

fn filters_section(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    let qb = &mut app.screens.query_builder;
    if qb.state.tables.is_empty() {
        return;
    }
    let mut open = qb.sections.filters;
    let count = count_conditions(&qb.state.filters);

    let aliases: Vec<String> = qb.state.tables.iter().map(|t| t.alias.clone()).collect();
    let typed: Vec<(String, Vec<(String, String)>)> = qb
        .state
        .tables
        .iter()
        .map(|t| (t.alias.clone(), t.columns.clone()))
        .collect();

    section(ui, palette, "qb_filters", "Filters", count, &mut open, |ui| {
        let group = std::mem::take(&mut qb.state.filters);
        let mut group = group;
        draw_group(ui, palette, &mut group, &aliases, &typed, 0, "g");
        qb.state.filters = group;
    });

    app.screens.query_builder.sections.filters = open;
}

fn count_conditions(group: &ConditionGroup) -> usize {
    group.conditions.len() + group.groups.iter().map(count_conditions).sum::<usize>()
}

/// One AND/OR group, recursively. `path` keeps widget ids unique per nesting
/// position, since egui identifies widgets by id, not by position in a tree.
fn draw_group(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    group: &mut ConditionGroup,
    aliases: &[String],
    typed: &[(String, Vec<(String, String)>)],
    depth: usize,
    path: &str,
) {
    ui.horizontal(|ui| {
        ui.label(crate::theme::text::muted(palette, "Match"));
        egui::ComboBox::from_id_salt((path, "op"))
            .selected_text(match group.operator {
                Logical::And => "all (AND)",
                Logical::Or => "any (OR)",
            })
            .width(110.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut group.operator, Logical::And, "all (AND)");
                ui.selectable_value(&mut group.operator, Logical::Or, "any (OR)");
            });
    });

    let mut remove: Option<usize> = None;
    for (i, cond) in group.conditions.iter_mut().enumerate() {
        let id = format!("{}c{}", path, i);
        ui.horizontal_wrapped(|ui| {
            let before = cond.table_alias.clone();
            egui::ComboBox::from_id_salt((id.clone(), "a"))
                .selected_text(if cond.table_alias.is_empty() {
                    "table".to_string()
                } else {
                    cond.table_alias.clone()
                })
                .width(100.0)
                .show_ui(ui, |ui| {
                    for a in aliases {
                        ui.selectable_value(&mut cond.table_alias, a.clone(), a);
                    }
                });
            if cond.table_alias != before {
                cond.column.clear();
            }

            let cols = typed
                .iter()
                .find(|(a, _)| a == &cond.table_alias)
                .map(|(_, c)| c.as_slice())
                .unwrap_or(&[]);
            let before_col = cond.column.clone();
            egui::ComboBox::from_id_salt((id.clone(), "c"))
                .selected_text(if cond.column.is_empty() {
                    "column".to_string()
                } else {
                    cond.column.clone()
                })
                .width(130.0)
                .show_ui(ui, |ui| {
                    for (name, _) in cols {
                        ui.selectable_value(&mut cond.column, name.clone(), name);
                    }
                });
            if cond.column != before_col {
                // The value editor and the literal form both follow the
                // column's declared type, so it is re-read on every change.
                cond.kind = cols
                    .iter()
                    .find(|(n, _)| n == &cond.column)
                    .map(|(_, t)| ValueKind::from_data_type(t))
                    .unwrap_or_default();
            }

            egui::ComboBox::from_id_salt((id.clone(), "o"))
                .selected_text(cond.op.sql())
                .width(110.0)
                .show_ui(ui, |ui| {
                    for op in CompareOp::ALL {
                        ui.selectable_value(&mut cond.op, op, op.sql());
                    }
                });

            value_editor(ui, palette, cond, &id);

            if ui.small_button("Remove").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        group.conditions.remove(i);
    }

    let mut remove_group: Option<usize> = None;
    for (i, nested) in group.groups.iter_mut().enumerate() {
        let id = format!("{}g{}", path, i);
        egui::Frame::none()
            .fill(palette.secondary)
            .rounding(egui::Rounding::same(6.0))
            .inner_margin(egui::Margin::same(6.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(crate::theme::text::muted(palette, "Nested group"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Remove group").clicked() {
                            remove_group = Some(i);
                        }
                    });
                });
                draw_group(ui, palette, nested, aliases, typed, depth + 1, &id);
            });
        ui.add_space(4.0);
    }
    if let Some(i) = remove_group {
        group.groups.remove(i);
    }

    ui.horizontal(|ui| {
        if ui.button("Add condition").clicked() {
            let alias = aliases.first().cloned().unwrap_or_default();
            let (column, kind) = typed
                .iter()
                .find(|(a, _)| a == &alias)
                .and_then(|(_, c)| c.first())
                .map(|(n, t)| (n.clone(), ValueKind::from_data_type(t)))
                .unwrap_or_default();
            group.conditions.push(Condition::new(alias, column, kind));
        }
        if depth + 1 < MAX_GROUP_DEPTH {
            if ui
                .button("Add group")
                .on_hover_text("A nested set of conditions with its own AND/OR")
                .clicked()
            {
                group.groups.push(ConditionGroup::default());
            }
        } else {
            ui.label(crate::theme::text::muted(
                palette,
                format!("Nesting stops at {} levels", MAX_GROUP_DEPTH),
            ));
        }
    });
}

/// The value editor the operator and the column's type ask for.
fn value_editor(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    cond: &mut Condition,
    id: &str,
) {
    match cond.op.arity() {
        Arity::None => {
            ui.label(crate::theme::text::muted(palette, "no value"));
        }
        Arity::List => {
            ui.add(
                egui::TextEdit::singleline(&mut cond.value)
                    .id_salt(format!("{}v", id))
                    .hint_text("a, b, c")
                    .desired_width(160.0),
            );
        }
        Arity::Range => {
            ui.add(
                egui::TextEdit::singleline(&mut cond.value)
                    .id_salt(format!("{}v", id))
                    .hint_text("from")
                    .desired_width(80.0),
            );
            ui.label(crate::theme::text::muted(palette, "and"));
            ui.add(
                egui::TextEdit::singleline(&mut cond.value2)
                    .id_salt(format!("{}v2", id))
                    .hint_text("to")
                    .desired_width(80.0),
            );
        }
        Arity::One => {
            if cond.kind == ValueKind::Bool && !matches!(cond.op, CompareOp::Like | CompareOp::NotLike)
            {
                let mut on = matches!(cond.value.to_ascii_lowercase().as_str(), "true" | "t" | "1");
                if ui.checkbox(&mut on, "true").changed() {
                    cond.value = if on { "true" } else { "false" }.to_string();
                }
                if cond.value.is_empty() {
                    cond.value = "false".to_string();
                }
            } else {
                ui.add(
                    egui::TextEdit::singleline(&mut cond.value)
                        .id_salt(format!("{}v", id))
                        .hint_text("value")
                        .desired_width(140.0),
                );
            }
        }
    }
}

fn group_by_section(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    let qb = &mut app.screens.query_builder;
    if qb.state.tables.is_empty() {
        return;
    }
    let mut open = qb.sections.group_by;
    let count = qb.state.group_by.len() + qb.state.having.len();

    let aliases: Vec<String> = qb.state.tables.iter().map(|t| t.alias.clone()).collect();
    let columns: Vec<(String, Vec<String>)> = qb
        .state
        .tables
        .iter()
        .map(|t| {
            (
                t.alias.clone(),
                t.columns.iter().map(|(n, _)| n.clone()).collect(),
            )
        })
        .collect();

    let mut remove_group: Option<usize> = None;
    let mut remove_having: Option<usize> = None;
    let mut add_group = false;
    let mut add_having = false;

    section(
        ui,
        palette,
        "qb_group_by",
        "Group by / having",
        count,
        &mut open,
        |ui| {
            for (i, g) in qb.state.group_by.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    alias_column_picker(
                        ui,
                        ("qb_gb", i),
                        &aliases,
                        &columns,
                        &mut g.table_alias,
                        &mut g.column,
                    );
                    if ui.small_button("Remove").clicked() {
                        remove_group = Some(i);
                    }
                });
            }
            if ui.button("Add group by").clicked() {
                add_group = true;
            }

            ui.add_space(6.0);
            ui.separator();
            ui.label(crate::theme::text::muted(palette, "HAVING"));

            for (i, h) in qb.state.having.iter_mut().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt(("qb_hav_agg", i))
                        .selected_text(h.aggregate.label())
                        .width(130.0)
                        .show_ui(ui, |ui| {
                            for a in Aggregate::FOR_HAVING {
                                ui.selectable_value(&mut h.aggregate, a, a.label());
                            }
                        });
                    alias_column_picker(
                        ui,
                        ("qb_hav", i),
                        &aliases,
                        &columns,
                        &mut h.table_alias,
                        &mut h.column,
                    );
                    egui::ComboBox::from_id_salt(("qb_hav_op", i))
                        .selected_text(h.op.sql())
                        .width(70.0)
                        .show_ui(ui, |ui| {
                            for op in CompareOp::FOR_HAVING {
                                ui.selectable_value(&mut h.op, op, op.sql());
                            }
                        });
                    ui.add(
                        egui::TextEdit::singleline(&mut h.value)
                            .hint_text("value")
                            .desired_width(90.0),
                    );
                    if ui.small_button("Remove").clicked() {
                        remove_having = Some(i);
                    }
                });
            }
            if ui.button("Add having").clicked() {
                add_having = true;
            }
        },
    );

    let qb = &mut app.screens.query_builder;
    if let Some(i) = remove_group {
        qb.state.group_by.remove(i);
    }
    if let Some(i) = remove_having {
        qb.state.having.remove(i);
    }
    if add_group {
        let (alias, column) = first_column(&qb.state);
        qb.state.group_by.push(GroupByClause {
            table_alias: alias,
            column,
        });
    }
    if add_having {
        let (alias, column) = first_column(&qb.state);
        qb.state.having.push(HavingCondition::new(alias, column));
    }
    qb.sections.group_by = open;
}

fn first_column(state: &BuilderState) -> (String, String) {
    match state.tables.first() {
        Some(t) => (
            t.alias.clone(),
            t.columns
                .first()
                .map(|(n, _)| n.clone())
                .unwrap_or_default(),
        ),
        None => (String::new(), String::new()),
    }
}

fn order_by_section(app: &mut DbHiveApp, ui: &mut egui::Ui) {
    let palette = app.mode.palette();
    let qb = &mut app.screens.query_builder;
    if qb.state.tables.is_empty() {
        return;
    }
    let mut open = qb.sections.order_by;
    let count = qb.state.order_by.len();

    let aliases: Vec<String> = qb.state.tables.iter().map(|t| t.alias.clone()).collect();
    let columns: Vec<(String, Vec<String>)> = qb
        .state
        .tables
        .iter()
        .map(|t| {
            (
                t.alias.clone(),
                t.columns.iter().map(|(n, _)| n.clone()).collect(),
            )
        })
        .collect();

    let mut remove: Option<usize> = None;
    let mut add = false;
    let mut move_up: Option<usize> = None;

    section(ui, palette, "qb_order_by", "Order by", count, &mut open, |ui| {
        for (i, o) in qb.state.order_by.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                alias_column_picker(
                    ui,
                    ("qb_ob", i),
                    &aliases,
                    &columns,
                    &mut o.table_alias,
                    &mut o.column,
                );
                if ui
                    .small_button(o.direction.sql())
                    .on_hover_text("Toggle direction")
                    .clicked()
                {
                    o.direction = o.direction.toggled();
                }
                if ui
                    .add_enabled(i > 0, egui::Button::new("⬆").small())
                    .on_hover_text("Sort by this earlier")
                    .clicked()
                {
                    move_up = Some(i);
                }
                if ui.small_button("Remove").clicked() {
                    remove = Some(i);
                }
            });
        }
        if ui.button("Add order by").clicked() {
            add = true;
        }
    });

    let qb = &mut app.screens.query_builder;
    if let Some(i) = remove {
        qb.state.order_by.remove(i);
    }
    if let Some(i) = move_up {
        qb.state.order_by.swap(i - 1, i);
    }
    if add {
        let (table_alias, column) = first_column(&qb.state);
        qb.state.order_by.push(OrderByClause {
            table_alias,
            column,
            direction: SortDir::Asc,
        });
    }
    qb.sections.order_by = open;
}

fn limit_section(app: &mut DbHiveApp, ui: &mut egui::Ui, rules: DialectRules) {
    let palette = app.mode.palette();
    let qb = &mut app.screens.query_builder;
    if qb.state.tables.is_empty() {
        return;
    }
    let mut open = qb.sections.limit;
    let count = usize::from(qb.state.limit.is_some()) + usize::from(qb.state.offset.is_some());

    section(ui, palette, "qb_limit", "Limit / offset", count, &mut open, |ui| {
        ui.checkbox(&mut qb.state.distinct, "DISTINCT (whole result)");
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(crate::theme::text::muted(palette, "LIMIT"));
            optional_number(ui, "qb_limit_v", &mut qb.state.limit);
            ui.label(crate::theme::text::muted(palette, "OFFSET"));
            optional_number(ui, "qb_offset_v", &mut qb.state.offset);
        });
        ui.label(crate::theme::text::muted(
            palette,
            pagination_summary(qb.state.limit, qb.state.offset),
        ));
        if rules.limit_style == LimitStyle::OffsetFetch {
            ui.label(crate::theme::text::muted(
                palette,
                "SQL Server needs an ORDER BY for OFFSET/FETCH.",
            ));
        }
    });

    app.screens.query_builder.sections.limit = open;
}

/// A number box where empty means "no clause". Non-numeric text is simply not
/// accepted, so the state can never hold something that is not a number.
fn optional_number(ui: &mut egui::Ui, id: &str, target: &mut Option<u64>) {
    let mut text = target.map(|v| v.to_string()).unwrap_or_default();
    let response = ui.add(
        egui::TextEdit::singleline(&mut text)
            .id_salt(id)
            .hint_text("none")
            .desired_width(70.0),
    );
    if response.changed() {
        let trimmed = text.trim();
        *target = if trimmed.is_empty() {
            None
        } else {
            trimmed.parse::<u64>().ok().or(*target)
        };
    }
}

fn preview_and_results(
    app: &mut DbHiveApp,
    ui: &mut egui::Ui,
    intents: &mut Vec<Intent>,
    warnings: &[String],
) {
    let palette = app.mode.palette();

    if !warnings.is_empty() {
        egui::Frame::none()
            .fill(palette.secondary)
            .rounding(egui::Rounding::same(6.0))
            .inner_margin(egui::Margin::same(8.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                for w in warnings {
                    ui.label(egui::RichText::new(w).small().color(palette.warning));
                }
            });
        ui.add_space(6.0);
    }

    let index = schema_index(&app.screens.query_builder.state);
    let qb = &mut app.screens.query_builder;

    ui.horizontal(|ui| {
        ui.label(crate::theme::text::strong(palette, "SQL"));
        let edited = qb.generated.as_deref() != Some(qb.preview.as_str());
        if edited && !qb.preview.is_empty() {
            ui.label(crate::theme::text::muted(
                palette,
                "edited by hand; changing a clause regenerates it",
            ));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!qb.preview.trim().is_empty(), egui::Button::new("Copy"))
                .clicked()
            {
                let sql = qb.preview.clone();
                ui.output_mut(|o| o.copied_text = sql);
            }
        });
    });

    if qb.preview.trim().is_empty() {
        ui.add_space(12.0);
        ui.label(crate::theme::text::muted(
            palette,
            "Pick a table on the left and the statement appears here.",
        ));
    } else {
        let mut buffer = std::mem::take(&mut qb.preview);
        let action = sql_editor::show(ui, palette, &mut qb.editor, &index, &mut buffer);
        qb.preview = buffer;
        if action == sql_editor::EditorAction::Execute {
            intents.push(Intent::Run);
        }
    }

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(4.0);

    if let Some(err) = qb.last_error.clone() {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(palette.destructive, "Query failed:");
            ui.label(err);
            if ui.button("Dismiss").clicked() {
                qb.last_error = None;
            }
        });
        return;
    }

    if qb.run_job.is_running() && qb.result.is_none() {
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            ui.add(egui::Spinner::new());
            ui.label(crate::theme::text::muted(palette, "Running"));
        });
        return;
    }

    match qb.result.take() {
        Some(result) => {
            if let Some(ms) = qb.elapsed_ms {
                ui.label(crate::theme::text::muted(
                    palette,
                    format!("{} row(s) in {} ms", result.rows.len(), ms),
                ));
            }
            results_grid::show(ui, palette, &result, &mut qb.grid);
            qb.result = Some(result);
        }
        None => {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(crate::theme::text::muted(
                    palette,
                    "Run the query to see results",
                ));
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Async work
// ---------------------------------------------------------------------------

/// Drain finished jobs so this frame draws the newest data.
fn poll(app: &mut DbHiveApp) {
    let qb = &mut app.screens.query_builder;

    if let Some(result) = qb.catalog_job.take_result() {
        match result {
            Ok((tables, keys)) => {
                qb.catalog = tables;
                qb.foreign_keys = keys;
            }
            Err(e) => qb.last_error = Some(e.to_string()),
        }
    }

    if let Some(result) = qb.add_job.take_result() {
        let pending = qb.pending_add.take();
        match (result, pending) {
            (Ok(schema), Some((name, alias))) => {
                let columns = schema
                    .columns
                    .iter()
                    .map(|c| (c.name.clone(), c.data_type.clone()))
                    .collect();
                let table = QueryTable::new(alias, qb.schema.clone(), name).with_columns(columns);
                qb.state.tables.push(table);
            }
            (Err(e), _) => qb.last_error = Some(e.to_string()),
            (Ok(_), None) => {}
        }
    }

    if let Some((result, elapsed)) = qb.run_job.take_result() {
        qb.elapsed_ms = Some(elapsed);
        match result {
            Ok(r) => {
                qb.grid = results_grid::GridState::default();
                qb.result = Some(r);
                qb.last_error = None;
            }
            Err(e) => {
                qb.result = None;
                qb.last_error = Some(e.to_string());
            }
        }
    }
}

fn start_pending(app: &mut DbHiveApp, ctx: &egui::Context, connection_id: &str) {
    if !app.screens.query_builder.needs_catalog {
        return;
    }
    let Some(d) = driver(app, connection_id) else {
        app.screens.query_builder.needs_catalog = false;
        app.screens.query_builder.last_error = Some("Connection is no longer open".to_string());
        return;
    };
    let qb = &mut app.screens.query_builder;
    qb.needs_catalog = false;
    let schema = qb.schema.clone();
    let handle = app.rt.handle().clone();
    app.screens
        .query_builder
        .catalog_job
        .start(&handle, ctx, async move {
            let tables = d.get_tables(&schema).await?;
            // Suggestions are a nice-to-have; a driver that cannot report
            // foreign keys must not cost the user the table list.
            let keys = d.get_foreign_keys(&schema).await.unwrap_or_default();
            Ok((tables, keys))
        });
}

fn start_add_table(app: &mut DbHiveApp, ctx: &egui::Context, connection_id: &str, table: String) {
    if app.screens.query_builder.add_job.is_running() {
        return;
    }
    let Some(d) = driver(app, connection_id) else {
        app.screens.query_builder.last_error = Some("Connection is no longer open".to_string());
        return;
    };
    let qb = &mut app.screens.query_builder;
    let alias = qb.state.free_alias(&table);
    qb.pending_add = Some((table.clone(), alias));
    let schema = qb.schema.clone();
    let handle = app.rt.handle().clone();
    qb.add_job.start(&handle, ctx, async move {
        d.get_table_schema(&schema, &table).await
    });
}

fn run(app: &mut DbHiveApp, ctx: &egui::Context, connection_id: &str) {
    let sql = app.screens.query_builder.preview.clone();
    if sql.trim().is_empty() || app.screens.query_builder.run_job.is_running() {
        return;
    }
    let Some(d) = driver(app, connection_id) else {
        app.screens.query_builder.last_error = Some("Connection is no longer open".to_string());
        return;
    };
    let handle = app.rt.handle().clone();
    app.screens
        .query_builder
        .run_job
        .start(&handle, ctx, async move {
            let started = std::time::Instant::now();
            let out = d.execute_query(&sql).await;
            (out, started.elapsed().as_millis())
        });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Standard SQL quoting, as Postgres and SQLite use.
    fn q(ident: &str) -> String {
        format!("\"{}\"", ident.replace('"', "\"\""))
    }
    /// MySQL backtick quoting, to prove nothing assumes double quotes.
    fn q_mysql(ident: &str) -> String {
        format!("`{}`", ident.replace('`', "``"))
    }
    fn esc(v: &str) -> String {
        v.replace('\'', "''")
    }
    /// MySQL's escaping, which also has to handle the backslash.
    fn esc_mysql(v: &str) -> String {
        v.replace('\\', "\\\\").replace('\'', "''")
    }

    fn pg() -> DialectRules {
        rules_for(&DbDriver::Postgres)
    }

    fn users() -> QueryTable {
        QueryTable::new("users", "public", "users").with_columns(vec![
            ("id".into(), "integer".into()),
            ("name".into(), "text".into()),
            ("active".into(), "boolean".into()),
        ])
    }

    fn orders() -> QueryTable {
        QueryTable::new("orders", "public", "orders").with_columns(vec![
            ("id".into(), "integer".into()),
            ("user_id".into(), "integer".into()),
            ("total".into(), "numeric".into()),
        ])
    }

    /// A one-table state, the base every clause test builds on.
    fn base() -> BuilderState {
        BuilderState {
            tables: vec![users()],
            ..Default::default()
        }
    }

    /// users joined to orders, for the multi-table cases.
    fn joined() -> BuilderState {
        let mut s = base();
        s.tables.push(orders());
        s.joins.push(Join {
            join_type: JoinType::Inner,
            left_table: "users".into(),
            left_column: "id".into(),
            right_table: "orders".into(),
            right_column: "user_id".into(),
        });
        s
    }

    fn sql(state: &BuilderState) -> String {
        generate_sql(&q, &esc, pg(), state).expect("expected a statement")
    }

    // -- empty and partial state -------------------------------------------

    #[test]
    fn no_tables_produces_no_sql() {
        assert_eq!(generate_sql(&q, &esc, pg(), &BuilderState::default()), None);
    }

    #[test]
    fn a_single_table_selects_everything() {
        assert_eq!(
            sql(&base()),
            "SELECT *\nFROM \"public\".\"users\" AS \"users\";"
        );
    }

    #[test]
    fn a_half_typed_condition_is_left_out_rather_than_compared_to_empty() {
        let mut s = base();
        s.filters.conditions.push(Condition::new("users", "name", ValueKind::Text));
        // No value typed yet.
        assert!(!sql(&s).contains("WHERE"));
    }

    #[test]
    fn an_incomplete_join_leaves_its_table_and_dependents_out() {
        let mut s = base();
        s.tables.push(orders());
        // The join names both tables but neither ON column.
        s.joins.push(Join::new("users", "orders"));
        s.columns.push(SelectedColumn::new("orders", "total"));
        let out = sql(&s);
        assert!(!out.contains("JOIN"), "{}", out);
        assert!(!out.contains("orders"), "{}", out);
        // Nothing survives that would reference the undeclared alias.
        assert_eq!(out, "SELECT *\nFROM \"public\".\"users\" AS \"users\";");
    }

    #[test]
    fn a_table_nobody_joins_is_reported_and_excluded() {
        let mut s = base();
        s.tables.push(orders());
        let warnings = validate(&s, pg());
        assert!(
            warnings.iter().any(|w| w.contains("orders") && w.contains("not joined")),
            "{:?}",
            warnings
        );
        assert_eq!(active_aliases(&s), vec!["users".to_string()]);
    }

    #[test]
    fn an_in_condition_with_no_items_is_left_out_rather_than_emitting_in_nothing() {
        let mut s = base();
        let mut c = Condition::new("users", "id", ValueKind::Number);
        c.op = CompareOp::In;
        c.value = "  ,  , ".into();
        s.filters.conditions.push(c);
        assert!(!sql(&s).contains("IN ("));
    }

    #[test]
    fn a_between_missing_its_upper_bound_is_left_out() {
        let mut s = base();
        let mut c = Condition::new("users", "id", ValueKind::Number);
        c.op = CompareOp::Between;
        c.value = "1".into();
        s.filters.conditions.push(c);
        assert!(!sql(&s).contains("BETWEEN"));
    }

    #[test]
    fn empty_group_by_and_order_by_emit_no_clause() {
        let mut s = base();
        s.group_by.push(GroupByClause {
            table_alias: "users".into(),
            column: String::new(),
        });
        s.order_by.push(OrderByClause {
            table_alias: "users".into(),
            column: String::new(),
            direction: SortDir::Asc,
        });
        let out = sql(&s);
        assert!(!out.contains("GROUP BY"), "{}", out);
        assert!(!out.contains("ORDER BY"), "{}", out);
    }

    // -- quoting and escaping ----------------------------------------------

    #[test]
    fn awkward_identifiers_are_quoted_everywhere_they_appear() {
        let table = QueryTable::new("order details", "my schema", "order details")
            .with_columns(vec![("select".into(), "text".into())]);
        let mut s = BuilderState {
            tables: vec![table],
            ..Default::default()
        };
        let mut col = SelectedColumn::new("order details", "select");
        col.alias = "group by".into();
        s.columns.push(col);
        s.order_by.push(OrderByClause {
            table_alias: "order details".into(),
            column: "select".into(),
            direction: SortDir::Desc,
        });
        assert_eq!(
            sql(&s),
            "SELECT \"order details\".\"select\" AS \"group by\"\n\
             FROM \"my schema\".\"order details\" AS \"order details\"\n\
             ORDER BY \"order details\".\"select\" DESC;"
        );
    }

    #[test]
    fn an_identifier_containing_the_quote_character_is_escaped_not_terminated() {
        let table = QueryTable::new("we\"ird", "s", "we\"ird")
            .with_columns(vec![("c\"ol".into(), "text".into())]);
        let mut s = BuilderState {
            tables: vec![table],
            ..Default::default()
        };
        s.columns.push(SelectedColumn::new("we\"ird", "c\"ol"));
        let out = sql(&s);
        assert!(out.contains("\"we\"\"ird\".\"c\"\"ol\""), "{}", out);
    }

    #[test]
    fn the_drivers_own_quoting_is_used_not_a_hardcoded_one() {
        let out = generate_sql(&q_mysql, &esc, rules_for(&DbDriver::MySql), &base()).unwrap();
        assert_eq!(out, "SELECT *\nFROM `public`.`users` AS `users`;");
    }

    #[test]
    fn a_value_containing_a_quote_is_escaped_and_cannot_end_the_literal() {
        let mut s = base();
        let mut c = Condition::new("users", "name", ValueKind::Text);
        c.value = "O'Brien".into();
        s.filters.conditions.push(c);
        let out = sql(&s);
        assert!(out.contains("= 'O''Brien'"), "{}", out);
    }

    #[test]
    fn an_attempted_injection_stays_inside_the_literal() {
        let mut s = base();
        let mut c = Condition::new("users", "name", ValueKind::Text);
        c.value = "x'; DROP TABLE users; --".into();
        s.filters.conditions.push(c);
        let out = sql(&s);
        assert!(out.contains("'x''; DROP TABLE users; --'"), "{}", out);
        // Outside the string literals there is one statement, terminated
        // once, and no trace of the injected DDL.
        let code = strip_literals(&out);
        assert_eq!(code.matches(';').count(), 1, "{}", code);
        assert!(code.ends_with(';'), "{}", code);
        assert!(!code.contains("DROP"), "{}", code);
    }

    /// Everything outside single-quoted literals, with `''` handled as an
    /// escaped quote rather than an empty string.
    fn strip_literals(sql: &str) -> String {
        let mut out = String::new();
        let mut chars = sql.chars().peekable();
        let mut inside = false;
        while let Some(c) = chars.next() {
            match (inside, c) {
                (false, '\'') => inside = true,
                (false, _) => out.push(c),
                (true, '\'') => {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                    } else {
                        inside = false;
                    }
                }
                (true, _) => {}
            }
        }
        out
    }

    #[test]
    fn escaping_goes_through_the_driver_so_mysql_backslashes_are_handled() {
        let mut s = base();
        let mut c = Condition::new("users", "name", ValueKind::Text);
        c.value = "back\\slash".into();
        s.filters.conditions.push(c);
        let out = generate_sql(&q_mysql, &esc_mysql, rules_for(&DbDriver::MySql), &s).unwrap();
        assert!(out.contains("'back\\\\slash'"), "{}", out);
    }

    #[test]
    fn a_schemaless_driver_gets_a_bare_table_name() {
        let mut s = base();
        s.tables[0].schema = String::new();
        assert_eq!(sql(&s), "SELECT *\nFROM \"users\" AS \"users\";");
    }

    // -- values -------------------------------------------------------------

    #[test]
    fn numbers_are_written_bare_on_numeric_columns() {
        assert_eq!(format_value(&esc, "42", ValueKind::Number), "42");
        assert_eq!(format_value(&esc, "-3.5", ValueKind::Number), "-3.5");
    }

    #[test]
    fn text_that_only_looks_numeric_is_still_quoted() {
        // A number in a text column, and a non-number in a numeric column.
        assert_eq!(format_value(&esc, "42", ValueKind::Text), "'42'");
        assert_eq!(
            format_value(&esc, "1; DROP TABLE t", ValueKind::Number),
            "'1; DROP TABLE t'"
        );
        assert_eq!(format_value(&esc, "1e5", ValueKind::Number), "'1e5'");
        assert_eq!(format_value(&esc, "+-", ValueKind::Number), "'+-'");
    }

    #[test]
    fn booleans_are_written_bare() {
        assert_eq!(format_value(&esc, "true", ValueKind::Bool), "TRUE");
        assert_eq!(format_value(&esc, "no", ValueKind::Bool), "FALSE");
    }

    #[test]
    fn like_patterns_are_quoted_even_against_a_numeric_column() {
        let mut s = base();
        let mut c = Condition::new("users", "id", ValueKind::Number);
        c.op = CompareOp::Like;
        c.value = "1%".into();
        s.filters.conditions.push(c);
        assert!(sql(&s).contains("LIKE '1%'"));
    }

    #[test]
    fn value_kinds_are_read_off_the_declared_type() {
        assert_eq!(ValueKind::from_data_type("BOOLEAN"), ValueKind::Bool);
        assert_eq!(ValueKind::from_data_type("bigint"), ValueKind::Number);
        assert_eq!(ValueKind::from_data_type("numeric(10,2)"), ValueKind::Number);
        assert_eq!(ValueKind::from_data_type("varchar(255)"), ValueKind::Text);
        assert_eq!(ValueKind::from_data_type("timestamptz"), ValueKind::Text);
        assert_eq!(ValueKind::from_data_type("interval"), ValueKind::Text);
    }

    // -- SELECT -------------------------------------------------------------

    #[test]
    fn columns_are_qualified_by_alias() {
        let mut s = base();
        s.columns.push(SelectedColumn::new("users", "id"));
        s.columns.push(SelectedColumn::new("users", "name"));
        assert!(sql(&s).starts_with("SELECT \"users\".\"id\", \"users\".\"name\"\n"));
    }

    #[test]
    fn aggregates_wrap_the_column() {
        let mut s = base();
        let mut c = SelectedColumn::new("users", "id");
        c.aggregate = Aggregate::Sum;
        c.alias = "total".into();
        s.columns.push(c);
        assert!(sql(&s).starts_with("SELECT SUM(\"users\".\"id\") AS \"total\"\n"));
    }

    #[test]
    fn count_distinct_has_its_own_spelling() {
        let mut s = base();
        let mut c = SelectedColumn::new("users", "name");
        c.aggregate = Aggregate::CountDistinct;
        s.columns.push(c);
        assert!(sql(&s).contains("COUNT(DISTINCT \"users\".\"name\")"));
    }

    #[test]
    fn count_star_is_written_without_a_column() {
        let mut s = base();
        let mut c = SelectedColumn::new("users", STAR);
        c.aggregate = Aggregate::Count;
        s.columns.push(c);
        assert!(sql(&s).starts_with("SELECT COUNT(*)\n"));
    }

    #[test]
    fn a_bare_star_expands_to_the_tables_own_columns() {
        let mut s = base();
        s.columns.push(SelectedColumn::new("users", STAR));
        assert!(sql(&s).starts_with("SELECT \"users\".*\n"));
    }

    #[test]
    fn an_aggregate_that_cannot_take_star_is_dropped_and_reported() {
        let mut s = base();
        let mut c = SelectedColumn::new("users", STAR);
        c.aggregate = Aggregate::Sum;
        s.columns.push(c);
        // Nothing usable is left, so it falls back to SELECT *.
        assert!(sql(&s).starts_with("SELECT *\n"));
        assert!(validate(&s, pg()).iter().any(|w| w.contains("only COUNT")));
    }

    #[test]
    fn statement_level_distinct_precedes_the_column_list() {
        let mut s = base();
        s.distinct = true;
        s.columns.push(SelectedColumn::new("users", "name"));
        assert!(sql(&s).starts_with("SELECT DISTINCT \"users\".\"name\"\n"));
    }

    #[test]
    fn per_column_distinct_applies_to_the_leading_column_only() {
        let mut s = base();
        let mut first = SelectedColumn::new("users", "name");
        first.distinct = true;
        let mut second = SelectedColumn::new("users", "id");
        second.distinct = true;
        s.columns.push(first);
        s.columns.push(second);
        let out = sql(&s);
        // Exactly one DISTINCT, and it leads the list: `a, DISTINCT b` is not
        // valid SQL in any dialect.
        assert_eq!(out.matches("DISTINCT").count(), 1);
        assert!(out.starts_with("SELECT DISTINCT \"users\".\"name\", \"users\".\"id\"\n"));
        assert!(validate(&s, pg())
            .iter()
            .any(|w| w.contains("first output column")));
    }

    #[test]
    fn statement_distinct_suppresses_the_per_column_one() {
        let mut s = base();
        s.distinct = true;
        let mut c = SelectedColumn::new("users", "name");
        c.distinct = true;
        s.columns.push(c);
        assert_eq!(sql(&s).matches("DISTINCT").count(), 1);
    }

    // -- JOIN ---------------------------------------------------------------

    #[test]
    fn an_inner_join_names_the_table_its_alias_and_the_on_condition() {
        assert_eq!(
            sql(&joined()),
            "SELECT *\n\
             FROM \"public\".\"users\" AS \"users\"\n\
             INNER JOIN \"public\".\"orders\" AS \"orders\" \
             ON \"users\".\"id\" = \"orders\".\"user_id\";"
        );
    }

    #[test]
    fn every_join_type_is_spelled_correctly() {
        for (kind, expected) in [
            (JoinType::Inner, "INNER JOIN"),
            (JoinType::Left, "LEFT JOIN"),
            (JoinType::Right, "RIGHT JOIN"),
            (JoinType::FullOuter, "FULL OUTER JOIN"),
        ] {
            let mut s = joined();
            s.joins[0].join_type = kind;
            assert!(sql(&s).contains(expected), "{:?}", kind);
        }
    }

    #[test]
    fn a_cross_join_omits_the_on_clause_and_warns() {
        let mut s = joined();
        s.joins[0].join_type = JoinType::Cross;
        s.joins[0].left_column.clear();
        s.joins[0].right_column.clear();
        let out = sql(&s);
        assert!(out.contains("CROSS JOIN \"public\".\"orders\" AS \"orders\""), "{}", out);
        assert!(!out.contains(" ON "), "{}", out);
        assert!(validate(&s, pg())
            .iter()
            .any(|w| w.contains("every combination")));
    }

    #[test]
    fn a_self_join_uses_the_second_alias_throughout() {
        let mut s = base();
        let alias = s.free_alias("users");
        assert_eq!(alias, "users_2");
        s.tables
            .push(QueryTable::new(alias, "public", "users").with_columns(vec![
                ("id".into(), "integer".into()),
                ("manager_id".into(), "integer".into()),
            ]));
        s.joins.push(Join {
            join_type: JoinType::Left,
            left_table: "users".into(),
            left_column: "id".into(),
            right_table: "users_2".into(),
            right_column: "manager_id".into(),
        });
        let out = sql(&s);
        assert!(
            out.contains("LEFT JOIN \"public\".\"users\" AS \"users_2\" ON \"users\".\"id\" = \"users_2\".\"manager_id\""),
            "{}",
            out
        );
    }

    #[test]
    fn joins_written_out_of_order_still_resolve() {
        // b is joined to c before c is joined to a; both must still appear.
        let mut s = base();
        s.tables.push(orders());
        s.tables.push(
            QueryTable::new("items", "public", "items")
                .with_columns(vec![("order_id".into(), "integer".into())]),
        );
        s.joins.push(Join {
            join_type: JoinType::Inner,
            left_table: "orders".into(),
            left_column: "id".into(),
            right_table: "items".into(),
            right_column: "order_id".into(),
        });
        s.joins.push(Join {
            join_type: JoinType::Inner,
            left_table: "users".into(),
            left_column: "id".into(),
            right_table: "orders".into(),
            right_column: "user_id".into(),
        });
        let out = sql(&s);
        let orders_at = out.find("AS \"orders\"").expect("orders joined");
        let items_at = out.find("AS \"items\"").expect("items joined");
        // orders has to be in the statement before items references it.
        assert!(orders_at < items_at, "{}", out);
    }

    #[test]
    fn dialect_gaps_in_join_support_are_reported() {
        let mut s = joined();
        s.joins[0].join_type = JoinType::FullOuter;
        assert!(validate(&s, rules_for(&DbDriver::MySql))
            .iter()
            .any(|w| w.contains("FULL OUTER")));
        s.joins[0].join_type = JoinType::Right;
        assert!(validate(&s, rules_for(&DbDriver::Sqlite))
            .iter()
            .any(|w| w.contains("RIGHT JOIN")));
    }

    // -- WHERE --------------------------------------------------------------

    #[test]
    fn each_operator_produces_its_own_shape() {
        let cases: Vec<(CompareOp, &str, &str, &str)> = vec![
            (CompareOp::Eq, "5", "", "\"users\".\"id\" = 5"),
            (CompareOp::Ne, "5", "", "\"users\".\"id\" != 5"),
            (CompareOp::Gt, "5", "", "\"users\".\"id\" > 5"),
            (CompareOp::Lt, "5", "", "\"users\".\"id\" < 5"),
            (CompareOp::Gte, "5", "", "\"users\".\"id\" >= 5"),
            (CompareOp::Lte, "5", "", "\"users\".\"id\" <= 5"),
            (CompareOp::IsNull, "", "", "\"users\".\"id\" IS NULL"),
            (CompareOp::IsNotNull, "", "", "\"users\".\"id\" IS NOT NULL"),
            (CompareOp::In, "1, 2,3", "", "\"users\".\"id\" IN (1, 2, 3)"),
            (
                CompareOp::NotIn,
                "1,2",
                "",
                "\"users\".\"id\" NOT IN (1, 2)",
            ),
            (
                CompareOp::Between,
                "1",
                "9",
                "\"users\".\"id\" BETWEEN 1 AND 9",
            ),
        ];
        for (op, value, value2, expected) in cases {
            let mut c = Condition::new("users", "id", ValueKind::Number);
            c.op = op;
            c.value = value.into();
            c.value2 = value2.into();
            assert_eq!(condition_sql(&q, &esc, &c), expected, "{:?}", op);
        }
    }

    #[test]
    fn conditions_are_joined_by_the_groups_operator() {
        let mut s = base();
        let mut a = Condition::new("users", "id", ValueKind::Number);
        a.value = "1".into();
        let mut b = Condition::new("users", "name", ValueKind::Text);
        b.value = "ann".into();
        s.filters.conditions.push(a);
        s.filters.conditions.push(b);
        assert!(sql(&s).contains("WHERE \"users\".\"id\" = 1 AND \"users\".\"name\" = 'ann'"));

        s.filters.operator = Logical::Or;
        assert!(sql(&s).contains("\"users\".\"id\" = 1 OR \"users\".\"name\" = 'ann'"));
    }

    #[test]
    fn nested_groups_are_parenthesised_so_and_or_cannot_rebind() {
        let mut s = base();
        let mut outer = Condition::new("users", "active", ValueKind::Bool);
        outer.value = "true".into();
        s.filters.conditions.push(outer);

        let mut inner = ConditionGroup {
            operator: Logical::Or,
            ..Default::default()
        };
        let mut x = Condition::new("users", "id", ValueKind::Number);
        x.value = "1".into();
        let mut y = Condition::new("users", "id", ValueKind::Number);
        y.value = "2".into();
        inner.conditions.push(x);
        inner.conditions.push(y);
        s.filters.groups.push(inner);

        assert!(sql(&s).contains(
            "WHERE \"users\".\"active\" = TRUE AND (\"users\".\"id\" = 1 OR \"users\".\"id\" = 2)"
        ));
    }

    #[test]
    fn nesting_three_levels_deep_parenthesises_each_level() {
        let mut s = base();
        let mut level3 = ConditionGroup::default();
        let mut c = Condition::new("users", "id", ValueKind::Number);
        c.value = "3".into();
        level3.conditions.push(c);

        let mut level2 = ConditionGroup {
            operator: Logical::Or,
            ..Default::default()
        };
        let mut c2 = Condition::new("users", "id", ValueKind::Number);
        c2.value = "2".into();
        level2.conditions.push(c2);
        level2.groups.push(level3);

        s.filters.groups.push(level2);
        let out = sql(&s);
        assert!(
            out.contains("WHERE (\"users\".\"id\" = 2 OR (\"users\".\"id\" = 3))"),
            "{}",
            out
        );
    }

    #[test]
    fn an_empty_nested_group_contributes_nothing() {
        let mut s = base();
        let mut c = Condition::new("users", "id", ValueKind::Number);
        c.value = "1".into();
        s.filters.conditions.push(c);
        s.filters.groups.push(ConditionGroup::default());
        let out = sql(&s);
        assert!(out.contains("WHERE \"users\".\"id\" = 1;"), "{}", out);
        assert!(!out.contains("()"), "{}", out);
    }

    // -- GROUP BY, HAVING, ORDER BY -----------------------------------------

    #[test]
    fn group_by_and_having_are_emitted_in_order() {
        let mut s = base();
        let mut c = SelectedColumn::new("users", "name");
        c.alias = "name".into();
        s.columns.push(c);
        let mut agg = SelectedColumn::new("users", "id");
        agg.aggregate = Aggregate::Count;
        s.columns.push(agg);
        s.group_by.push(GroupByClause {
            table_alias: "users".into(),
            column: "name".into(),
        });
        let mut h = HavingCondition::new("users", "id");
        h.value = "2".into();
        s.having.push(h);

        assert_eq!(
            sql(&s),
            "SELECT \"users\".\"name\" AS \"name\", COUNT(\"users\".\"id\")\n\
             FROM \"public\".\"users\" AS \"users\"\n\
             GROUP BY \"users\".\"name\"\n\
             HAVING COUNT(\"users\".\"id\") > 2;"
        );
    }

    #[test]
    fn having_on_count_star_needs_no_column() {
        let mut s = base();
        s.group_by.push(GroupByClause {
            table_alias: "users".into(),
            column: "name".into(),
        });
        let mut h = HavingCondition::new("users", STAR);
        h.value = "1".into();
        s.having.push(h);
        assert!(sql(&s).contains("HAVING COUNT(*) > 1"));
    }

    #[test]
    fn an_ungrouped_unaggregated_column_is_reported() {
        let mut s = base();
        s.columns.push(SelectedColumn::new("users", "name"));
        s.group_by.push(GroupByClause {
            table_alias: "users".into(),
            column: "id".into(),
        });
        assert!(validate(&s, pg())
            .iter()
            .any(|w| w.contains("users.name") && w.contains("GROUP BY")));
    }

    #[test]
    fn order_by_keeps_insertion_order_and_per_clause_direction() {
        let mut s = base();
        s.order_by.push(OrderByClause {
            table_alias: "users".into(),
            column: "name".into(),
            direction: SortDir::Asc,
        });
        s.order_by.push(OrderByClause {
            table_alias: "users".into(),
            column: "id".into(),
            direction: SortDir::Desc,
        });
        assert!(sql(&s).contains("ORDER BY \"users\".\"name\" ASC, \"users\".\"id\" DESC"));
    }

    // -- LIMIT / OFFSET -----------------------------------------------------

    #[test]
    fn limit_and_offset_use_the_standard_spelling() {
        let r = pg();
        assert_eq!(limit_offset_clause(r, None, None), "");
        assert_eq!(limit_offset_clause(r, Some(10), None), "LIMIT 10");
        assert_eq!(limit_offset_clause(r, None, Some(20)), "OFFSET 20");
        assert_eq!(
            limit_offset_clause(r, Some(10), Some(20)),
            "LIMIT 10 OFFSET 20"
        );
    }

    #[test]
    fn a_zero_offset_is_not_written() {
        assert_eq!(limit_offset_clause(pg(), Some(5), Some(0)), "LIMIT 5");
        assert_eq!(limit_offset_clause(pg(), None, Some(0)), "");
    }

    #[test]
    fn mysql_and_sqlite_pair_a_bare_offset_with_a_limit() {
        // Both reject `OFFSET n` on its own; each has its own idiom for
        // "no upper bound".
        assert_eq!(
            limit_offset_clause(rules_for(&DbDriver::MySql), None, Some(20)),
            "LIMIT 18446744073709551615 OFFSET 20"
        );
        assert_eq!(
            limit_offset_clause(rules_for(&DbDriver::Sqlite), None, Some(20)),
            "LIMIT -1 OFFSET 20"
        );
        assert_eq!(
            limit_offset_clause(rules_for(&DbDriver::MySql), Some(5), Some(20)),
            "LIMIT 5 OFFSET 20"
        );
    }

    #[test]
    fn sql_server_uses_offset_fetch() {
        let r = rules_for(&DbDriver::SqlServer);
        assert_eq!(
            limit_offset_clause(r, Some(10), Some(20)),
            "OFFSET 20 ROWS FETCH NEXT 10 ROWS ONLY"
        );
        assert_eq!(
            limit_offset_clause(r, Some(10), None),
            "OFFSET 0 ROWS FETCH NEXT 10 ROWS ONLY"
        );
        assert_eq!(limit_offset_clause(r, None, Some(20)), "OFFSET 20 ROWS");
    }

    #[test]
    fn sql_server_offset_without_order_by_is_reported() {
        let mut s = base();
        s.limit = Some(10);
        assert!(validate(&s, rules_for(&DbDriver::SqlServer))
            .iter()
            .any(|w| w.contains("ORDER BY")));
    }

    #[test]
    fn pagination_is_described_in_plain_english() {
        assert_eq!(pagination_summary(None, None), "Every matching row");
        assert_eq!(pagination_summary(Some(25), None), "The first 25 row(s)");
        assert_eq!(
            pagination_summary(None, Some(50)),
            "Every row after the first 50"
        );
        // Page 3 of 25: rows 51 to 75.
        assert_eq!(pagination_summary(Some(25), Some(50)), "Rows 51 to 75");
        assert_eq!(pagination_summary(Some(10), Some(0)), "The first 10 row(s)");
    }

    #[test]
    fn pagination_arithmetic_does_not_overflow() {
        assert_eq!(
            pagination_summary(Some(u64::MAX), Some(u64::MAX)),
            format!("Rows {} to {}", u64::MAX, u64::MAX)
        );
    }

    // -- clause ordering and combination ------------------------------------

    #[test]
    fn every_clause_together_comes_out_in_sql_order() {
        let mut s = joined();
        s.distinct = true;
        let mut name = SelectedColumn::new("users", "name");
        name.alias = "customer".into();
        s.columns.push(name);
        let mut total = SelectedColumn::new("orders", "total");
        total.aggregate = Aggregate::Sum;
        total.alias = "spent".into();
        s.columns.push(total);

        let mut active = Condition::new("users", "active", ValueKind::Bool);
        active.value = "true".into();
        s.filters.conditions.push(active);

        s.group_by.push(GroupByClause {
            table_alias: "users".into(),
            column: "name".into(),
        });
        let mut h = HavingCondition::new("orders", "total");
        h.aggregate = Aggregate::Sum;
        h.op = CompareOp::Gt;
        h.kind = ValueKind::Number;
        h.value = "100".into();
        s.having.push(h);
        s.order_by.push(OrderByClause {
            table_alias: "users".into(),
            column: "name".into(),
            direction: SortDir::Asc,
        });
        s.limit = Some(10);
        s.offset = Some(20);

        assert_eq!(
            sql(&s),
            "SELECT DISTINCT \"users\".\"name\" AS \"customer\", SUM(\"orders\".\"total\") AS \"spent\"\n\
             FROM \"public\".\"users\" AS \"users\"\n\
             INNER JOIN \"public\".\"orders\" AS \"orders\" ON \"users\".\"id\" = \"orders\".\"user_id\"\n\
             WHERE \"users\".\"active\" = TRUE\n\
             GROUP BY \"users\".\"name\"\n\
             HAVING SUM(\"orders\".\"total\") > 100\n\
             ORDER BY \"users\".\"name\" ASC\n\
             LIMIT 10 OFFSET 20;"
        );
    }

    #[test]
    fn the_statement_is_terminated_once() {
        let s = joined();
        let out = sql(&s);
        assert!(out.ends_with(';'));
        assert_eq!(out.matches(';').count(), 1);
    }

    // -- state bookkeeping --------------------------------------------------

    #[test]
    fn removing_a_table_cascades_to_everything_that_used_it() {
        let mut s = joined();
        s.columns.push(SelectedColumn::new("orders", "total"));
        s.group_by.push(GroupByClause {
            table_alias: "orders".into(),
            column: "total".into(),
        });
        s.order_by.push(OrderByClause {
            table_alias: "orders".into(),
            column: "total".into(),
            direction: SortDir::Asc,
        });
        s.having.push(HavingCondition::new("orders", "total"));
        let mut c = Condition::new("orders", "total", ValueKind::Number);
        c.value = "1".into();
        let mut nested = ConditionGroup::default();
        nested.conditions.push(c.clone());
        s.filters.conditions.push(c);
        s.filters.groups.push(nested);

        s.remove_table("orders");

        assert_eq!(s.tables.len(), 1);
        assert!(s.columns.is_empty());
        assert!(s.joins.is_empty());
        assert!(s.group_by.is_empty());
        assert!(s.order_by.is_empty());
        assert!(s.having.is_empty());
        assert!(s.filters.conditions.is_empty());
        // The nested group emptied out and went with it.
        assert!(s.filters.groups.is_empty());
    }

    #[test]
    fn aliases_are_unique_per_table_name() {
        let mut s = base();
        assert_eq!(s.free_alias("orders"), "orders");
        assert_eq!(s.free_alias("users"), "users_2");
        s.tables
            .push(QueryTable::new("users_2", "public", "users"));
        assert_eq!(s.free_alias("users"), "users_3");
    }

    #[test]
    fn duplicate_output_names_are_reported() {
        let mut s = base();
        let mut a = SelectedColumn::new("users", "id");
        a.alias = "x".into();
        let mut b = SelectedColumn::new("users", "name");
        b.alias = "x".into();
        s.columns.push(a);
        s.columns.push(b);
        assert!(validate(&s, pg()).iter().any(|w| w.contains("both named x")));
    }

    #[test]
    fn having_without_group_by_is_reported() {
        let mut s = base();
        let mut h = HavingCondition::new("users", "id");
        h.value = "1".into();
        s.having.push(h);
        assert!(validate(&s, pg())
            .iter()
            .any(|w| w.contains("HAVING without GROUP BY")));
    }

    #[test]
    fn an_empty_builder_asks_for_a_table_and_says_nothing_else() {
        assert_eq!(
            validate(&BuilderState::default(), pg()),
            vec!["Add a table to start building a query.".to_string()]
        );
    }

    #[test]
    fn a_clean_query_reports_nothing() {
        assert!(validate(&joined(), pg()).is_empty());
    }

    #[test]
    fn foreign_keys_suggest_joins_between_chosen_tables_only() {
        let mut s = base();
        s.tables.push(orders());
        let fk = ForeignKeyInfo {
            name: "orders_user_id_fkey".into(),
            schema: "public".into(),
            table: "orders".into(),
            columns: vec!["user_id".into()],
            referenced_schema: "public".into(),
            referenced_table: "users".into(),
            referenced_columns: vec!["id".into()],
            on_delete: None,
            on_update: None,
        };
        let suggestions = join_suggestions(&s, std::slice::from_ref(&fk));
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].left_table, "orders");
        assert_eq!(suggestions[0].left_column, "user_id");
        assert_eq!(suggestions[0].right_table, "users");
        assert_eq!(suggestions[0].right_column, "id");

        // Once the join exists it stops being suggested.
        s.joins.push(suggestions[0].clone());
        assert!(join_suggestions(&s, std::slice::from_ref(&fk)).is_empty());

        // A key touching a table that is not in the query suggests nothing.
        s.remove_table("orders");
        assert!(join_suggestions(&s, std::slice::from_ref(&fk)).is_empty());
    }

    #[test]
    fn mongo_and_redis_are_not_offered_the_builder() {
        assert!(!supports_builder(&DbDriver::MongoDb));
        assert!(!supports_builder(&DbDriver::Redis));
        assert!(supports_builder(&DbDriver::Postgres));
    }

    #[test]
    fn the_completion_index_covers_tables_aliases_and_columns() {
        let index = schema_index(&joined());
        assert!(index.tables.contains("users"));
        assert!(index.tables.contains("orders"));
        assert!(index.columns.contains("user_id"));
    }
}
