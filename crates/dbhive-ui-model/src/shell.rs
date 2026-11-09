//! Window-level decisions with no toolkit in them: what to open at startup,
//! how to hand a link to the OS, how a menu spells a chord, and what a crash
//! report may say.
//!
//! Each is a pure function so it is tested here, and the Slint shell's
//! `app/shell.rs` only carries the result out.

use dbhive_core::models::settings::StartupBehavior;
use dbhive_core::models::ConnectionProfile;

use crate::command_palette::{Chord, KeyCode};

// ---------------------------------------------------------------------------
// Links
// ---------------------------------------------------------------------------

/// The project's home on GitHub.
pub const GITHUB_URL: &str = "https://github.com/KwaminaWhyte/db-hive";
/// The user guide, rendered by GitHub from `docs/user-guide.md`.
pub const DOCS_URL: &str = "https://github.com/KwaminaWhyte/db-hive/blob/main/docs/user-guide.md";

/// Only web links are handed to the OS opener: anything else (a `file:`
/// URL, a path, a string starting with `-` that the opener would read as a
/// flag) could make it run or reveal something the user did not ask for.
pub fn is_web_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://"))
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// The program and arguments that open `url` in the default browser on
/// `os` (`std::env::consts::OS`), or `None` when `url` is not a web link.
///
/// The same openers `result_export::reveal_command` and the updater use:
/// `open` on macOS, `explorer` on Windows (it hands a URL to the default
/// browser and, unlike `cmd /C start`, does not reinterpret `&` in a query
/// string), `xdg-open` elsewhere. The URL is one argument, never shell text.
pub fn open_url_command(url: &str, os: &str) -> Option<(String, Vec<String>)> {
    if !is_web_url(url) {
        return None;
    }
    let program = match os {
        "macos" => "open",
        "windows" => "explorer",
        _ => "xdg-open",
    };
    Some((program.to_string(), vec![url.to_string()]))
}

// ---------------------------------------------------------------------------
// Startup behaviour
// ---------------------------------------------------------------------------

/// What the app does once its window is up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupPlan {
    /// Stay on the connection list.
    ShowConnections,
    /// Connect to this saved connection and open its workspace, which lands
    /// on a query tab.
    Open { profile_id: String },
    /// The setting asked for a connection that cannot be found: stay on the
    /// list and say why.
    Explain(String),
}

/// The most recently used saved connection, by `last_connected_at`.
pub fn last_used(profiles: &[ConnectionProfile]) -> Option<&ConnectionProfile> {
    profiles
        .iter()
        .filter(|p| p.last_connected_at.is_some())
        .max_by_key(|p| p.last_connected_at)
}

/// Decide what startup does, from the saved settings and connections.
///
/// "Show Query Editor" opens the most recent connection: the workspace
/// already lands on a query tab, and there is no editor without a connection
/// in this shell. A first run with nothing used yet is not an error, so it
/// quietly shows the list; a default connection that was deleted is, so it
/// says so.
pub fn startup_plan(
    behavior: StartupBehavior,
    default_connection: Option<&str>,
    profiles: &[ConnectionProfile],
) -> StartupPlan {
    match behavior {
        StartupBehavior::ShowConnectionList => StartupPlan::ShowConnections,
        StartupBehavior::OpenLastConnection | StartupBehavior::ShowQueryEditor => {
            match last_used(profiles) {
                Some(p) => StartupPlan::Open {
                    profile_id: p.id.clone(),
                },
                None => StartupPlan::ShowConnections,
            }
        }
        StartupBehavior::OpenDefaultConnection => match default_connection {
            None | Some("") => StartupPlan::Explain(
                "No default connection is set. Choose one in Settings > General.".to_string(),
            ),
            Some(id) => match profiles.iter().find(|p| p.id == id) {
                Some(p) => StartupPlan::Open {
                    profile_id: p.id.clone(),
                },
                None => StartupPlan::Explain(
                    "The default connection no longer exists. Choose another in Settings > General."
                        .to_string(),
                ),
            },
        },
    }
}

/// Labels for the "Default connection" picker: "None" first, then every
/// saved connection by name. Index 0 means none.
pub fn default_connection_options(profiles: &[ConnectionProfile]) -> Vec<String> {
    let mut v = vec!["None".to_string()];
    v.extend(sorted(profiles).into_iter().map(|p| p.name.clone()));
    v
}

