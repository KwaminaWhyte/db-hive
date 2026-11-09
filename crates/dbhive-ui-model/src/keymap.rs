//! The live key bindings: the command registry's chords with the user's
//! rebinds from `settings.json` laid over them.
//!
//! [`COMMANDS`](crate::command_palette::COMMANDS) holds each command's
//! default chord. Settings > Keyboard Shortcuts stores rebinds as text
//! ("Ctrl+Shift+F", "Cmd+J", "?") in [`ShortcutsSettings`]. A [`Keymap`] is
//! the merge of the two, and it is what dispatch, the shortcuts help, the
//! palette's key caps and the menus all read, so a rebind changes every one of
//! them at once and they cannot disagree.
//!
//! Only some settings rows name a registry command (see [`command_for_field`]).
//! The others (Clear Editor, Format SQL, Save Snippet, Search) are editor
//! actions with no command in the registry; they are stored and shown in
//! Settings but this map does not dispatch them.
//!
//! A stored binding whose key the registry's [`KeyCode`] cannot express
//! ("F5", "Ctrl+/") leaves its command unbound rather than silently keeping
//! the old chord: what Settings shows is what the help shows. The Settings
//! controller warns when that happens ([`binding_problem`]).

use dbhive_core::models::settings::ShortcutsSettings;

use crate::command_palette::{command, Chord, CommandId, KeyCode, Scope, COMMANDS};
use crate::settings::{ShortcutField, EDITOR_SHORTCUTS, NAVIGATION_SHORTCUTS};

/// The registry command a settings row rebinds, if it rebinds one.
pub fn command_for_field(field: ShortcutField) -> Option<CommandId> {
    match field {
        ShortcutField::ExecuteQuery => Some(CommandId::RunQuery),
        ShortcutField::NewTab => Some(CommandId::NewTab),
        ShortcutField::CloseTab => Some(CommandId::CloseTab),
        ShortcutField::ToggleSidebar => Some(CommandId::ToggleSidebar),
        ShortcutField::OpenSettings => Some(CommandId::GoSettings),
        ShortcutField::ShowShortcuts => Some(CommandId::ShowShortcuts),
        ShortcutField::ClearEditor
        | ShortcutField::FormatSql
        | ShortcutField::SaveSnippet
        | ShortcutField::Search => None,
    }
}

/// Parse a stored binding ("Ctrl+Shift+F", "Cmd+Enter", "Option+K", "?")
/// into a chord, or `None` when its key is outside [`KeyCode`] or the text is
/// not a binding at all.
///
/// Ctrl, Cmd and Meta all mean the registry's "command" modifier, the same
/// rule [`crate::palette_session::chord_from_input`] applies to real key
/// presses: a stored "Ctrl+T" is Cmd+T on a Mac. Shift is implied by "?" and
/// ignored there, as it is when the key is pressed.
pub fn parse_binding(text: &str) -> Option<Chord> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut parts: Vec<&str> = text.split('+').collect();
    // "Ctrl++" splits into ["Ctrl", "", ""]: the bound key is "+" itself,
    // which the registry cannot express.
    if parts.len() > 1 && parts.last() == Some(&"") {
        return None;
    }
    let key_text = parts.pop()?.trim();
    let (mut command, mut shift, mut alt) = (false, false, false);
    for m in parts {
        match m.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "cmd" | "command" | "meta" | "super" => command = true,
            "shift" => shift = true,
            "alt" | "option" | "opt" => alt = true,
            _ => return None,
        }
    }
    let key = key_from_text(key_text)?;
    if key == KeyCode::Questionmark {
        shift = false;
    }
    Some(Chord {
        command,
        shift,
        alt,
        key,
    })
}

fn key_from_text(key: &str) -> Option<KeyCode> {
    let lower = key.to_ascii_lowercase();
    let named = match lower.as_str() {
        "enter" | "return" => Some(KeyCode::Enter),
        "escape" | "esc" => Some(KeyCode::Escape),
        "space" => Some(KeyCode::Space),
        "backspace" => Some(KeyCode::Backspace),
        "delete" | "del" => Some(KeyCode::Delete),
        "tab" => Some(KeyCode::Tab),
        "," | "comma" => Some(KeyCode::Comma),
        "?" | "questionmark" => Some(KeyCode::Questionmark),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    let mut chars = lower.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_lowercase() => Some(KeyCode::ALL[(c as u8 - b'a') as usize]),
        _ => None,
    }
}

/// Why a just-recorded binding will not fire, or `None` when it will. Shown
/// as a warning in Settings next to the duplicate warning.
pub fn binding_problem(field: ShortcutField, binding: &str) -> Option<String> {
    if binding.is_empty() {
        return None;
    }
    if command_for_field(field).is_none() {
        return Some(format!(
            "\"{}\" is saved, but {} has no keyboard shortcut in this version yet.",
            binding,
            field.label()
        ));
    }
    if parse_binding(binding).is_none() {
        return Some(format!(
            "\"{}\" is saved but will not fire: shortcuts here use a letter, Enter, Space, Tab, Backspace, Delete, Escape, comma or ?.",
            binding
        ));
    }
    None
}

