//! Pure decisions for the workspace screen: which database and schema a
//! freshly opened connection lands on, and how a `QueryResult` becomes grid
//! rows. Lifted from the same decisions the old egui shell's workspace made,
//! kept here rather than in `dbhive-ui-model` because they are this shell's
//! own async-flow shape, not logic the two shells share verbatim.

use dbhive_core::drivers::QueryResult;
use dbhive_core::models::{ConnectionProfile, DbDriver, SchemaInfo};
use dbhive_ui_model::results_grid::cell_text;

/// The database a fresh connection is on, mirroring
/// `dbhive_core::service::connect`'s own fallback: a Postgres-family profile
/// with no database configured lands on "postgres".
pub fn connected_database(profile: &ConnectionProfile) -> Option<String> {
    match profile.database.as_deref() {
        Some(d) if !d.is_empty() => Some(d.to_string()),
        _ if profile.driver.is_postgres_compatible() => Some("postgres".to_string()),
        _ => None,
    }
}

/// Prefer "public" when a Postgres server offers it, else the first schema
/// that is not one of the server's own internal ones.
///
/// Postgres always lists `pg_catalog` and `information_schema`; landing a
/// new connection there instead of the user's own schema would be useless.
pub fn pick_default_schema(schemas: &[SchemaInfo]) -> Option<String> {
    if schemas.iter().any(|s| s.name == "public") {
        return Some("public".to_string());
    }
    schemas
        .iter()
        .find(|s| !s.name.starts_with("pg_") && s.name != "information_schema")
        .or_else(|| schemas.first())
        .map(|s| s.name.clone())
}

/// The argument `get_tables` wants for this driver: a schema name on the
/// Postgres family, the database itself everywhere else (MySQL, SQLite and
/// the rest have no separate schema level `get_tables` cares about).
pub fn tables_schema_arg(
    driver: &DbDriver,
    database: &str,
    wanted_schema: Option<&str>,
    schemas: &[SchemaInfo],
) -> String {
    if driver.is_postgres_compatible() {
        wanted_schema
            .map(str::to_string)
            .or_else(|| pick_default_schema(schemas))
            .unwrap_or_else(|| "public".to_string())
    } else {
        database.to_string()
    }
}

/// Turn query rows into what `LazyRows` wants: `None` for a database NULL,
/// `Some` text otherwise, with numbers and booleans formatted the same way
/// the egui grid does (`dbhive_ui_model::results_grid::cell_text`).
pub fn to_grid_rows(result: &QueryResult) -> Vec<Vec<Option<String>>> {
    result
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|v| {
                    let (text, is_null) = cell_text(v);
                    if is_null {
                        None
                    } else {
                        Some(text)
                    }
                })
                .collect()
        })
        .collect()
}

/// A serviceable default width per column until real content measurement
/// lands; wide enough for most values, narrow enough that ten columns still
/// fit an ordinary window.
pub const DEFAULT_COLUMN_WIDTH: f32 = 160.0;

/// `1234567` -> `"1,234,567"`, for the sidebar's approximate row count.
pub fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn profile(driver: DbDriver, database: Option<&str>) -> ConnectionProfile {
        let mut p = ConnectionProfile::new(
            "id".into(),
            "name".into(),
            driver,
            "host".into(),
            0,
            "user".into(),
        );
        p.database = database.map(str::to_string);
        p
    }

    #[test]
    fn postgres_with_no_database_falls_back_to_postgres() {
        assert_eq!(
            connected_database(&profile(DbDriver::Postgres, None)),
            Some("postgres".to_string())
        );
    }

    #[test]
    fn mysql_with_no_database_has_none() {
        assert_eq!(connected_database(&profile(DbDriver::MySql, None)), None);
    }

    #[test]
    fn a_configured_database_wins_regardless_of_driver() {
        assert_eq!(
            connected_database(&profile(DbDriver::Postgres, Some("app"))),
            Some("app".to_string())
        );
    }

    fn schema(name: &str) -> SchemaInfo {
        SchemaInfo::new(name.to_string(), "db".to_string())
    }

    #[test]
    fn public_is_preferred_when_present() {
        let schemas = vec![schema("pg_catalog"), schema("public"), schema("app")];
        assert_eq!(pick_default_schema(&schemas), Some("public".to_string()));
    }

    #[test]
    fn falls_back_to_first_non_internal_schema() {
        let schemas = vec![
            schema("pg_catalog"),
            schema("information_schema"),
            schema("app"),
        ];
        assert_eq!(pick_default_schema(&schemas), Some("app".to_string()));
    }

    #[test]
    fn empty_schema_list_has_no_default() {
        assert_eq!(pick_default_schema(&[]), None);
    }

    #[test]
    fn postgres_tables_arg_is_a_schema() {
        let schemas = vec![schema("public")];
        assert_eq!(
            tables_schema_arg(&DbDriver::Postgres, "app", None, &schemas),
            "public"
        );
    }

    #[test]
    fn mysql_tables_arg_is_the_database() {
        assert_eq!(tables_schema_arg(&DbDriver::MySql, "app", None, &[]), "app");
    }

    #[test]
    fn a_chosen_schema_outranks_the_default() {
        let schemas = vec![schema("public")];
        assert_eq!(
            tables_schema_arg(&DbDriver::Postgres, "app", Some("app_schema"), &schemas),
            "app_schema"
        );
    }

    #[test]
    fn group_thousands_places_commas_every_three_digits() {
        assert_eq!(group_thousands(0), "0");
        assert_eq!(group_thousands(999), "999");
        assert_eq!(group_thousands(1000), "1,000");
        assert_eq!(group_thousands(1234567), "1,234,567");
    }

    #[test]
    fn grid_rows_flag_null_and_format_other_values() {
        let result = QueryResult::with_data(
            vec!["id".into(), "name".into()],
            vec![vec![json!(1), json!(null)]],
        );
        let rows = to_grid_rows(&result);
        assert_eq!(rows, vec![vec![Some("1".to_string()), None]]);
    }
}
