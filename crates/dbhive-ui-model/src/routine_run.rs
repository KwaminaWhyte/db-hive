//! Running a stored procedure or function from the schema browser: one input
//! per argument, read from the routine's signature, and the statement that
//! calls it.
//!
//! The statement is built here from the live driver's `quote_identifier` and
//! `escape_string_literal` (passed in as closures), never by pasting what was
//! typed into SQL. Each box is read according to its parameter's type: a
//! number must be a plain number and goes in bare, a boolean must be a
//! boolean word, and anything else is a quoted, escaped string literal that
//! the server casts. So typing `1); DROP TABLE x; --` into a text argument
//! yields one harmless string, and into a number argument an error under the
//! box.
//!
//! Per dialect:
//!
//! * PostgreSQL: a function is `SELECT f(...)`, or `SELECT * FROM f(...)` when
//!   it returns rows (`TABLE(...)`, `SETOF`, `record`); a procedure is
//!   `CALL p(...)`. A function's `OUT` parameters are not passed; a
//!   procedure's are passed as `NULL` (PostgreSQL 14 and later want them).
//!   Trailing arguments that have a default and were left empty are left out,
//!   so the server applies the default. `VARIADIC` takes an array literal.
//! * MySQL: `SELECT f(...)` or `CALL p(...)`. An `OUT` or `INOUT` parameter of
//!   a procedure must be a variable, so it gets a session variable the app
//!   names (`@dbhive_out_1`); its value is not read back.
//! * SQL Server: `EXEC [s].[p] a, b`, `SELECT [s].[f](...)` for a scalar
//!   function and `SELECT * FROM [s].[f](...)` for a table-valued one. Text
//!   goes in as `N'...'` so it survives a non-Unicode collation, booleans
//!   (`bit`) as 1 or 0.
//! * SQLite, Turso, MongoDB and Redis have no stored routines, so nothing is
//!   built for them.

use dbhive_core::models::DbDriver;
use dbhive_core::service_procedures::{qualify, supports_routines, ProcedureInfo, RoutineKind};

use crate::procedures::parse_signature;

/// How a parameter passes its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamMode {
    In,
    Out,
    InOut,
    Variadic,
}

/// How a box is read, from the parameter's type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueClass {
    Number,
    Bool,
    /// Everything else: sent as a string literal and cast by the server.
    Text,
}

/// One parameter of a routine, read from its signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutineParam {
    /// The signature's own text for it, without a default clause.
    pub label: String,
    /// May be empty: PostgreSQL allows unnamed parameters.
    pub name: String,
    pub type_name: String,
    pub mode: ParamMode,
    pub has_default: bool,
}

/// What goes in one position of the call.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Slot {
    /// Not passed at all (a PostgreSQL function's OUT parameter).
    Skip,
    /// The app's own SQL text: `NULL` or a session variable name.
    Fixed(String),
    /// The box at this index.
    Input(usize),
}

fn split_default(item: &str) -> (&str, bool) {
    let lower = item.to_ascii_lowercase();
    if let Some(at) = lower.find(" default ") {
        return (item[..at].trim_end(), true);
    }
    // `a integer = 1` is the other spelling PostgreSQL accepts.
    if let Some(at) = item.find(" = ") {
        return (item[..at].trim_end(), true);
    }
    (item, false)
}

/// Second words that mean the first word is part of a type, not a name:
/// `double precision`, `character varying`, `timestamp with time zone`.
fn continues_type(word: &str) -> bool {
    matches!(
        word.to_ascii_lowercase().as_str(),
        "precision" | "varying" | "with" | "without" | "zone" | "unsigned" | "signed"
    )
}

