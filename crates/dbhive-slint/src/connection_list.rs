//! What the connections screen shows, as pure functions.
//!
//! Filtering, ordering and the per-driver chip are decisions, not drawing, so
//! they live here where they can be tested without a window.

use dbhive_core::models::connection::Environment;
use dbhive_core::models::{ConnectionProfile, DbDriver};

/// Initials and tint for a driver's chip.
pub fn driver_chip(driver: &DbDriver) -> (&'static str, (u8, u8, u8)) {
    match driver {
        DbDriver::Postgres => ("PG", (56, 118, 171)),
        DbDriver::Supabase => ("SB", (62, 175, 124)),
        DbDriver::Neon => ("NE", (0, 200, 140)),
        DbDriver::MySql => ("MY", (0, 117, 143)),
        DbDriver::Sqlite => ("SL", (0, 121, 168)),
        DbDriver::MongoDb => ("MG", (77, 179, 61)),
        DbDriver::SqlServer => ("MS", (168, 61, 61)),
        DbDriver::Turso => ("TU", (78, 205, 196)),
        DbDriver::Redis => ("RD", (196, 60, 44)),
    }
}

/// How an environment is labelled, if the profile has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvTone {
    Neutral,
    Warning,
    Danger,
}

pub fn environment(profile: &ConnectionProfile) -> Option<(&'static str, EnvTone)> {
    match profile.environment.as_ref()? {
        Environment::Local => Some(("Local", EnvTone::Neutral)),
        Environment::Staging => Some(("Staging", EnvTone::Warning)),
        Environment::Production => Some(("Production", EnvTone::Danger)),
    }
}

/// Profiles matching `search`, favourites first, then by name.
///
/// Matching is case-insensitive on name and host, which is what people
/// actually type.
pub fn visible<'a>(profiles: &'a [ConnectionProfile], search: &str) -> Vec<&'a ConnectionProfile> {
    let needle = search.trim().to_lowercase();
    let mut out: Vec<&ConnectionProfile> = profiles
        .iter()
        .filter(|p| {
            needle.is_empty()
                || p.name.to_lowercase().contains(&needle)
                || p.host.to_lowercase().contains(&needle)
        })
        .collect();
    out.sort_by(|a, b| {
        b.is_favorite
            .cmp(&a.is_favorite)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

/// `host:port`, or the file path for file-based drivers.
pub fn address(p: &ConnectionProfile) -> String {
    match p.driver {
        DbDriver::Sqlite => p.database.clone().unwrap_or_else(|| p.host.clone()),
        _ => format!("{}:{}", p.host, p.port),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str, host: &str, fav: bool) -> ConnectionProfile {
        let mut p = ConnectionProfile::new(
            name.to_string(),
            name.to_string(),
            DbDriver::Postgres,
            host.to_string(),
            5432,
            String::new(),
        );
        p.is_favorite = fav;
        p
    }

    #[test]
    fn favourites_come_first_then_names_case_insensitively() {
        let ps = vec![
            profile("beta", "h", false),
            profile("Alpha", "h", false),
            profile("zeta", "h", true),
        ];
        let names: Vec<&str> = visible(&ps, "").iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["zeta", "Alpha", "beta"]);
    }

    #[test]
    fn search_matches_name_or_host_ignoring_case() {
        let ps = vec![
            profile("Local MySQL", "localhost", false),
            profile("Prod", "db.example.com", false),
        ];
        assert_eq!(visible(&ps, "local").len(), 1);
        assert_eq!(visible(&ps, "EXAMPLE").len(), 1);
        assert_eq!(visible(&ps, "  ").len(), 2, "whitespace is not a filter");
        assert!(visible(&ps, "nothing").is_empty());
    }

    #[test]
    fn every_driver_has_distinct_initials() {
        use DbDriver::*;
        let all = [Postgres, Supabase, Neon, MySql, Sqlite, MongoDb, SqlServer, Turso, Redis];
        let mut seen = std::collections::HashSet::new();
        for d in all {
            assert!(seen.insert(driver_chip(&d).0), "{:?} shares initials", d);
        }
    }

    #[test]
    fn address_uses_host_and_port() {
        assert_eq!(address(&profile("a", "localhost", false)), "localhost:5432");
    }
}