/// Every command's live chord.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    /// One slot per entry of [`COMMANDS`], in the same order.
    chords: Vec<(CommandId, Option<Chord>)>,
    /// Commands whose chord came from the user rather than the registry.
    rebound: Vec<CommandId>,
}

impl Default for Keymap {
    /// The registry's own chords, no rebinds.
    fn default() -> Self {
        Self {
            chords: COMMANDS.iter().map(|c| (c.id, c.chord)).collect(),
            rebound: Vec::new(),
        }
    }
}

impl Keymap {
    /// The registry with the saved rebinds applied.
    pub fn from_settings(shortcuts: &ShortcutsSettings) -> Self {
        let mut map = Self::default();
        for field in EDITOR_SHORTCUTS.iter().chain(NAVIGATION_SHORTCUTS.iter()) {
            let Some(id) = command_for_field(*field) else {
                continue;
            };
            let stored = field.get(shortcuts);
            let chord = parse_binding(stored);
            if chord == command(id).chord {
                continue;
            }
            map.set(id, chord);
        }
        map
    }

    fn set(&mut self, id: CommandId, chord: Option<Chord>) {
        if let Some(slot) = self.chords.iter_mut().find(|(c, _)| *c == id) {
            slot.1 = chord;
        }
        if !self.rebound.contains(&id) {
            self.rebound.push(id);
        }
    }

    /// The chord that runs `id` now, if any.
    pub fn chord(&self, id: CommandId) -> Option<Chord> {
        self.chords
            .iter()
            .find(|(c, _)| *c == id)
            .and_then(|(_, chord)| *chord)
    }

    /// Whether the user changed `id`'s binding from the registry default.
    pub fn is_rebound(&self, id: CommandId) -> bool {
        self.rebound.contains(&id)
    }

    /// The command bound to `chord` on `screen`.
    ///
    /// A rebind wins over a registry default on the same chord: binding New
    /// Tab to Cmd+K means Cmd+K opens a tab, not the palette, because that is
    /// what the user asked for and what the help now says. Among equals
    /// (two rebinds on one chord, which Settings warns about but allows) the
    /// earlier command in registry order wins.
    pub fn command_for_chord(&self, chord: Chord, screen: Scope) -> Option<CommandId> {
        let live = |id: CommandId| {
            let scope = command(id).scope;
            scope == Scope::Global || scope == screen
        };
        let matches = || {
            self.chords
                .iter()
                .filter(move |(id, c)| *c == Some(chord) && live(*id))
                .map(|(id, _)| *id)
        };
        matches()
            .find(|id| self.is_rebound(*id))
            .or_else(|| matches().next())
    }

