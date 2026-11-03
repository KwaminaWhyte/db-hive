//! The plugin marketplace listing.
//!
//! There is no marketplace server. The Tauri shell's `get_marketplace_plugins`
//! command returned a hard-coded catalogue of three entries, and this is that
//! catalogue, moved somewhere both shells can read it and given the search,
//! filter and sort behaviour that used to be split between Rust (category and
//! search) and TypeScript (sort and the verified filter).
//!
//! Keeping it here rather than in the UI means the ordering rules are
//! testable, and means the day a real endpoint appears only [`catalog`] has to
//! change.

#![allow(dead_code)] // shared by both shells; neither uses every accessor.

use serde::{Deserialize, Serialize};

use super::manifest::{PluginAuthor, PluginCategory, PluginManifest, PluginPermission, PluginType};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MarketplacePlugin {
    pub manifest: PluginManifest,
    pub stats: MarketplaceStats,
    #[serde(default)]
    pub screenshots: Vec<String>,
    #[serde(default)]
    pub changelog: Option<String>,
    #[serde(default)]
    pub readme: Option<String>,
    pub download_url: String,
    #[serde(default)]
    pub size: u64,
    /// SHA-256 of the artifact. Checked before anything is installed; see
    /// `super::manager::verify_artifact_hash`.
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceStats {
    pub downloads: u64,
    pub stars: u64,
    pub rating: f32,
    pub reviews: u64,
    /// RFC 3339.
    pub updated_at: String,
    pub created_at: String,
}

/// Sort orders offered by the marketplace tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortOption {
    Popular,
    Recent,
    Rating,
    Downloads,
    Name,
}

impl SortOption {
    pub const ALL: [SortOption; 5] = [
        SortOption::Popular,
        SortOption::Recent,
        SortOption::Rating,
        SortOption::Downloads,
        SortOption::Name,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SortOption::Popular => "Most popular",
            SortOption::Recent => "Recently updated",
            SortOption::Rating => "Top rated",
            SortOption::Downloads => "Most downloaded",
            SortOption::Name => "Name",
        }
    }
}

impl Default for SortOption {
    fn default() -> Self {
        SortOption::Popular
    }
}

/// What the marketplace tab is currently showing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filters {
    pub search: String,
    pub category: Option<PluginCategory>,
    pub verified_only: bool,
    pub sort: SortOption,
}

