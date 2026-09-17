//! Visual query builder model: compose a SELECT without writing SQL.
//!
//! Lifted from the egui `query_builder` screen so any shell can reuse it.
//! Parity target: the React `QueryBuilder.tsx` route and the pure generator
//! in `src/lib/sqlGenerator.ts` (see 2.8 in
//! docs/rust-ui-migration/01-frontend-inventory.md).
//!
//! * **Everything that reaches SQL is quoted or escaped by the driver.**
//!   Identifiers go through `quote_identifier`, values through
//!   `escape_string_literal`. The only text that ever lands unquoted is a
//!   number this module has itself proved to be numeric.
//! * **Partial state produces no SQL, never broken SQL.** A half-typed
//!   condition, a join missing its columns, a table nothing joins to: each is
//!   dropped from generation and reported as a warning.
//!
//! `generate_sql` and its helpers are pure functions over `BuilderState`, and
//! that is where the tests live.

use dbhive_core::models::{DbDriver, ForeignKeyInfo};

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

// ---------------------------------------------------------------------------
// Screen model
// ---------------------------------------------------------------------------

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

/// Joins the schema's foreign keys imply between tables already chosen, minus
/// the ones already written.
pub fn join_suggestions(state: &BuilderState, keys: &[ForeignKeyInfo]) -> Vec<Join> {
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

pub fn columns_for<'a>(columns: &'a [(String, Vec<String>)], alias: &str) -> &'a [String] {
    columns
        .iter()
        .find(|(a, _)| a == alias)
        .map(|(_, c)| c.as_slice())
        .unwrap_or(&[])
}

pub fn count_conditions(group: &ConditionGroup) -> usize {
    group.conditions.len() + group.groups.iter().map(count_conditions).sum::<usize>()
}

pub fn first_column(state: &BuilderState) -> (String, String) {
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
}