/// Read one parameter from its signature text.
pub fn parse_param(driver: &DbDriver, item: &str) -> RoutineParam {
    let (body, has_default) = split_default(item.trim());
    let mut words: Vec<&str> = body.split_whitespace().collect();
    let mut mode = ParamMode::In;

    if let Some(first) = words.first() {
        let m = match first.to_ascii_uppercase().as_str() {
            "IN" => Some(ParamMode::In),
            "OUT" => Some(ParamMode::Out),
            "INOUT" => Some(ParamMode::InOut),
            "VARIADIC" => Some(ParamMode::Variadic),
            _ => None,
        };
        if let Some(m) = m {
            if words.len() > 1 {
                mode = m;
                words.remove(0);
            }
        }
    }
    // T-SQL marks output parameters at the end: `@total int OUTPUT`. They are
    // read and written, so they still take a value.
    if matches!(driver, DbDriver::SqlServer) {
        if let Some(last) = words.last() {
            if matches!(last.to_ascii_uppercase().as_str(), "OUTPUT" | "OUT") && words.len() > 1 {
                mode = ParamMode::InOut;
                words.pop();
            }
        }
    }

    let (name, type_name) = match words.as_slice() {
        [] => (String::new(), String::new()),
        [only] => (String::new(), (*only).to_string()),
        [_, second, ..] if continues_type(second) => (String::new(), words.join(" ")),
        [first, rest @ ..] => ((*first).to_string(), rest.join(" ")),
    };

    let mut label = words.join(" ");
    match mode {
        ParamMode::In => {}
        ParamMode::Out => label = format!("OUT {label}"),
        ParamMode::InOut => label = format!("INOUT {label}"),
        ParamMode::Variadic => label = format!("VARIADIC {label}"),
    }

    RoutineParam {
        label,
        name,
        type_name,
        mode,
        has_default,
    }
}

/// Every parameter of `p`, in order.
pub fn params(driver: &DbDriver, p: &ProcedureInfo) -> Vec<RoutineParam> {
    parse_signature(&p.argument_signature)
        .iter()
        .map(|item| parse_param(driver, item))
        .collect()
}

/// How a box for `type_name` is read.
pub fn value_class(driver: &DbDriver, type_name: &str) -> ValueClass {
    let t = type_name.trim().to_ascii_lowercase();
    if t.ends_with("[]") || t.starts_with('_') {
        return ValueClass::Text;
    }
    let base: String = t
        .split(|c: char| c == '(' || c.is_whitespace())
        .next()
        .unwrap_or("")
        .to_string();
    match base.as_str() {
        "boolean" | "bool" => ValueClass::Bool,
        "bit" if matches!(driver, DbDriver::SqlServer) => ValueClass::Bool,
        "int" | "integer" | "smallint" | "bigint" | "tinyint" | "mediumint" | "int2" | "int4"
        | "int8" | "numeric" | "decimal" | "dec" | "real" | "float" | "float4" | "float8"
        | "double" | "smallserial" | "serial" | "bigserial" | "oid" | "money" | "smallmoney" => {
            ValueClass::Number
        }
        _ => ValueClass::Text,
    }
}

/// Whether the box takes a value at all for this routine on this driver.
pub fn takes_input(driver: &DbDriver, kind: RoutineKind, param: &RoutineParam) -> bool {
    match param.mode {
        ParamMode::In | ParamMode::Variadic => true,
        ParamMode::InOut => !(matches!(driver, DbDriver::MySql) && kind == RoutineKind::Procedure),
        ParamMode::Out => false,
    }
}

/// A plain decimal number: an optional sign, digits, an optional fraction and
/// an optional exponent. Nothing else reaches the SQL bare.
pub fn is_plain_number(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut digits = i - int_start;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        digits += i - frac_start;
    }
    if digits == 0 {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let exp_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp_start {
            return false;
        }
    }
    i == b.len()
}

fn bool_word(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "true" | "t" | "yes" | "y" | "on" | "1" => Some(true),
        "false" | "f" | "no" | "n" | "off" | "0" => Some(false),
        _ => None,
    }
}