/// The picker index of `default_connection` (0 when none or not found).
pub fn default_connection_index(
    profiles: &[ConnectionProfile],
    default_connection: Option<&str>,
) -> usize {
    default_connection
        .and_then(|id| sorted(profiles).iter().position(|p| p.id == id))
        .map_or(0, |i| i + 1)
}

/// The connection id a picker index means (`None` for index 0).
pub fn default_connection_at(profiles: &[ConnectionProfile], index: usize) -> Option<String> {
    index
        .checked_sub(1)
        .and_then(|i| sorted(profiles).get(i).map(|p| p.id.clone()))
}

fn sorted(profiles: &[ConnectionProfile]) -> Vec<&ConnectionProfile> {
    let mut v: Vec<&ConnectionProfile> = profiles.iter().collect();
    v.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    v
}

// ---------------------------------------------------------------------------
// Menu shortcuts
// ---------------------------------------------------------------------------

/// A chord as the parts `slint::Keys::from_parts` takes (the `@keys` syntax),
/// so a menu item shows and answers to the live binding.
///
/// `Control` is Slint's portable "command" modifier: Cmd on macOS, Ctrl
/// elsewhere, the same rule the registry uses. Named keys use Slint's names;
/// letters are lower case. `QuestionMark` ignores Shift by itself (it is
/// shifted on most layouts), so Shift is never added to it.
pub fn menu_key_parts(chord: Chord) -> Vec<&'static str> {
    let mut parts = Vec::new();
    if chord.command {
        parts.push("Control");
    }
    if chord.alt {
        parts.push("Alt");
    }
    if chord.shift && chord.key != KeyCode::Questionmark {
        parts.push("Shift");
    }
    parts.push(match chord.key {
        KeyCode::Enter => "Return",
        KeyCode::Escape => "Escape",
        KeyCode::Space => "Space",
        KeyCode::Backspace => "Backspace",
        KeyCode::Delete => "Delete",
        KeyCode::Tab => "Tab",
        KeyCode::Comma => "Comma",
        KeyCode::Questionmark => "QuestionMark",
        letter => {
            const LOWER: [&str; 26] = [
                "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p",
                "q", "r", "s", "t", "u", "v", "w", "x", "y", "z",
            ];
            let index = KeyCode::ALL
                .iter()
                .position(|k| *k == letter)
                .expect("every KeyCode is in ALL");
            LOWER[index]
        }
    });
    parts
}

// ---------------------------------------------------------------------------
// Crash reports
// ---------------------------------------------------------------------------

/// Longest panic message kept, in characters.
const PANIC_CHARS: usize = 400;

/// Words after which a value is a secret.
const SECRET_KEYS: [&str; 7] = [
    "password", "passwd", "pwd", "secret", "token", "api_key", "apikey",
];

/// A panic message made safe to log or show: secrets redacted, length capped.
///
/// A panic message is code-defined text, but `expect` and `unwrap` on an
/// error can carry a connection string or a provider's response, so this
/// strips `user:password@` from URLs and the value after anything that looks
/// like a password or key (`password=`, `"api_key": "..."`, `token: ...`).
pub fn sanitize_panic_message(message: &str) -> String {
    let mut out = redact_url_credentials(message);
    for key in SECRET_KEYS {
        out = redact_after(&out, key);
    }
    let mut chars = out.chars();
    let mut capped: String = chars.by_ref().take(PANIC_CHARS).collect();
    if chars.next().is_some() {
        capped.push('\u{2026}');
    }
    capped
}

