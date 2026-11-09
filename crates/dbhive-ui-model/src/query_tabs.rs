//! The workspace tab bar's extras and the query result grid's selection.
//!
//! - The tab bar's right-click menu (Close, Close Others, Close to the Right,
//!   Close All): which entries apply, which tabs each one closes, and what to
//!   ask when some of them hold unsaved work.
//! - Keyboard cycling between tabs (arrows on the bar, Ctrl+Tab anywhere).
//! - A query tab's selected cell, which must be forgotten when the rows it
//!   points at change (a new run, a filter, a sort).
//!
//! Pure: no toolkit, no async. The shell asks and acts.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabMenuAction {
    Close,
    CloseOthers,
    CloseToRight,
    CloseAll,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabMenuEntry {
    pub action: TabMenuAction,
    pub label: &'static str,
    /// Name of a kit icon; the view maps it to the image.
    pub icon: &'static str,
    pub enabled: bool,
    /// A rule above the entry, to set the bulk closes apart.
    pub divider_above: bool,
}

/// The menu for the tab at `index` of `count` tabs. Entries that would close
/// nothing are present but disabled, so the menu keeps its shape.
pub fn tab_menu(index: usize, count: usize) -> Vec<TabMenuEntry> {
    let entry = |action, label, icon, enabled, divider_above| TabMenuEntry {
        action,
        label,
        icon,
        enabled,
        divider_above,
    };
    vec![
        entry(TabMenuAction::Close, "Close", "x", index < count, false),
        entry(
            TabMenuAction::CloseOthers,
            "Close Others",
            "x",
            count > 1 && index < count,
            false,
        ),
        entry(
            TabMenuAction::CloseToRight,
            "Close to the Right",
            "x",
            index + 1 < count,
            false,
        ),
        entry(TabMenuAction::CloseAll, "Close All", "x", count > 0, true),
    ]
}

/// The ids a menu action closes, left to right, for the tab at `index` among
/// `ids` (every tab's id in bar order).
pub fn tabs_to_close(action: TabMenuAction, index: usize, ids: &[u64]) -> Vec<u64> {
    if index >= ids.len() {
        return Vec::new();
    }
    match action {
        TabMenuAction::Close => vec![ids[index]],
        TabMenuAction::CloseOthers => ids
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .map(|(_, id)| *id)
            .collect(),
        TabMenuAction::CloseToRight => ids[index + 1..].to_vec(),
        TabMenuAction::CloseAll => ids.to_vec(),
    }
}

/// The confirmation for closing several tabs when some hold work that would
/// be lost. `None` when nothing needs asking. `unsaved` are the titles of the
/// tabs that do.
pub fn bulk_close_prompt(closing: usize, unsaved: &[String]) -> Option<(String, String, String)> {
    if unsaved.is_empty() {
        return None;
    }
    let title = if closing == 1 {
        "Close this tab?".to_string()
    } else {
        format!("Close {closing} tabs?")
    };
    let names = quoted_list(unsaved, 3);
    let message = if unsaved.len() == 1 {
        format!("{names} has unsaved SQL or row changes that will be lost.")
    } else {
        format!(
            "{} of them have unsaved SQL or row changes that will be lost: {names}.",
            unsaved.len()
        )
    };
    let label = if closing == 1 {
        "Close tab".to_string()
    } else {
        format!("Close {closing} tabs")
    };
    Some((title, message, label))
}

/// `"a", "b" and 2 more`.
fn quoted_list(items: &[String], show: usize) -> String {
    let shown: Vec<String> = items
        .iter()
        .take(show)
        .map(|s| format!("\"{s}\""))
        .collect();
    let rest = items.len().saturating_sub(show);
    if rest > 0 {
        format!("{} and {rest} more", shown.join(", "))
    } else {
        shown.join(", ")
    }
}

/// The tab `delta` steps from `active`, wrapping at either end. With no
/// active tab the first (or last, going back) is picked.
pub fn cycle(active: Option<usize>, count: usize, delta: i32) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let n = count as i64;
    let next = match active {
        Some(i) => (i as i64 + delta as i64).rem_euclid(n),
        None if delta < 0 => n - 1,
        None => 0,
    };
    Some(next as usize)
}

/// A key on the focused tab bar, already decoded by the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarKey {
    Left,
    Right,
    Home,
    End,
}

/// Where a key on the focused tab bar moves the selection. Left and Right
/// stop at the ends (the bar is a list, not a ring); Ctrl+Tab wraps.
pub fn bar_key(active: Option<usize>, count: usize, key: BarKey) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let i = active.unwrap_or(0).min(count - 1);
    Some(match key {
        BarKey::Left => i.saturating_sub(1),
        BarKey::Right => (i + 1).min(count - 1),
        BarKey::Home => 0,
        BarKey::End => count - 1,
    })
}

