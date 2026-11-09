//! Grouping saved connections by folder for the connections list.
//!
//! A profile's `folder` is free text typed in the connection editor. The
//! list shows one collapsible header per folder with its connections under
//! it, then the connections with no folder under "Unfiled". With no folders
//! at all the list stays flat: a lone "Unfiled" header would only add noise.
//!
//! Folder names match ignoring case and surrounding spaces, so "Work" and
//! "work " land in one group (shown with the first spelling met). Which
//! folders are collapsed is a set of [`folder_key`]s the shell persists.

use std::collections::{HashMap, HashSet};

use dbhive_core::models::ConnectionProfile;

/// The header label for connections without a folder.
pub const UNFILED: &str = "Unfiled";

/// One row of the grouped list.
#[derive(Debug, Clone)]
pub enum Entry<'a> {
    Folder {
        /// The collapse key ([`folder_key`]); empty for "Unfiled".
        key: String,
        /// The name to show.
        label: String,
        /// Connections in it that match the current search.
        count: usize,
        collapsed: bool,
    },
    Connection(&'a ConnectionProfile),
}

/// A profile's folder, trimmed, or `None` when it has none.
pub fn folder_of(profile: &ConnectionProfile) -> Option<&str> {
    profile
        .folder
        .as_deref()
        .map(str::trim)
        .filter(|f| !f.is_empty())
}

/// The key a folder is grouped and collapsed by.
pub fn folder_key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// The spelling shown for a folder, from every spelling met.
///
/// Profiles come back from the store in no guaranteed order, so "Work" and
/// "work" would otherwise be labelled by whichever arrived first. The
/// lexicographically smallest spelling wins, which is stable and prefers the
/// capitalised form ("Work" sorts before "work").
fn label_for<'a>(spellings: impl IntoIterator<Item = &'a str>) -> String {
    spellings
        .into_iter()
        .min()
        .map(str::to_string)
        .unwrap_or_default()
}

/// Every distinct folder name in `profiles`, sorted ignoring case, one
/// stable spelling each. For the editor's suggestions and the header count.
pub fn folder_names(profiles: &[ConnectionProfile]) -> Vec<String> {
    let mut by_key: HashMap<String, Vec<&str>> = HashMap::new();
    for f in profiles.iter().filter_map(folder_of) {
        by_key.entry(folder_key(f)).or_default().push(f);
    }
    let mut out: Vec<String> = by_key.into_values().map(label_for).collect();
    out.sort_by_key(|f| folder_key(f));
    out
}

/// Group `visible` (already filtered and ordered) into folder sections.
///
/// `catalog` is every profile, filtered or not: folder names come from it, so
/// a search does not change "Work" to "work" just because the one match
/// spelled it that way. While `searching`, collapsed folders open: a match
/// hidden inside a closed folder looks like no match at all.
pub fn group<'a>(
    visible: &[&'a ConnectionProfile],
    catalog: &[ConnectionProfile],
    collapsed: &HashSet<String>,
    searching: bool,
) -> Vec<Entry<'a>> {
    if !visible.iter().any(|p| folder_of(p).is_some()) {
        return visible.iter().map(|p| Entry::Connection(p)).collect();
    }

    let mut spellings: HashMap<String, Vec<&str>> = HashMap::new();
    for f in catalog.iter().filter_map(folder_of) {
        spellings.entry(folder_key(f)).or_default().push(f);
    }
    let labels: HashMap<String, String> = spellings
        .into_iter()
        .map(|(k, v)| (k, label_for(v)))
        .collect();

    // Folders in name order, each keeping the incoming connection order.
    let mut sections: Vec<(String, String, Vec<&'a ConnectionProfile>)> = Vec::new();
    let mut unfiled: Vec<&'a ConnectionProfile> = Vec::new();
    for p in visible {
        match folder_of(p) {
            Some(f) => {
                let key = folder_key(f);
                match sections.iter_mut().find(|(k, _, _)| *k == key) {
                    Some((_, _, items)) => items.push(p),
                    None => {
                        let label = labels.get(&key).cloned().unwrap_or_else(|| f.to_string());
                        sections.push((key, label, vec![p]))
                    }
                }
            }
            None => unfiled.push(p),
        }
    }
    sections.sort_by(|a, b| a.0.cmp(&b.0));
    if !unfiled.is_empty() {
        sections.push((String::new(), UNFILED.to_string(), unfiled));
    }

    let mut out = Vec::new();
    for (key, label, items) in sections {
        let closed = !searching && collapsed.contains(&key);
        out.push(Entry::Folder {
            key,
            label,
            count: items.len(),
            collapsed: closed,
        });
        if !closed {
            out.extend(items.into_iter().map(Entry::Connection));
        }
    }
    out
}

/// The line under "Saved Connections".
pub fn summary(total: usize, folders: usize) -> String {
    let conns = match total {
        0 => return "No saved connections yet".to_string(),
        1 => "1 connection".to_string(),
        n => format!("{n} connections"),
    };
    match folders {
        0 => conns,
        1 => format!("{conns} in 1 folder"),
        n => format!("{conns} in {n} folders"),
    }
}