/// `scheme://user:pass@host` becomes `scheme://user:***@host`.
fn redact_url_credentials(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("://") {
        let (head, tail) = rest.split_at(i + 3);
        out.push_str(head);
        let end = tail
            .find(|c: char| c.is_whitespace() || c == '/' || c == '"' || c == '\'')
            .unwrap_or(tail.len());
        let authority = &tail[..end];
        match (authority.rfind('@'), authority.find(':')) {
            (Some(at), Some(colon)) if colon < at => {
                out.push_str(&authority[..colon]);
                out.push_str(":***");
                out.push_str(&authority[at..]);
            }
            _ => out.push_str(authority),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// Replace the value after `key` (case-insensitive) when it is followed by
/// `=` or `:` (optionally quoted), up to the next separator.
fn redact_after(s: &str, key: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let mut out = String::with_capacity(s.len());
    let mut cursor = 0usize;
    let mut search = 0usize;
    while let Some(found) = lower[search..].find(key) {
        let start = search + found;
        let after_key = start + key.len();
        let bytes = s.as_bytes();
        let mut j = after_key;
        // Skip a closing quote of a JSON key, then spaces.
        while j < bytes.len() && (bytes[j] == b'"' || bytes[j] == b'\'' || bytes[j] == b' ') {
            j += 1;
        }
        if j < bytes.len() && (bytes[j] == b'=' || bytes[j] == b':') {
            j += 1;
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'"' || bytes[j] == b'\'') {
                j += 1;
            }
            let value_start = j;
            while j < bytes.len()
                && !matches!(
                    bytes[j],
                    b' ' | b'"' | b'\'' | b',' | b';' | b'&' | b'}' | b')'
                )
                && !(bytes[j] as char).is_whitespace()
            {
                j += 1;
            }
            if j > value_start {
                out.push_str(&s[cursor..value_start]);
                out.push_str("***");
                cursor = j;
            }
        }
        search = after_key.max(j.min(s.len()));
        if search >= s.len() {
            break;
        }
    }
    out.push_str(&s[cursor..]);
    out
}

/// The toast shown when background work panicked. Says what happened and
/// what to do, never the raw payload (the log has the sanitized detail).
pub fn worker_panic_toast() -> &'static str {
    "Something went wrong in the background and that action was stopped. \
     The rest of DB Hive is fine; try again, and if it keeps happening, \
     report it with the log."
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str, name: &str, last: Option<i64>) -> ConnectionProfile {
        let mut p = ConnectionProfile::new(
            id.into(),
            name.into(),
            dbhive_core::models::DbDriver::Sqlite,
            String::new(),
            0,
            String::new(),
        );
        p.last_connected_at = last;
        p
    }

    // -- links ---------------------------------------------------------------

    #[test]
    fn urls_open_with_the_platform_opener_as_one_argument() {
        let url = "https://github.com/KwaminaWhyte/db-hive?a=1&b=2";
        assert_eq!(
            open_url_command(url, "macos"),
            Some(("open".into(), vec![url.into()]))
        );
        assert_eq!(
            open_url_command(url, "windows"),
            Some(("explorer".into(), vec![url.into()]))
        );
        assert_eq!(
            open_url_command(url, "linux"),
            Some(("xdg-open".into(), vec![url.into()]))
        );
        assert_eq!(open_url_command(url, "freebsd").unwrap().0, "xdg-open");
    }

    #[test]
    fn only_web_links_are_opened() {
        for bad in [
            "",
            "file:///etc/passwd",
            "/Applications/Calculator.app",
            "-a Calculator",
            "javascript:alert(1)",
            "https://example.com/a b",
            "https://example.com/\n--flag",
        ] {
            assert_eq!(open_url_command(bad, "macos"), None, "{bad:?}");
        }
        assert!(is_web_url(DOCS_URL));
        assert!(is_web_url(GITHUB_URL));
        assert!(is_web_url("HTTP://example.com"));
    }

    // -- startup -------------------------------------------------------------

    #[test]
    fn the_connection_list_is_the_default() {
        let ps = [profile("a", "A", Some(5))];
        assert_eq!(
            startup_plan(StartupBehavior::ShowConnectionList, Some("a"), &ps),
            StartupPlan::ShowConnections
        );
    }

    #[test]
    fn open_last_picks_the_most_recent_and_is_quiet_on_a_first_run() {
        let ps = [
            profile("a", "A", Some(5)),
            profile("b", "B", Some(9)),
            profile("c", "C", None),
        ];
        let want = StartupPlan::Open {
            profile_id: "b".into(),
        };
        assert_eq!(
            startup_plan(StartupBehavior::OpenLastConnection, None, &ps),
            want
        );
        assert_eq!(
            startup_plan(StartupBehavior::ShowQueryEditor, None, &ps),
            want
        );
        let fresh = [profile("c", "C", None)];
        assert_eq!(
            startup_plan(StartupBehavior::OpenLastConnection, None, &fresh),
            StartupPlan::ShowConnections
        );
        assert_eq!(
            startup_plan(StartupBehavior::OpenLastConnection, None, &[]),
            StartupPlan::ShowConnections
        );
    }

    #[test]
    fn open_default_needs_a_default_that_still_exists() {
        let ps = [profile("a", "A", None), profile("b", "B", None)];
        assert_eq!(
            startup_plan(StartupBehavior::OpenDefaultConnection, Some("b"), &ps),
            StartupPlan::Open {
                profile_id: "b".into()
            }
        );
        match startup_plan(StartupBehavior::OpenDefaultConnection, None, &ps) {
            StartupPlan::Explain(m) => assert!(m.contains("No default connection")),
            other => panic!("{other:?}"),
        }
        match startup_plan(StartupBehavior::OpenDefaultConnection, Some("gone"), &ps) {
            StartupPlan::Explain(m) => assert!(m.contains("no longer exists")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_default_connection_picker_round_trips() {
        let ps = [
            profile("z", "zeta", None),
            profile("a", "Alpha", None),
            profile("m", "mid", None),
        ];
        assert_eq!(
            default_connection_options(&ps),
            vec!["None", "Alpha", "mid", "zeta"]
        );
        assert_eq!(default_connection_index(&ps, None), 0);
        assert_eq!(default_connection_index(&ps, Some("gone")), 0);
        for (i, id) in [(1, "a"), (2, "m"), (3, "z")] {
            assert_eq!(default_connection_index(&ps, Some(id)), i);
            assert_eq!(default_connection_at(&ps, i).as_deref(), Some(id));
        }
        assert_eq!(default_connection_at(&ps, 0), None);
        assert_eq!(default_connection_at(&ps, 9), None);
    }

    // -- menu keys -----------------------------------------------------------

    #[test]
    fn menu_parts_spell_chords_the_way_keys_from_parts_reads_them() {
        assert_eq!(menu_key_parts(Chord::cmd(KeyCode::N)), vec!["Control", "n"]);
        assert_eq!(
            menu_key_parts(Chord::cmd_shift(KeyCode::H)),
            vec!["Control", "Shift", "h"]
        );
        assert_eq!(
            menu_key_parts(Chord::cmd(KeyCode::Comma)),
            vec!["Control", "Comma"]
        );
        assert_eq!(
            menu_key_parts(Chord::cmd(KeyCode::Enter)),
            vec!["Control", "Return"]
        );
        // Shift is implied by "?" and must not be added.
        let q = Chord {
            command: false,
            shift: true,
            alt: false,
            key: KeyCode::Questionmark,
        };
        assert_eq!(menu_key_parts(q), vec!["QuestionMark"]);
        let alt = Chord {
            command: true,
            shift: false,
            alt: true,
            key: KeyCode::Z,
        };
        assert_eq!(menu_key_parts(alt), vec!["Control", "Alt", "z"]);
    }

    #[test]
    fn every_letter_maps_to_its_own_lower_case_letter() {
        for (i, k) in KeyCode::ALL.iter().take(26).enumerate() {
            let parts = menu_key_parts(Chord::bare(*k));
            assert_eq!(parts, vec![((b'a' + i as u8) as char).to_string()]);
        }
    }

    // -- crash reports -------------------------------------------------------

    #[test]
    fn panic_messages_lose_their_secrets() {
        let m = sanitize_panic_message(
            "called `Result::unwrap()` on an `Err` value: connect postgres://app:hunter2@db:5432/x failed",
        );
        assert!(!m.contains("hunter2"), "{m}");
        assert!(m.contains("postgres://app:***@db:5432/x"), "{m}");

        let m = sanitize_panic_message("host=db password=hunter2 dbname=x");
        assert!(!m.contains("hunter2"), "{m}");
        assert!(m.contains("password=***"), "{m}");
        assert!(m.contains("dbname=x"), "{m}");

        let m = sanitize_panic_message(r#"bad body {"api_key": "sk-live-123", "model": "x"}"#);
        assert!(!m.contains("sk-live-123"), "{m}");
        assert!(m.contains("\"model\": \"x\""), "{m}");

        let m = sanitize_panic_message("Token: abc.def.ghi expired");
        assert!(!m.contains("abc.def.ghi"), "{m}");

        let m = sanitize_panic_message("PASSWORD='x y'");
        assert!(!m.contains("PASSWORD='x"), "{m}");
    }

    #[test]
    fn ordinary_panic_messages_pass_through() {
        let plain = "index out of bounds: the len is 3 but the index is 7";
        assert_eq!(sanitize_panic_message(plain), plain);
        assert_eq!(
            sanitize_panic_message("no password here, just words"),
            "no password here, just words"
        );
    }

    #[test]
    fn long_panic_messages_are_capped() {
        let long = "x".repeat(5000);
        let m = sanitize_panic_message(&long);
        assert_eq!(m.chars().count(), PANIC_CHARS + 1);
        assert!(m.ends_with('\u{2026}'));
    }
}
