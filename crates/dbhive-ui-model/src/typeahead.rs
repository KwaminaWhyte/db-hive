//! Type-to-select in a short menu: press letters to move the highlight to
//! the entry that starts with them, the way native context menus behave.
//!
//! Rules (the macOS and Windows menu behaviour):
//!
//! - Typing builds a prefix; the shell clears it after a short pause.
//! - The prefix matches the start of a label, ignoring case and leading
//!   spaces. Disabled entries are skipped.
//! - A longer prefix keeps the current entry if it still matches, so typing
//!   "dr" after "d" does not jump past "Drop Table".
//! - Repeating one letter ("ddd") cycles through the entries starting with
//!   that letter, so short menus with several "D" entries stay reachable.
//! - Searching wraps around the end of the list.

/// Whether a key event's text is something to type ahead with: one
/// printable character. Named keys arrive as control or private-use
/// characters and are left to the menu's own handling.
pub fn is_typeahead_text(text: &str) -> bool {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => {
            !c.is_control() && !('\u{e000}'..='\u{f8ff}').contains(&c) && !c.is_whitespace()
        }
        _ => false,
    }
}

/// The entry to highlight after typing `typed` (the whole prefix so far,
/// including the key just pressed), with `current` highlighted now (`-1` for
/// none). `None` when nothing matches; the highlight should then stay put.
pub fn next_match(labels: &[(&str, bool)], current: i32, typed: &str) -> Option<usize> {
    let typed = typed.to_lowercase();
    let mut chars = typed.chars();
    let first = chars.next()?;
    let repeating = chars.all(|c| c == first);
    let n = labels.len();
    if n == 0 {
        return None;
    }

    let matches = |i: usize, prefix: &str| {
        let (label, enabled) = labels[i];
        enabled && label.trim_start().to_lowercase().starts_with(prefix)
    };

    let current = usize::try_from(current).ok().filter(|c| *c < n);

    // A growing prefix that still fits the current entry keeps it.
    if typed.chars().count() > 1 {
        if let Some(c) = current {
            if matches(c, &typed) {
                return Some(c);
            }
        }
    }

    // Search after the current entry (or from the top), wrapping round.
    let start = current.map_or(0, |c| c + 1);
    let search = |prefix: &str| {
        (0..n)
            .map(|k| (start + k) % n)
            .find(|&i| matches(i, prefix))
    };
    search(&typed).or_else(|| {
        if repeating {
            search(&first.to_string())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MENU: &[(&str, bool)] = &[
        ("View Data", true),
        ("Generate SELECT", true),
        ("Generate INSERT", true),
        ("Copy Name", true),
        ("Edit Table", true),
        ("Refresh", true),
        ("Drop Table", true),
    ];

    #[test]
    fn a_letter_jumps_to_the_first_entry_after_the_highlight() {
        assert_eq!(next_match(MENU, -1, "c"), Some(3));
        assert_eq!(next_match(MENU, 0, "e"), Some(4));
        assert_eq!(next_match(MENU, 0, "D"), Some(6), "case is ignored");
    }

    #[test]
    fn repeating_a_letter_cycles_and_wraps() {
        assert_eq!(next_match(MENU, -1, "g"), Some(1));
        assert_eq!(next_match(MENU, 1, "gg"), Some(2));
        assert_eq!(next_match(MENU, 2, "ggg"), Some(1), "wraps to the first G");
    }

    #[test]
    fn a_longer_prefix_narrows_without_leaving_a_match() {
        assert_eq!(next_match(MENU, -1, "g"), Some(1));
        assert_eq!(next_match(MENU, 1, "ge"), Some(1));
        assert_eq!(next_match(MENU, 1, "generate i"), Some(2));
        assert_eq!(next_match(MENU, 2, "generate s"), Some(1));
    }

    #[test]
    fn nothing_matching_leaves_the_highlight_alone() {
        assert_eq!(next_match(MENU, 3, "q"), None);
        assert_eq!(next_match(MENU, 3, ""), None);
        assert_eq!(next_match(&[], -1, "a"), None);
    }

    #[test]
    fn disabled_entries_are_skipped() {
        let menu = [("Drop Table", false), ("Duplicate", true)];
        assert_eq!(next_match(&menu, -1, "d"), Some(1));
        let menu = [("Drop Table", false)];
        assert_eq!(next_match(&menu, -1, "d"), None);
    }

    #[test]
    fn an_out_of_range_highlight_searches_from_the_top() {
        assert_eq!(next_match(MENU, 99, "v"), Some(0));
    }

    #[test]
    fn only_single_printable_characters_type_ahead() {
        assert!(is_typeahead_text("a"));
        assert!(is_typeahead_text("?"));
        assert!(is_typeahead_text("é"));
        assert!(!is_typeahead_text(""));
        assert!(!is_typeahead_text("ab"));
        assert!(!is_typeahead_text(" "), "Space activates, it does not type");
        assert!(!is_typeahead_text("\n"));
        assert!(!is_typeahead_text("\u{1b}"));
        assert!(!is_typeahead_text("\u{f700}"), "arrow keys");
    }
}