/// Flip a folder's collapsed state. Returns whether it is now collapsed.
pub fn toggle(collapsed: &mut HashSet<String>, key: &str) -> bool {
    if collapsed.remove(key) {
        false
    } else {
        collapsed.insert(key.to_string());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbhive_core::models::DbDriver;

    fn p(name: &str, folder: Option<&str>) -> ConnectionProfile {
        let mut p = ConnectionProfile::new(
            name.into(),
            name.into(),
            DbDriver::Postgres,
            "h".into(),
            5432,
            String::new(),
        );
        p.folder = folder.map(str::to_string);
        p
    }

    fn render(entries: &[Entry]) -> Vec<String> {
        entries
            .iter()
            .map(|e| match e {
                Entry::Folder {
                    label,
                    count,
                    collapsed,
                    ..
                } => format!(
                    "[{label} {count}{}]",
                    if *collapsed { " closed" } else { "" }
                ),
                Entry::Connection(p) => p.name.clone(),
            })
            .collect()
    }

    #[test]
    fn no_folders_means_a_flat_list() {
        let ps = vec![p("a", None), p("b", Some("  "))];
        let refs: Vec<&ConnectionProfile> = ps.iter().collect();
        assert_eq!(
            render(&group(&refs, &ps, &HashSet::new(), false)),
            ["a", "b"]
        );
    }

    #[test]
    fn folders_sorted_then_unfiled_last_keeping_connection_order() {
        let ps = vec![
            p("z-fav", Some("Work")),
            p("loose", None),
            p("a", Some("work ")),
            p("b", Some("Clients")),
        ];
        let refs: Vec<&ConnectionProfile> = ps.iter().collect();
        assert_eq!(
            render(&group(&refs, &ps, &HashSet::new(), false)),
            [
                "[Clients 1]",
                "b",
                "[Work 2]",
                "z-fav",
                "a",
                "[Unfiled 1]",
                "loose"
            ]
        );
    }

    #[test]
    fn collapsed_folders_hide_rows_but_keep_the_count() {
        let ps = vec![p("a", Some("Work")), p("b", None)];
        let refs: Vec<&ConnectionProfile> = ps.iter().collect();
        let mut closed = HashSet::new();
        assert!(toggle(&mut closed, &folder_key("WORK")));
        assert_eq!(
            render(&group(&refs, &ps, &closed, false)),
            ["[Work 1 closed]", "[Unfiled 1]", "b"]
        );
        // Unfiled collapses under the empty key.
        toggle(&mut closed, "");
        assert_eq!(
            render(&group(&refs, &ps, &closed, false)),
            ["[Work 1 closed]", "[Unfiled 1 closed]"]
        );
        // A search opens everything.
        assert_eq!(
            render(&group(&refs, &ps, &closed, true)),
            ["[Work 1]", "a", "[Unfiled 1]", "b"]
        );
        assert!(!toggle(&mut closed, "work"));
    }

    #[test]
    fn a_search_keeps_the_folder_name_it_had() {
        let ps = vec![p("Alpha", Some("Work")), p("Beta", Some("work "))];
        let just_beta: Vec<&ConnectionProfile> = vec![&ps[1]];
        assert_eq!(
            render(&group(&just_beta, &ps, &HashSet::new(), true)),
            ["[Work 1]", "Beta"]
        );
    }

    #[test]
    fn folder_names_are_distinct_ignoring_case() {
        for order in [["Work", "work"], ["work", "Work"], ["work", "WORK"]] {
            let ps = vec![
                p("a", Some(order[0])),
                p("b", Some(order[1])),
                p("c", Some("Analytics")),
                p("d", None),
            ];
            let shown = order[0].min(order[1]);
            assert_eq!(folder_names(&ps), ["Analytics", shown]);
        }
    }

    #[test]
    fn the_displayed_spelling_does_not_depend_on_profile_order() {
        let a = vec![p("Alpha", Some("Work")), p("Beta", Some("work"))];
        let b = vec![p("Beta", Some("work")), p("Alpha", Some("Work"))];
        for ps in [a, b] {
            let refs: Vec<&ConnectionProfile> = ps.iter().collect();
            let entries = group(&refs, &ps, &HashSet::new(), false);
            let label = match &entries[0] {
                Entry::Folder { label, .. } => label.clone(),
                other => panic!("expected a folder header, got {other:?}"),
            };
            assert_eq!(label, "Work");
        }
    }

    #[test]
    fn summary_reads_naturally() {
        assert_eq!(summary(0, 0), "No saved connections yet");
        assert_eq!(summary(1, 0), "1 connection");
        assert_eq!(summary(1, 1), "1 connection in 1 folder");
        assert_eq!(summary(5, 2), "5 connections in 2 folders");
    }
}
