//! What the Redis key browser shows: TTL wording, type filters, the key list
//! and a key's detail summary.
//!
//! The driver does the scanning and reading; this module only decides how
//! those values are worded and filtered, so it is testable without a server.

use dbhive_core::drivers::{KeyDetail, KeyEntry};

/// The type filter's options. Empty means every type.
pub const TYPE_ALL: &str = "All types";

pub fn type_options() -> Vec<String> {
    vec![
        TYPE_ALL.to_string(),
        "string".to_string(),
        "hash".to_string(),
        "list".to_string(),
        "set".to_string(),
        "zset".to_string(),
        "stream".to_string(),
    ]
}

/// "no expiry", "45s", "3m 20s", "2h 05m", "5d".
pub fn ttl_label(ttl: Option<i64>) -> String {
    let Some(seconds) = ttl else {
        return "no expiry".to_string();
    };
    let seconds = seconds.max(0);
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        let (m, s) = (seconds / 60, seconds % 60);
        if s == 0 {
            format!("{m}m")
        } else {
            format!("{m}m {s:02}s")
        }
    } else if seconds < 86_400 {
        let (h, m) = (seconds / 3600, (seconds % 3600) / 60);
        if m == 0 {
            format!("{h}h")
        } else {
            format!("{h}h {m:02}m")
        }
    } else {
        format!("{}d", seconds / 86_400)
    }
}

/// The keys to show: the loaded page, narrowed by type and by a
/// case-insensitive substring of the key.
pub fn filter<'a>(entries: &'a [KeyEntry], search: &str, kind: &str) -> Vec<&'a KeyEntry> {
    let needle = search.trim().to_lowercase();
    entries
        .iter()
        .filter(|e| kind.is_empty() || kind == TYPE_ALL || e.kind == kind)
        .filter(|e| needle.is_empty() || e.key.to_lowercase().contains(&needle))
        .collect()
}

/// The line under the key list: how many are loaded and whether more exist.
pub fn list_note(loaded: usize, cursor: u64) -> String {
    let keys = if loaded == 1 { "key" } else { "keys" };
    if cursor == 0 {
        format!("{loaded} {keys}")
    } else {
        format!("{loaded} {keys}, more available")
    }
}

/// The detail pane's headline: type, rows and expiry.
pub fn detail_summary(detail: &KeyDetail) -> String {
    let rows = detail.rows.len();
    let items = if rows == 1 { "entry" } else { "entries" };
    format!(
        "{} · {} {} · {}",
        detail.kind,
        rows,
        items,
        ttl_label(detail.ttl)
    )
}

/// The detail pane as TSV, for Copy.
pub fn detail_tsv(detail: &KeyDetail) -> String {
    let mut out = String::new();
    if !detail.columns.1.is_empty() {
        out.push_str(&format!("{}\t{}\n", detail.columns.0, detail.columns.1));
    }
    for (a, b) in &detail.rows {
        if detail.columns.1.is_empty() {
            out.push_str(&format!("{a}\n"));
        } else {
            out.push_str(&format!("{a}\t{b}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, kind: &str, ttl: Option<i64>) -> KeyEntry {
        KeyEntry {
            key: key.to_string(),
            kind: kind.to_string(),
            ttl,
        }
    }

    #[test]
    fn ttls_read_the_way_a_person_says_them() {
        assert_eq!(ttl_label(None), "no expiry");
        assert_eq!(ttl_label(Some(-1)), "0s");
        assert_eq!(ttl_label(Some(45)), "45s");
        assert_eq!(ttl_label(Some(60)), "1m");
        assert_eq!(ttl_label(Some(200)), "3m 20s");
        assert_eq!(ttl_label(Some(7200)), "2h");
        assert_eq!(ttl_label(Some(7500)), "2h 05m");
        assert_eq!(ttl_label(Some(172_800)), "2d");
    }

    #[test]
    fn the_filter_narrows_by_type_then_name() {
        let entries = vec![
            entry("user:1", "hash", None),
            entry("user:2", "string", Some(60)),
            entry("orders:1", "list", None),
        ];
        assert_eq!(filter(&entries, "", TYPE_ALL).len(), 3);
        assert_eq!(filter(&entries, "", "string").len(), 1);
        assert_eq!(filter(&entries, "USER", TYPE_ALL).len(), 2);
        assert_eq!(filter(&entries, "user", "hash")[0].key, "user:1");
        assert!(filter(&entries, "nothing", TYPE_ALL).is_empty());
    }

    #[test]
    fn the_list_note_says_whether_more_pages_exist() {
        assert_eq!(list_note(0, 0), "0 keys");
        assert_eq!(list_note(1, 0), "1 key");
        assert_eq!(list_note(200, 4912), "200 keys, more available");
    }

    #[test]
    fn the_detail_summary_names_type_rows_and_expiry() {
        let detail = KeyDetail {
            key: "k".into(),
            kind: "hash".into(),
            ttl: Some(90),
            columns: ("field".into(), "value".into()),
            rows: vec![("a".into(), "1".into())],
            note: String::new(),
        };
        assert_eq!(detail_summary(&detail), "hash · 1 entry · 1m 30s");
    }

    #[test]
    fn copy_produces_real_tsv_with_a_header_when_there_are_two_columns() {
        let two = KeyDetail {
            key: "k".into(),
            kind: "hash".into(),
            ttl: None,
            columns: ("field".into(), "value".into()),
            rows: vec![("a".into(), "1".into()), ("b".into(), "2".into())],
            note: String::new(),
        };
        assert_eq!(detail_tsv(&two), "field\tvalue\na\t1\nb\t2\n");

        let one = KeyDetail {
            columns: ("value".into(), String::new()),
            rows: vec![("hello".into(), String::new())],
            ..two
        };
        assert_eq!(detail_tsv(&one), "hello\n");
    }
}