/// Search, filter and sort in one pass, matching the web shell's behaviour:
/// case-insensitive substring search over name, description and keywords;
/// category equality; verified flag; then the chosen sort.
pub fn filter_and_sort(plugins: &[MarketplacePlugin], filters: &Filters) -> Vec<MarketplacePlugin> {
    let needle = filters.search.trim().to_lowercase();

    let mut out: Vec<MarketplacePlugin> = plugins
        .iter()
        .filter(|p| match filters.category {
            Some(c) => p.manifest.category == c,
            None => true,
        })
        .filter(|p| !filters.verified_only || p.verified)
        .filter(|p| {
            if needle.is_empty() {
                return true;
            }
            p.manifest.name.to_lowercase().contains(&needle)
                || p.manifest.description.to_lowercase().contains(&needle)
                || p.manifest
                    .keywords
                    .iter()
                    .any(|k| k.to_lowercase().contains(&needle))
        })
        .cloned()
        .collect();

    match filters.sort {
        SortOption::Popular => out.sort_by(|a, b| b.stats.stars.cmp(&a.stats.stars)),
        SortOption::Downloads => out.sort_by(|a, b| b.stats.downloads.cmp(&a.stats.downloads)),
        // RFC 3339 timestamps in a fixed zone sort correctly as strings, and
        // an unparseable one simply sorts last rather than panicking.
        SortOption::Recent => out.sort_by(|a, b| b.stats.updated_at.cmp(&a.stats.updated_at)),
        SortOption::Rating => out.sort_by(|a, b| {
            b.stats
                .rating
                .partial_cmp(&a.stats.rating)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        SortOption::Name => out.sort_by(|a, b| {
            a.manifest
                .name
                .to_lowercase()
                .cmp(&b.manifest.name.to_lowercase())
        }),
    }
    out
}

/// "15.2K", "1.5M": the compact download counts the cards show.
pub fn format_downloads(downloads: u64) -> String {
    if downloads >= 1_000_000 {
        format!("{:.1}M", downloads as f64 / 1_000_000.0)
    } else if downloads >= 1_000 {
        format!("{:.1}K", downloads as f64 / 1_000.0)
    } else {
        downloads.to_string()
    }
}

pub fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

/// The catalogue. Static, exactly as the Tauri command returned it.
pub fn catalog() -> Vec<MarketplacePlugin> {
    vec![
        MarketplacePlugin {
            manifest: PluginManifest {
                id: "com.dbhive.csv-exporter".into(),
                name: "Advanced CSV Exporter".into(),
                version: "1.0.0".into(),
                description: "Export query results to CSV with advanced formatting options".into(),
                author: PluginAuthor {
                    name: "DB Hive Team".into(),
                    email: Some("plugins@dbhive.com".into()),
                    url: None,
                },
                category: PluginCategory::Export,
                main: "index.js".into(),
                plugin_type: PluginType::JavaScript,
                permissions: vec![PluginPermission::ExecuteQuery, PluginPermission::WriteFiles],
                min_version: "0.13.0".into(),
                max_version: None,
                icon: Some("csv-icon.svg".into()),
                homepage: None,
                repository: None,
                license: "MIT".into(),
                keywords: vec!["csv".into(), "export".into(), "data".into()],
                dependencies: None,
                config_schema: None,
            },
            stats: MarketplaceStats {
                downloads: 15_234,
                stars: 342,
                rating: 4.8,
                reviews: 89,
                updated_at: "2025-11-20T10:00:00Z".into(),
                created_at: "2025-10-01T10:00:00Z".into(),
            },
            screenshots: vec![],
            changelog: Some("v1.0.0: Initial release with advanced CSV formatting".into()),
            readme: None,
            download_url: "https://plugins.dbhive.com/csv-exporter-1.0.0.zip".into(),
            size: 45_678,
            hash: "abc123def456".into(),
            signature: None,
            verified: true,
        },
        MarketplacePlugin {
            manifest: PluginManifest {
                id: "com.dbhive.dark-theme-plus".into(),
                name: "Dark Theme Plus".into(),
                version: "2.1.0".into(),
                description: "Enhanced dark theme with customizable accent colors".into(),
                author: PluginAuthor {
                    name: "Theme Designer".into(),
                    email: None,
                    url: Some("https://themes.example.com".into()),
                },
                category: PluginCategory::Theme,
                main: "theme.css".into(),
                plugin_type: PluginType::JavaScript,
                permissions: vec![PluginPermission::ModifyUi],
                min_version: "0.12.0".into(),
                max_version: None,
                icon: Some("theme-icon.svg".into()),
                homepage: None,
                repository: None,
                license: "MIT".into(),
                keywords: vec!["theme".into(), "dark".into(), "ui".into()],
                dependencies: None,
                config_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "accentColor": { "type": "string", "default": "#3b82f6" }
                    }
                })),
            },
            stats: MarketplaceStats {
                downloads: 28_456,
                stars: 567,
                rating: 4.9,
                reviews: 234,
                updated_at: "2025-11-15T10:00:00Z".into(),
                created_at: "2025-09-01T10:00:00Z".into(),
            },
            screenshots: vec!["screenshot1.png".into(), "screenshot2.png".into()],
            changelog: Some("v2.1.0: Added 10 new accent colors".into()),
            readme: None,
            download_url: "https://plugins.dbhive.com/dark-theme-plus-2.1.0.zip".into(),
            size: 23_456,
            hash: "xyz789ghi012".into(),
            signature: None,
            verified: true,
        },
        MarketplacePlugin {
            manifest: PluginManifest {
                id: "com.dbhive.sql-formatter".into(),
                name: "SQL Formatter Pro".into(),
                version: "1.5.2".into(),
                description: "Format and beautify SQL queries with multiple style options".into(),
                author: PluginAuthor {
                    name: "SQL Tools Inc".into(),
                    email: Some("support@sqltools.com".into()),
                    url: None,
                },
                category: PluginCategory::Formatter,
                main: "formatter.js".into(),
                plugin_type: PluginType::JavaScript,
                permissions: vec![PluginPermission::ModifyUi],
                min_version: "0.11.0".into(),
                max_version: None,
                icon: Some("formatter-icon.svg".into()),
                homepage: Some("https://sqltools.com".into()),
                repository: Some("https://github.com/sqltools/formatter".into()),
                license: "Apache-2.0".into(),
                keywords: vec!["sql".into(), "formatter".into(), "beautify".into()],
                dependencies: None,
                config_schema: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "indentSize": { "type": "number", "default": 2 },
                        "uppercase": { "type": "boolean", "default": true }
                    }
                })),
            },
            stats: MarketplaceStats {
                downloads: 45_678,
                stars: 890,
                rating: 4.7,
                reviews: 456,
                updated_at: "2025-11-22T10:00:00Z".into(),
                created_at: "2025-08-15T10:00:00Z".into(),
            },
            screenshots: vec![],
            changelog: Some("v1.5.2: Fixed formatting for complex JOIN queries".into()),
            readme: Some("# SQL Formatter Pro\n\nFormat your SQL queries with style!".into()),
            download_url: "https://plugins.dbhive.com/sql-formatter-1.5.2.zip".into(),
            size: 67_890,
            hash: "qwe345rty678".into(),
            signature: None,
            verified: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_entries_are_well_formed() {
        for p in catalog() {
            p.manifest
                .validate()
                .unwrap_or_else(|e| panic!("{}: {}", p.manifest.id, e));
        }
    }

    #[test]
    fn search_matches_name_description_and_keywords() {
        let all = catalog();
        let find = |q: &str| {
            filter_and_sort(
                &all,
                &Filters {
                    search: q.into(),
                    ..Default::default()
                },
            )
            .into_iter()
            .map(|p| p.manifest.id)
            .collect::<Vec<_>>()
        };

        assert_eq!(find("csv"), vec!["com.dbhive.csv-exporter"]);
        assert_eq!(
            find("CSV"),
            vec!["com.dbhive.csv-exporter"],
            "case-insensitive"
        );
        assert_eq!(
            find("beautify"),
            vec!["com.dbhive.sql-formatter"],
            "keyword"
        );
        assert_eq!(
            find("accent colors"),
            vec!["com.dbhive.dark-theme-plus"],
            "description"
        );
        assert!(find("no such plugin").is_empty());
        assert_eq!(find("   ").len(), 3, "a blank search filters nothing");
    }

    #[test]
    fn category_and_verified_filters_apply() {
        let all = catalog();
        let themes = filter_and_sort(
            &all,
            &Filters {
                category: Some(PluginCategory::Theme),
                ..Default::default()
            },
        );
        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0].manifest.id, "com.dbhive.dark-theme-plus");

        let mut unverified = all.clone();
        unverified[0].verified = false;
        let verified = filter_and_sort(
            &unverified,
            &Filters {
                verified_only: true,
                ..Default::default()
            },
        );
        assert_eq!(verified.len(), 2);
        assert!(verified.iter().all(|p| p.verified));
    }

    #[test]
    fn every_sort_order_matches_the_web_shell() {
        let all = catalog();
        let ids = |sort: SortOption| {
            filter_and_sort(
                &all,
                &Filters {
                    sort,
                    ..Default::default()
                },
            )
            .into_iter()
            .map(|p| p.manifest.name)
            .collect::<Vec<_>>()
        };

        // stars: 890, 567, 342
        assert_eq!(
            ids(SortOption::Popular),
            vec![
                "SQL Formatter Pro",
                "Dark Theme Plus",
                "Advanced CSV Exporter"
            ]
        );
        // downloads: 45678, 28456, 15234
        assert_eq!(
            ids(SortOption::Downloads),
            vec![
                "SQL Formatter Pro",
                "Dark Theme Plus",
                "Advanced CSV Exporter"
            ]
        );
        // rating: 4.9, 4.8, 4.7
        assert_eq!(
            ids(SortOption::Rating),
            vec![
                "Dark Theme Plus",
                "Advanced CSV Exporter",
                "SQL Formatter Pro"
            ]
        );
        // updatedAt: Nov 22, Nov 20, Nov 15
        assert_eq!(
            ids(SortOption::Recent),
            vec![
                "SQL Formatter Pro",
                "Advanced CSV Exporter",
                "Dark Theme Plus"
            ]
        );
        assert_eq!(
            ids(SortOption::Name),
            vec![
                "Advanced CSV Exporter",
                "Dark Theme Plus",
                "SQL Formatter Pro"
            ]
        );
    }

    #[test]
    fn filters_compose() {
        let all = catalog();
        let out = filter_and_sort(
            &all,
            &Filters {
                search: "sql".into(),
                category: Some(PluginCategory::Formatter),
                verified_only: true,
                sort: SortOption::Name,
            },
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].manifest.id, "com.dbhive.sql-formatter");
    }

    #[test]
    fn compact_number_formatting() {
        assert_eq!(format_downloads(999), "999");
        assert_eq!(format_downloads(15_234), "15.2K");
        assert_eq!(format_downloads(1_500_000), "1.5M");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(45_678), "44.6 KB");
        assert_eq!(format_size(3 * 1024 * 1024), "3.0 MB");
    }
}