/// Read a box: `Ok(None)` for NULL (an empty box or `null`), `Ok(Some(sql))`
/// for a literal, `Err(why)` for something its type cannot take.
///
/// A box holding a JSON string (`"null"`, `""`, `" padded "`) is taken as
/// exactly that string, which is how to pass text that would otherwise read
/// as NULL or be trimmed.
pub fn render_value(
    driver: &DbDriver,
    class: ValueClass,
    raw: &str,
    escape: &dyn Fn(&str) -> String,
) -> Result<Option<String>, String> {
    let t = raw.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("null") {
        return Ok(None);
    }
    let text = |s: &str| {
        if matches!(driver, DbDriver::SqlServer) {
            format!("N'{}'", escape(s))
        } else {
            format!("'{}'", escape(s))
        }
    };
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        if let Ok(serde_json::Value::String(s)) = serde_json::from_str::<serde_json::Value>(t) {
            return match class {
                ValueClass::Text => Ok(Some(text(&s))),
                // A quoted number is still a number: check it the same way.
                _ => render_value(driver, class, &s, escape).and_then(|v| match v {
                    Some(v) => Ok(Some(v)),
                    None => Err("Expects a value, not an empty string".to_string()),
                }),
            };
        }
    }
    match class {
        ValueClass::Number => {
            if is_plain_number(t) {
                Ok(Some(t.to_string()))
            } else {
                Err("Expects a number".to_string())
            }
        }
        ValueClass::Bool => match bool_word(t) {
            Some(b) if matches!(driver, DbDriver::SqlServer) => {
                Ok(Some(if b { "1" } else { "0" }.to_string()))
            }
            Some(b) => Ok(Some(if b { "TRUE" } else { "FALSE" }.to_string())),
            None => Err("Expects true or false".to_string()),
        },
        ValueClass::Text => Ok(Some(text(t))),
    }
}

/// Whether a PostgreSQL function returns rows, so it reads better as a table.
fn returns_rows(driver: &DbDriver, p: &ProcedureInfo) -> bool {
    let Some(r) = p.return_type.as_deref() else {
        return false;
    };
    let r = r.trim().to_ascii_lowercase();
    if matches!(driver, DbDriver::SqlServer) {
        return r == "table";
    }
    driver.is_postgres_compatible()
        && (r.starts_with("table") || r.starts_with("setof ") || r == "record")
}

/// The statement for `p` with the boxes `values` (one per parameter, in
/// order), or one message per parameter saying what is wrong (empty where
/// nothing is).
pub fn build_call(
    driver: &DbDriver,
    p: &ProcedureInfo,
    params: &[RoutineParam],
    values: &[String],
    quote: &dyn Fn(&str) -> String,
    escape: &dyn Fn(&str) -> String,
) -> Result<String, Vec<String>> {
    if !supports_routines(driver) {
        return Err(vec!["This database has no stored routines".to_string()]);
    }
    let value = |i: usize| values.get(i).map(String::as_str).unwrap_or("");

    let mut slots: Vec<Slot> = Vec::new();
    let mut out_vars = 0;
    for (i, param) in params.iter().enumerate() {
        let slot = if takes_input(driver, p.kind, param) {
            Slot::Input(i)
        } else if matches!(driver, DbDriver::MySql) {
            out_vars += 1;
            Slot::Fixed(format!("@dbhive_out_{out_vars}"))
        } else if driver.is_postgres_compatible() && p.kind == RoutineKind::Function {
            Slot::Skip
        } else {
            Slot::Fixed("NULL".to_string())
        };
        slots.push(slot);
    }
    // Trailing defaults left empty are left out, so the server applies them.
    while let Some(Slot::Input(i)) = slots.iter().rev().find(|s| **s != Slot::Skip) {
        let i = *i;
        if params[i].has_default && value(i).trim().is_empty() {
            let at = slots
                .iter()
                .rposition(|s| *s == Slot::Input(i))
                .unwrap_or(0);
            slots.truncate(at);
        } else {
            break;
        }
    }

    let mut errors = vec![String::new(); params.len()];
    let mut rendered = Vec::new();
    for slot in &slots {
        match slot {
            Slot::Skip => {}
            Slot::Fixed(sql) => rendered.push(sql.clone()),
            Slot::Input(i) => {
                let param = &params[*i];
                let class = if param.mode == ParamMode::Variadic {
                    ValueClass::Text
                } else {
                    value_class(driver, &param.type_name)
                };
                match render_value(driver, class, value(*i), escape) {
                    Ok(Some(v)) if param.mode == ParamMode::Variadic => {
                        rendered.push(format!("VARIADIC {v}"))
                    }
                    Ok(Some(v)) => rendered.push(v),
                    Ok(None) => rendered.push("NULL".to_string()),
                    Err(e) => errors[*i] = e,
                }
            }
        }
    }
    if errors.iter().any(|e| !e.is_empty()) {
        return Err(errors);
    }

    let target = qualify(quote, &p.schema, &p.name);
    let args = rendered.join(", ");
    Ok(match (driver, p.kind) {
        (DbDriver::SqlServer, RoutineKind::Procedure) if args.is_empty() => {
            format!("EXEC {target}")
        }
        (DbDriver::SqlServer, RoutineKind::Procedure) => format!("EXEC {target} {args}"),
        (_, RoutineKind::Procedure) => format!("CALL {target}({args})"),
        (_, RoutineKind::Function) if returns_rows(driver, p) => {
            format!("SELECT * FROM {target}({args})")
        }
        (_, RoutineKind::Function) => format!("SELECT {target}({args})"),
    })
}