    /// A command whose chord is now taken by a rebind of another command live
    /// on the same screen, so it no longer fires from it there. The help drops
    /// it rather than advertising a key that does something else.
    ///
    /// A command that is not live on `screen` is never shadowed there: help
    /// lists every binding the app has, wherever it is read from.
    pub fn is_shadowed(&self, id: CommandId, screen: Scope) -> bool {
        let scope = command(id).scope;
        if scope != Scope::Global && scope != screen {
            return false;
        }
        match self.chord(id) {
            Some(chord) => self.command_for_chord(chord, screen) != Some(id),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> ShortcutsSettings {
        ShortcutsSettings::default()
    }

    #[test]
    fn stored_text_parses_to_the_chord_dispatch_compares_against() {
        assert_eq!(parse_binding("Ctrl+T"), Some(Chord::cmd(KeyCode::T)));
        assert_eq!(parse_binding("Cmd+T"), Some(Chord::cmd(KeyCode::T)));
        assert_eq!(parse_binding("cmd+t"), Some(Chord::cmd(KeyCode::T)));
        assert_eq!(
            parse_binding("Ctrl+Shift+F"),
            Some(Chord::cmd_shift(KeyCode::F))
        );
        assert_eq!(
            parse_binding("Ctrl+Enter"),
            Some(Chord::cmd(KeyCode::Enter))
        );
        assert_eq!(parse_binding("Ctrl+,"), Some(Chord::cmd(KeyCode::Comma)));
        assert_eq!(parse_binding("?"), Some(Chord::bare(KeyCode::Questionmark)));
        assert_eq!(
            parse_binding("Shift+?"),
            Some(Chord::bare(KeyCode::Questionmark))
        );
        assert_eq!(
            parse_binding("Option+K"),
            Some(Chord {
                command: false,
                shift: false,
                alt: true,
                key: KeyCode::K
            })
        );
    }

    #[test]
    fn keys_the_registry_cannot_express_do_not_parse() {
        for s in [
            "",
            "F5",
            "Ctrl+/",
            "Ctrl++",
            "Ctrl+ArrowUp",
            "Hyper+K",
            "Ctrl+5",
        ] {
            assert_eq!(parse_binding(s), None, "{s:?}");
        }
    }

    #[test]
    fn every_default_setting_agrees_with_the_registry() {
        // If these ever drift, a fresh install would rebind silently.
        let map = Keymap::from_settings(&defaults());
        assert_eq!(map, Keymap::default());
        for c in COMMANDS {
            assert_eq!(map.chord(c.id), c.chord, "{}", c.title);
            assert!(!map.is_rebound(c.id));
        }
    }

    #[test]
    fn a_rebind_moves_the_command_to_its_new_chord() {
        let mut s = defaults();
        s.new_tab = "Ctrl+J".into();
        let map = Keymap::from_settings(&s);
        assert_eq!(map.chord(CommandId::NewTab), Some(Chord::cmd(KeyCode::J)));
        assert!(map.is_rebound(CommandId::NewTab));
        assert_eq!(
            map.command_for_chord(Chord::cmd(KeyCode::J), Scope::Workspace),
            Some(CommandId::NewTab)
        );
        // The old chord is free again.
        assert_eq!(
            map.command_for_chord(Chord::cmd(KeyCode::T), Scope::Workspace),
            None
        );
        // Scope still applies to a rebound command.
        assert_eq!(
            map.command_for_chord(Chord::cmd(KeyCode::J), Scope::Connections),
            None
        );
    }

    #[test]
    fn clearing_a_binding_unbinds_the_command() {
        let mut s = defaults();
        s.execute_query = String::new();
        let map = Keymap::from_settings(&s);
        assert_eq!(map.chord(CommandId::RunQuery), None);
        assert_eq!(
            map.command_for_chord(Chord::cmd(KeyCode::Enter), Scope::Workspace),
            None
        );
    }

    #[test]
    fn an_unparseable_binding_unbinds_rather_than_keeping_the_old_chord() {
        let mut s = defaults();
        s.open_settings = "F5".into();
        let map = Keymap::from_settings(&s);
        assert_eq!(map.chord(CommandId::GoSettings), None);
        assert_eq!(
            map.command_for_chord(Chord::cmd(KeyCode::Comma), Scope::Connections),
            None
        );
    }

    #[test]
    fn a_workspace_binding_is_not_shadowed_on_the_connection_list() {
        // Help lists Run Query whichever screen it is read from; the check
        // must not call it shadowed just because it is dead there.
        let map = Keymap::default();
        assert!(!map.is_shadowed(CommandId::RunQuery, Scope::Connections));
        assert!(!map.is_shadowed(CommandId::RunQuery, Scope::Workspace));
        let mut s = defaults();
        s.new_tab = "Cmd+Enter".into();
        let map = Keymap::from_settings(&s);
        assert!(map.is_shadowed(CommandId::RunQuery, Scope::Workspace));
        assert!(!map.is_shadowed(CommandId::RunQuery, Scope::Connections));
    }

    #[test]
    fn a_rebind_onto_a_default_chord_wins_and_shadows_the_default() {
        let mut s = defaults();
        s.new_tab = "Cmd+K".into();
        let map = Keymap::from_settings(&s);
        assert_eq!(
            map.command_for_chord(Chord::cmd(KeyCode::K), Scope::Workspace),
            Some(CommandId::NewTab)
        );
        assert!(map.is_shadowed(CommandId::OpenCommandPalette, Scope::Workspace));
        assert!(!map.is_shadowed(CommandId::NewTab, Scope::Workspace));
        // On the connection list New Tab is not live, so Cmd+K is the
        // palette again.
        assert_eq!(
            map.command_for_chord(Chord::cmd(KeyCode::K), Scope::Connections),
            Some(CommandId::OpenCommandPalette)
        );
        assert!(!map.is_shadowed(CommandId::OpenCommandPalette, Scope::Connections));
    }

    #[test]
    fn editor_only_rows_never_touch_the_registry() {
        let mut s = defaults();
        s.clear_editor = "Ctrl+T".into();
        s.search = "Ctrl+N".into();
        let map = Keymap::from_settings(&s);
        assert_eq!(map, Keymap::default());
    }

    #[test]
    fn binding_problems_name_the_reason() {
        assert_eq!(binding_problem(ShortcutField::NewTab, "Ctrl+J"), None);
        assert_eq!(binding_problem(ShortcutField::NewTab, ""), None);
        assert!(binding_problem(ShortcutField::NewTab, "F5")
            .unwrap()
            .contains("will not fire"));
        assert!(binding_problem(ShortcutField::FormatSql, "Ctrl+Shift+F")
            .unwrap()
            .contains("Format SQL"));
    }

    #[test]
    fn every_mapped_field_names_a_command_that_has_a_registry_chord() {
        for f in EDITOR_SHORTCUTS.iter().chain(NAVIGATION_SHORTCUTS.iter()) {
            if let Some(id) = command_for_field(*f) {
                assert!(command(id).chord.is_some(), "{:?}", f);
            }
        }
    }
}