/// The selected cell of a query tab's grid, and what it was selected in.
/// Row is an index into the rows the grid shows (after filter and sort), so
/// any change to those makes it point at a different row: it is then dropped
/// rather than silently moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridSelection {
    pub tab: u64,
    pub run_seq: u64,
    pub filter: String,
    pub sort: Option<(usize, bool)>,
    pub row: usize,
    pub col: usize,
}

impl GridSelection {
    /// Whether this still names the same cell of the grid described.
    pub fn holds(
        &self,
        tab: u64,
        run_seq: u64,
        filter: &str,
        sort: Option<(usize, bool)>,
        visible_rows: usize,
        columns: usize,
    ) -> bool {
        self.tab == tab
            && self.run_seq == run_seq
            && self.filter == filter
            && self.sort == sort
            && self.row < visible_rows
            && self.col < columns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_disables_what_would_close_nothing() {
        let only = tab_menu(0, 1);
        assert!(only[0].enabled, "Close");
        assert!(!only[1].enabled, "Close Others with one tab");
        assert!(!only[2].enabled, "Close to the Right on the last tab");
        assert!(only[3].enabled, "Close All");
        assert!(only[3].divider_above);

        let middle = tab_menu(1, 3);
        assert!(middle.iter().all(|e| e.enabled));
        let last = tab_menu(2, 3);
        assert!(!last[2].enabled);
    }

    #[test]
    fn each_action_closes_the_right_tabs() {
        let ids = [10, 20, 30, 40];
        assert_eq!(tabs_to_close(TabMenuAction::Close, 1, &ids), vec![20]);
        assert_eq!(
            tabs_to_close(TabMenuAction::CloseOthers, 1, &ids),
            vec![10, 30, 40]
        );
        assert_eq!(
            tabs_to_close(TabMenuAction::CloseToRight, 1, &ids),
            vec![30, 40]
        );
        assert!(tabs_to_close(TabMenuAction::CloseToRight, 3, &ids).is_empty());
        assert_eq!(
            tabs_to_close(TabMenuAction::CloseAll, 2, &ids),
            ids.to_vec()
        );
        assert!(tabs_to_close(TabMenuAction::Close, 9, &ids).is_empty());
    }

    #[test]
    fn a_bulk_close_asks_only_when_work_would_be_lost() {
        assert!(bulk_close_prompt(4, &[]).is_none());
        let (title, message, label) = bulk_close_prompt(4, &["Query 1".into()]).unwrap();
        assert_eq!(title, "Close 4 tabs?");
        assert!(message.starts_with("\"Query 1\" has unsaved"), "{message}");
        assert_eq!(label, "Close 4 tabs");

        let many: Vec<String> = ["a", "b", "c", "d", "e"].map(String::from).to_vec();
        let (_, message, _) = bulk_close_prompt(5, &many).unwrap();
        assert!(message.starts_with("5 of them"), "{message}");
        assert!(
            message.contains("\"a\", \"b\", \"c\" and 2 more"),
            "{message}"
        );

        let (title, _, label) = bulk_close_prompt(1, &["x".into()]).unwrap();
        assert_eq!(title, "Close this tab?");
        assert_eq!(label, "Close tab");
    }

    #[test]
    fn cycling_wraps_at_both_ends() {
        assert_eq!(cycle(Some(0), 3, 1), Some(1));
        assert_eq!(cycle(Some(2), 3, 1), Some(0));
        assert_eq!(cycle(Some(0), 3, -1), Some(2));
        assert_eq!(cycle(None, 3, 1), Some(0));
        assert_eq!(cycle(None, 3, -1), Some(2));
        assert_eq!(cycle(Some(0), 0, 1), None);
        assert_eq!(cycle(Some(0), 1, 1), Some(0));
    }

    #[test]
    fn bar_keys_stop_at_the_ends() {
        assert_eq!(bar_key(Some(0), 3, BarKey::Left), Some(0));
        assert_eq!(bar_key(Some(2), 3, BarKey::Right), Some(2));
        assert_eq!(bar_key(Some(1), 3, BarKey::Right), Some(2));
        assert_eq!(bar_key(Some(1), 3, BarKey::Home), Some(0));
        assert_eq!(bar_key(Some(0), 3, BarKey::End), Some(2));
        assert_eq!(bar_key(None, 0, BarKey::End), None);
    }

    #[test]
    fn a_selection_is_dropped_when_its_rows_change() {
        let sel = GridSelection {
            tab: 1,
            run_seq: 2,
            filter: String::new(),
            sort: None,
            row: 4,
            col: 1,
        };
        assert!(sel.holds(1, 2, "", None, 10, 3));
        assert!(!sel.holds(1, 3, "", None, 10, 3), "a new run");
        assert!(!sel.holds(1, 2, "x", None, 10, 3), "a filter");
        assert!(!sel.holds(1, 2, "", Some((0, false)), 10, 3), "a sort");
        assert!(!sel.holds(2, 2, "", None, 10, 3), "another tab");
        assert!(!sel.holds(1, 2, "", None, 4, 3), "row out of range");
        assert!(!sel.holds(1, 2, "", None, 10, 1), "column out of range");
    }
}