/// A one-line hint under a box, from its type.
pub fn hint(driver: &DbDriver, param: &RoutineParam) -> String {
    let empty = if param.has_default {
        "empty for the default"
    } else {
        "empty for NULL"
    };
    if param.mode == ParamMode::Variadic {
        return format!("An array, e.g. {{1,2,3}}; {empty}");
    }
    match value_class(driver, &param.type_name) {
        ValueClass::Number => format!("A number; {empty}"),
        ValueClass::Bool => format!("true or false; {empty}"),
        ValueClass::Text => format!("Sent as text; {empty}"),
    }
}

/// Why a parameter has no box, shown in its place.
pub fn no_input_note(driver: &DbDriver, kind: RoutineKind) -> &'static str {
    match (driver, kind) {
        (DbDriver::MySql, _) => "Output: passed as a session variable, not shown",
        (_, RoutineKind::Function) => "Output: returned in the result",
        _ => "Output: passed as NULL, returned in the result",
    }
}

// ---------------------------------------------------------------------------
// The form
// ---------------------------------------------------------------------------

/// One row of the form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgField {
    pub label: String,
    pub hint: String,
    pub value: String,
    pub error: String,
    /// False for an output parameter, which shows a note instead of a box.
    pub input: bool,
}

/// What the run form draws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunView {
    pub fields: Vec<ArgField>,
    /// The statement, or empty while a box is wrong.
    pub sql: String,
    pub can_run: bool,
    pub confirming: bool,
    /// "Run function" or "Run procedure".
    pub run_label: String,
    pub confirm_text: String,
}

/// The run form for one routine: its boxes, and whether the user is being
/// asked to confirm.
#[derive(Debug, Clone)]
pub struct RunForm {
    pub driver: DbDriver,
    pub routine: ProcedureInfo,
    pub params: Vec<RoutineParam>,
    pub values: Vec<String>,
    pub confirming: bool,
}

impl RunForm {
    pub fn new(driver: &DbDriver, routine: &ProcedureInfo) -> Self {
        let params = params(driver, routine);
        Self {
            driver: driver.clone(),
            routine: routine.clone(),
            values: vec![String::new(); params.len()],
            params,
            confirming: false,
        }
    }

    /// A box changed. Any confirmation was for the old statement, so it is
    /// withdrawn.
    pub fn set_value(&mut self, index: usize, text: &str) {
        if let Some(v) = self.values.get_mut(index) {
            if v != text {
                *v = text.to_string();
                self.confirming = false;
            }
        }
    }

    pub fn statement(
        &self,
        quote: &dyn Fn(&str) -> String,
        escape: &dyn Fn(&str) -> String,
    ) -> Result<String, Vec<String>> {
        build_call(
            &self.driver,
            &self.routine,
            &self.params,
            &self.values,
            quote,
            escape,
        )
    }

    /// Ask before running. Refused while a box is wrong.
    pub fn ask(&mut self, quote: &dyn Fn(&str) -> String, escape: &dyn Fn(&str) -> String) -> bool {
        self.confirming = self.statement(quote, escape).is_ok();
        self.confirming
    }

    pub fn cancel(&mut self) {
        self.confirming = false;
    }

    /// The confirmed statement, once. `None` if nothing was confirmed or a
    /// box went wrong since.
    pub fn take_confirmed(
        &mut self,
        quote: &dyn Fn(&str) -> String,
        escape: &dyn Fn(&str) -> String,
    ) -> Option<String> {
        if !std::mem::replace(&mut self.confirming, false) {
            return None;
        }
        self.statement(quote, escape).ok()
    }

    pub fn view(
        &self,
        connection: &str,
        quote: &dyn Fn(&str) -> String,
        escape: &dyn Fn(&str) -> String,
    ) -> RunView {
        let built = self.statement(quote, escape);
        let errors = built.as_ref().err().cloned().unwrap_or_default();
        let fields = self
            .params
            .iter()
            .enumerate()
            .map(|(i, param)| {
                let input = takes_input(&self.driver, self.routine.kind, param);
                ArgField {
                    label: param.label.clone(),
                    hint: if input {
                        hint(&self.driver, param)
                    } else {
                        no_input_note(&self.driver, self.routine.kind).to_string()
                    },
                    value: self.values.get(i).cloned().unwrap_or_default(),
                    error: errors.get(i).cloned().unwrap_or_default(),
                    input,
                }
            })
            .collect();
        let what = self.routine.kind.label().to_lowercase();
        RunView {
            fields,
            can_run: built.is_ok(),
            sql: built.unwrap_or_default(),
            confirming: self.confirming,
            run_label: format!("Run {what}"),
            confirm_text: format!(
                "Run this {what} on {connection}? It runs as written, with your permissions, \
                 and may change data. The result opens in a new query tab."
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pg_q(s: &str) -> String {
        format!("\"{}\"", s.replace('"', "\"\""))
    }
    fn my_q(s: &str) -> String {
        format!("`{}`", s.replace('`', "``"))
    }
    fn ms_q(s: &str) -> String {
        format!("[{}]", s.replace(']', "]]"))
    }
    fn esc(s: &str) -> String {
        s.replace('\'', "''")
    }
    fn my_esc(s: &str) -> String {
        s.replace('\\', "\\\\").replace('\'', "''")
    }

    fn routine(
        kind: RoutineKind,
        schema: &str,
        name: &str,
        sig: &str,
        ret: Option<&str>,
    ) -> ProcedureInfo {
        ProcedureInfo {
            schema: schema.into(),
            name: name.into(),
            kind,
            language: None,
            return_type: ret.map(str::to_string),
            argument_signature: sig.into(),
        }
    }

    fn run(driver: &DbDriver, p: &ProcedureInfo, values: &[&str]) -> Result<String, Vec<String>> {
        let ps = params(driver, p);
        let values: Vec<String> = values.iter().map(|s| s.to_string()).collect();
        let (q, e): (&dyn Fn(&str) -> String, &dyn Fn(&str) -> String) = match driver {
            DbDriver::MySql => (&my_q, &my_esc),
            DbDriver::SqlServer => (&ms_q, &esc),
            _ => (&pg_q, &esc),
        };
        build_call(driver, p, &ps, &values, q, e)
    }

    // -- Reading signatures -------------------------------------------------------

    #[test]
    fn postgres_parameters_keep_name_type_mode_and_default() {
        let p = parse_param(&DbDriver::Postgres, "a integer");
        assert_eq!((p.name.as_str(), p.type_name.as_str()), ("a", "integer"));
        assert_eq!(p.mode, ParamMode::In);
        assert!(!p.has_default);

        let p = parse_param(&DbDriver::Postgres, "OUT total numeric");
        assert_eq!(p.mode, ParamMode::Out);
        assert_eq!(p.label, "OUT total numeric");

        let p = parse_param(&DbDriver::Postgres, "b text DEFAULT 'x'::text");
        assert!(p.has_default);
        assert_eq!(p.label, "b text");

        let p = parse_param(&DbDriver::Postgres, "double precision");
        assert_eq!(p.name, "");
        assert_eq!(p.type_name, "double precision");

        let p = parse_param(&DbDriver::Postgres, "at timestamp with time zone");
        assert_eq!(p.name, "at");
        assert_eq!(p.type_name, "timestamp with time zone");

        let p = parse_param(&DbDriver::Postgres, "VARIADIC nums integer[]");
        assert_eq!(p.mode, ParamMode::Variadic);
    }

    #[test]
    fn mysql_and_sqlserver_parameters_read_their_own_spelling() {
        let p = parse_param(&DbDriver::MySql, "INOUT counter int");
        assert_eq!(p.mode, ParamMode::InOut);
        assert_eq!(p.name, "counter");
        let p = parse_param(&DbDriver::MySql, "amount decimal(10,2)");
        assert_eq!(p.type_name, "decimal(10,2)");

        let p = parse_param(&DbDriver::SqlServer, "@total int OUTPUT");
        assert_eq!(p.mode, ParamMode::InOut);
        assert_eq!(p.name, "@total");
        assert_eq!(p.type_name, "int");
    }

    #[test]
    fn value_classes_follow_the_type() {
        assert_eq!(
            value_class(&DbDriver::Postgres, "integer"),
            ValueClass::Number
        );
        assert_eq!(
            value_class(&DbDriver::MySql, "decimal(10,2)"),
            ValueClass::Number
        );
        assert_eq!(
            value_class(&DbDriver::Postgres, "double precision"),
            ValueClass::Number
        );
        assert_eq!(
            value_class(&DbDriver::Postgres, "boolean"),
            ValueClass::Bool
        );
        assert_eq!(value_class(&DbDriver::SqlServer, "bit"), ValueClass::Bool);
        assert_eq!(value_class(&DbDriver::Postgres, "bit"), ValueClass::Text);
        assert_eq!(
            value_class(&DbDriver::Postgres, "integer[]"),
            ValueClass::Text
        );
        assert_eq!(value_class(&DbDriver::Postgres, "text"), ValueClass::Text);
        assert_eq!(value_class(&DbDriver::Postgres, "date"), ValueClass::Text);
    }

    #[test]
    fn only_plain_numbers_pass_as_numbers() {
        for ok in ["0", "-1", "+2", "3.5", ".5", "5.", "1e3", "-2.5E-4"] {
            assert!(is_plain_number(ok), "{ok}");
        }
        for bad in [
            "",
            "-",
            ".",
            "1e",
            "inf",
            "NaN",
            "0x10",
            "1 2",
            "1;",
            "1) OR (1=1",
            "1,000",
        ] {
            assert!(!is_plain_number(bad), "{bad}");
        }
    }

    // -- PostgreSQL ---------------------------------------------------------------

    #[test]
    fn postgres_function_with_arguments() {
        let p = routine(
            RoutineKind::Function,
            "public",
            "add_numbers",
            "(a integer, b integer)",
            Some("integer"),
        );
        assert_eq!(
            run(&DbDriver::Postgres, &p, &["2", "40"]).unwrap(),
            "SELECT \"public\".\"add_numbers\"(2, 40)"
        );
        // An empty box is NULL.
        assert_eq!(
            run(&DbDriver::Postgres, &p, &["2", ""]).unwrap(),
            "SELECT \"public\".\"add_numbers\"(2, NULL)"
        );
    }

    #[test]
    fn text_is_one_escaped_literal_whatever_is_typed() {
        let p = routine(
            RoutineKind::Function,
            "public",
            "greet",
            "(who text)",
            Some("text"),
        );
        assert_eq!(
            run(&DbDriver::Postgres, &p, &["o'brien"]).unwrap(),
            "SELECT \"public\".\"greet\"('o''brien')"
        );
        assert_eq!(
            run(&DbDriver::Postgres, &p, &["x'); DROP TABLE users; --"]).unwrap(),
            "SELECT \"public\".\"greet\"('x''); DROP TABLE users; --')"
        );
        // A number typed for text stays text.
        assert_eq!(
            run(&DbDriver::Postgres, &p, &["42"]).unwrap(),
            "SELECT \"public\".\"greet\"('42')"
        );
        // A JSON string passes exactly that string, so "null" is not NULL.
        assert_eq!(
            run(&DbDriver::Postgres, &p, &["\"null\""]).unwrap(),
            "SELECT \"public\".\"greet\"('null')"
        );
    }

    #[test]
    fn a_number_argument_refuses_anything_but_a_number() {
        let p = routine(
            RoutineKind::Function,
            "public",
            "add_numbers",
            "(a integer, b integer)",
            None,
        );
        let errors = run(&DbDriver::Postgres, &p, &["1) OR (1=1", "2"]).unwrap_err();
        assert_eq!(errors, vec!["Expects a number".to_string(), String::new()]);
    }

    #[test]
    fn postgres_set_returning_functions_are_selected_from() {
        let p = routine(
            RoutineKind::Function,
            "app",
            "orders_since",
            "(since date)",
            Some("TABLE(id integer, total numeric)"),
        );
        assert_eq!(
            run(&DbDriver::Postgres, &p, &["2024-01-01"]).unwrap(),
            "SELECT * FROM \"app\".\"orders_since\"('2024-01-01')"
        );
    }

    #[test]
    fn postgres_out_parameters_and_trailing_defaults() {
        let f = routine(
            RoutineKind::Function,
            "public",
            "split",
            "(n integer, OUT q integer, OUT r integer)",
            Some("record"),
        );
        let ps = params(&DbDriver::Postgres, &f);
        assert!(!takes_input(&DbDriver::Postgres, f.kind, &ps[1]));
        assert_eq!(
            run(&DbDriver::Postgres, &f, &["7", "", ""]).unwrap(),
            "SELECT * FROM \"public\".\"split\"(7)"
        );

        let p = routine(
            RoutineKind::Procedure,
            "public",
            "tally",
            "(IN n integer, OUT total integer)",
            None,
        );
        assert_eq!(
            run(&DbDriver::Postgres, &p, &["3", ""]).unwrap(),
            "CALL \"public\".\"tally\"(3, NULL)"
        );

        let d = routine(
            RoutineKind::Function,
            "public",
            "pad",
            "(s text, width integer DEFAULT 10, fill text DEFAULT ' '::text)",
            Some("text"),
        );
        assert_eq!(
            run(&DbDriver::Postgres, &d, &["ab", "", ""]).unwrap(),
            "SELECT \"public\".\"pad\"('ab')"
        );
        // A later box filled in keeps the earlier one, as NULL.
        assert_eq!(
            run(&DbDriver::Postgres, &d, &["ab", "", "*"]).unwrap(),
            "SELECT \"public\".\"pad\"('ab', NULL, '*')"
        );
    }

    #[test]
    fn postgres_booleans_and_variadic() {
        let p = routine(
            RoutineKind::Function,
            "public",
            "flag",
            "(on boolean)",
            Some("text"),
        );
        assert_eq!(
            run(&DbDriver::Postgres, &p, &["yes"]).unwrap(),
            "SELECT \"public\".\"flag\"(TRUE)"
        );
        assert_eq!(
            run(&DbDriver::Postgres, &p, &["maybe"]).unwrap_err(),
            vec!["Expects true or false"]
        );

        let v = routine(
            RoutineKind::Function,
            "public",
            "total",
            "(VARIADIC nums integer[])",
            Some("integer"),
        );
        assert_eq!(
            run(&DbDriver::Postgres, &v, &["{1,2,3}"]).unwrap(),
            "SELECT \"public\".\"total\"(VARIADIC '{1,2,3}')"
        );
    }

    #[test]
    fn identifiers_are_quoted_with_the_dialects_quoting() {
        let p = routine(
            RoutineKind::Function,
            "we\"ird",
            "sel\"ect",
            "()",
            Some("integer"),
        );
        assert_eq!(
            run(&DbDriver::Postgres, &p, &[]).unwrap(),
            "SELECT \"we\"\"ird\".\"sel\"\"ect\"()"
        );
    }

    // -- MySQL --------------------------------------------------------------------

    #[test]
    fn mysql_calls_procedures_and_gives_out_parameters_a_variable() {
        let p = routine(
            RoutineKind::Procedure,
            "shop",
            "place order",
            "(IN customer int, IN note varchar(200), OUT order_id int, INOUT counter int)",
            None,
        );
        let ps = params(&DbDriver::MySql, &p);
        assert!(!takes_input(&DbDriver::MySql, p.kind, &ps[3]));
        assert_eq!(
            run(&DbDriver::MySql, &p, &["5", "back\\slash 'quote'", "", ""]).unwrap(),
            "CALL `shop`.`place order`(5, 'back\\\\slash ''quote''', @dbhive_out_1, @dbhive_out_2)"
        );
    }

    #[test]
    fn mysql_functions_are_selected() {
        let f = routine(
            RoutineKind::Function,
            "shop",
            "tax",
            "(amount decimal(10,2))",
            Some("decimal(10,2)"),
        );
        assert_eq!(
            run(&DbDriver::MySql, &f, &["9.99"]).unwrap(),
            "SELECT `shop`.`tax`(9.99)"
        );
    }

    // -- SQL Server ---------------------------------------------------------------

    #[test]
    fn sqlserver_execs_procedures_with_unicode_text_and_bit() {
        let p = routine(
            RoutineKind::Procedure,
            "dbo",
            "usp_note",
            "(@id int, @text nvarchar, @urgent bit, @total int OUTPUT)",
            None,
        );
        assert_eq!(
            run(&DbDriver::SqlServer, &p, &["1", "café ]'", "true", ""]).unwrap(),
            "EXEC [dbo].[usp_note] 1, N'café ]''', 1, NULL"
        );
        let bare = routine(RoutineKind::Procedure, "dbo", "usp_run", "()", None);
        assert_eq!(
            run(&DbDriver::SqlServer, &bare, &[]).unwrap(),
            "EXEC [dbo].[usp_run]"
        );
    }

    #[test]
    fn sqlserver_functions_scalar_and_table_valued() {
        let f = routine(
            RoutineKind::Function,
            "dbo",
            "fn_total",
            "(@a int)",
            Some("int"),
        );
        assert_eq!(
            run(&DbDriver::SqlServer, &f, &["2"]).unwrap(),
            "SELECT [dbo].[fn_total](2)"
        );
        let t = routine(
            RoutineKind::Function,
            "dbo",
            "fn_rows",
            "(@since date)",
            Some("TABLE"),
        );
        assert_eq!(
            run(&DbDriver::SqlServer, &t, &["2024-01-01"]).unwrap(),
            "SELECT * FROM [dbo].[fn_rows](N'2024-01-01')"
        );
    }

    // -- SQLite -------------------------------------------------------------------

    #[test]
    fn sqlite_has_no_routines_to_run() {
        let p = routine(RoutineKind::Function, "", "f", "()", None);
        assert!(run(&DbDriver::Sqlite, &p, &[]).is_err());
    }

    // -- The form -----------------------------------------------------------------

    #[test]
    fn the_form_asks_before_running_and_editing_withdraws_the_question() {
        let p = routine(
            RoutineKind::Function,
            "public",
            "add_numbers",
            "(a integer, b integer)",
            Some("integer"),
        );
        let mut form = RunForm::new(&DbDriver::Postgres, &p);
        assert_eq!(form.values.len(), 2);

        form.set_value(0, "abc");
        assert!(!form.ask(&pg_q, &esc), "a wrong box cannot be confirmed");
        let v = form.view("Local", &pg_q, &esc);
        assert!(!v.can_run && v.sql.is_empty());
        assert_eq!(v.fields[0].error, "Expects a number");

        form.set_value(0, "1");
        form.set_value(1, "2");
        assert!(
            form.take_confirmed(&pg_q, &esc).is_none(),
            "nothing runs unconfirmed"
        );
        assert!(form.ask(&pg_q, &esc));
        let v = form.view("Local", &pg_q, &esc);
        assert!(v.confirming && v.confirm_text.contains("Local"));
        assert_eq!(v.run_label, "Run function");

        form.set_value(1, "3");
        assert!(!form.confirming, "a changed box needs a new confirmation");
        assert!(form.ask(&pg_q, &esc));
        assert_eq!(
            form.take_confirmed(&pg_q, &esc).as_deref(),
            Some("SELECT \"public\".\"add_numbers\"(1, 3)")
        );
        assert!(
            form.take_confirmed(&pg_q, &esc).is_none(),
            "a confirmation is used once"
        );
    }

    #[test]
    fn output_parameters_show_a_note_not_a_box() {
        let p = routine(
            RoutineKind::Function,
            "public",
            "split",
            "(n integer, OUT q integer)",
            Some("record"),
        );
        let form = RunForm::new(&DbDriver::Postgres, &p);
        let v = form.view("Local", &pg_q, &esc);
        assert!(v.fields[0].input && !v.fields[1].input);
        assert!(v.fields[1].hint.starts_with("Output"));
        assert_eq!(v.fields[0].hint, "A number; empty for NULL");
    }
}
