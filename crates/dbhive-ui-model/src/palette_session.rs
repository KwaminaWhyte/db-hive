//! The command palette and shortcuts help as headless state, for the Slint
//! shell (and any later shell).
//!
//! [`command_palette`](crate::command_palette) owns the registry: which
//! commands exist, their chords, ranking and grouping. This module adds what
//! a shell needs on top of it and the egui shell kept inline in its draw code:
//!
//! - a stable text id per command ([`CommandId::slug`]) so a menu, the palette
//!   and a test can all name a command as a string;
//! - [`status`]: whether a command is offered at all in this shell, and if it
//!   is offered but cannot run right now, the reason (the "enabled() reason"
//!   of `docs/ui-guidelines.md` section 9);
//! - [`chord_from_input`] and [`command_for_key`]: raw key events to commands,
//!   including the rule that nothing fires under an open dialog;
//! - [`PaletteState`] and [`ShortcutsState`]: query, selection with
//!   wraparound, and the flat rows (headers included) a list draws;
//! - [`reveal_offset`]: the scroll position that keeps the selected row
//!   visible after an arrow key.
//!
//! No toolkit types. The shell converts keys in and rows out.

use crate::command_palette::{
    command, command_for_chord, help_groups, palette_groups, score, Chord, CommandId, Group,
    KeyCode, Scope, COMMAND_GLYPH,
};

// ─────────────────────────────────────────────────────────────────────────
// Ids as text
// ─────────────────────────────────────────────────────────────────────────

impl CommandId {
    /// Every variant. A test pins that this and `COMMANDS` agree, and `slug`
    /// is an exhaustive match, so adding a variant cannot go unnoticed here.
    pub const ALL: &'static [CommandId] = &[
        CommandId::GoConnections,
        CommandId::GoQueryEditor,
        CommandId::GoVisualQueryBuilder,
        CommandId::GoSchemaDesigner,
        CommandId::GoErDiagram,
        CommandId::GoTableInspector,
        CommandId::GoActivityMonitor,
        CommandId::GoHistory,
        CommandId::GoAiAssistant,
        CommandId::GoImportExport,
        CommandId::GoBackup,
        CommandId::GoMigrations,
        CommandId::GoPlugins,
        CommandId::GoSettings,
        CommandId::GoAbout,
        CommandId::NewConnection,
        CommandId::NewWindow,
        CommandId::NewTable,
        CommandId::RefreshMetadata,
        CommandId::ShowShortcuts,
        CommandId::OpenCommandPalette,
        CommandId::Connect,
        CommandId::Disconnect,
        CommandId::ConnectionsBack,
        CommandId::RunQuery,
        CommandId::NewTab,
        CommandId::CloseTab,
        CommandId::ToggleSidebar,
        CommandId::ThemeLight,
        CommandId::ThemeDark,
        CommandId::ThemeToggle,
        CommandId::WindowMinimize,
        CommandId::WindowMaximizeToggle,
        CommandId::WindowClose,
    ];

    /// The id as kebab-case text, for menus and tests.
    pub fn slug(self) -> &'static str {
        match self {
            CommandId::GoConnections => "go-connections",
            CommandId::GoQueryEditor => "go-query-editor",
            CommandId::GoVisualQueryBuilder => "go-visual-query-builder",
            CommandId::GoSchemaDesigner => "go-schema-designer",
            CommandId::GoErDiagram => "go-er-diagram",
            CommandId::GoTableInspector => "go-table-inspector",
            CommandId::GoActivityMonitor => "go-activity-monitor",
            CommandId::GoHistory => "go-history",
            CommandId::GoAiAssistant => "go-ai-assistant",
            CommandId::GoImportExport => "go-import-export",
            CommandId::GoBackup => "go-backup",
            CommandId::GoMigrations => "go-migrations",
            CommandId::GoPlugins => "go-plugins",
            CommandId::GoSettings => "go-settings",
            CommandId::GoAbout => "go-about",
            CommandId::NewConnection => "new-connection",
            CommandId::NewWindow => "new-window",
            CommandId::NewTable => "new-table",
            CommandId::RefreshMetadata => "refresh-metadata",
            CommandId::ShowShortcuts => "show-shortcuts",
            CommandId::OpenCommandPalette => "open-command-palette",
            CommandId::Connect => "connect",
            CommandId::Disconnect => "disconnect",
            CommandId::ConnectionsBack => "connections-back",
            CommandId::RunQuery => "run-query",
            CommandId::NewTab => "new-tab",
            CommandId::CloseTab => "close-tab",
            CommandId::ToggleSidebar => "toggle-sidebar",
            CommandId::ThemeLight => "theme-light",
            CommandId::ThemeDark => "theme-dark",
            CommandId::ThemeToggle => "theme-toggle",
            CommandId::WindowMinimize => "window-minimize",
            CommandId::WindowMaximizeToggle => "window-maximize-toggle",
            CommandId::WindowClose => "window-close",
        }
    }

    pub fn from_slug(slug: &str) -> Option<CommandId> {
        CommandId::ALL.iter().copied().find(|id| id.slug() == slug)
    }
}

// ─────────────────────────────────────────────────────────────────────────
// What each command can do right now
// ─────────────────────────────────────────────────────────────────────────

/// What the shell knows about the world, enough to decide whether a command
/// can run. Plain values, so tests build one directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandContext {
    /// `Connections` or `Workspace`; never `Global`.
    pub screen: Scope,
    pub dark: bool,
    pub tables_loading: bool,
    pub tab_count: usize,
    pub active_is_query: bool,
    pub running: bool,
    pub editor_empty: bool,
    /// A dialog (connection editor, confirmation, About) is open. Nothing
    /// runs from a menu or a key under it.
    pub dialog_open: bool,
}

impl CommandContext {
    /// The connection list, nothing connected.
    pub fn connections() -> Self {
        Self {
            screen: Scope::Connections,
            dark: true,
            tables_loading: false,
            tab_count: 0,
            active_is_query: false,
            running: false,
            editor_empty: true,
            dialog_open: false,
        }
    }

    /// A connected workspace with one empty query tab open.
    pub fn workspace() -> Self {
        Self {
            screen: Scope::Workspace,
            tab_count: 1,
            active_is_query: true,
            ..Self::connections()
        }
    }
}

/// Whether this shell implements a command at all, independent of context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// Implemented: listed in the palette and the help, bound to its chord.
    Live,
    /// Defined in the registry and deliberately not offered: shown greyed in
    /// the palette with the reason, absent from the shortcuts help, and its
    /// chord does nothing.
    Unsupported(&'static str),
    /// The screen behind it does not exist in this shell yet: not listed, not
    /// bound. Offering a row that can only say "not built" is clutter.
    Hidden,
}

pub fn support(id: CommandId) -> Support {
    use CommandId::*;
    match id {
        GoConnections | GoSettings | GoAbout | NewConnection | RefreshMetadata | ShowShortcuts
        | OpenCommandPalette | Disconnect | RunQuery | NewTab | CloseTab | ThemeLight
        | ThemeDark | ThemeToggle | GoImportExport | GoBackup | GoActivityMonitor
        | GoMigrations | GoVisualQueryBuilder | NewTable | GoErDiagram | GoSchemaDesigner
        | GoAiAssistant => Support::Live,

        NewWindow => Support::Unsupported("Multiple windows are not supported yet"),

        GoQueryEditor | GoTableInspector | GoHistory | GoPlugins
        | ToggleSidebar | Connect
        | ConnectionsBack | WindowMinimize | WindowMaximizeToggle | WindowClose => Support::Hidden,
    }
}

/// A command's state in one context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Enabled,
    /// Offered but cannot run now; the text says why and what to do.
    Disabled(&'static str),
    Unsupported(&'static str),
    Hidden,
}

impl Status {
    pub fn is_enabled(self) -> bool {
        self == Status::Enabled
    }

    /// Shown in the palette (greyed when not enabled).
    pub fn is_listed(self) -> bool {
        !matches!(self, Status::Hidden)
    }

    /// The reason a listed command cannot run, or "" when it can.
    pub fn reason(self) -> &'static str {
        match self {
            Status::Disabled(r) | Status::Unsupported(r) => r,
            _ => "",
        }
    }
}

pub fn status(id: CommandId, ctx: &CommandContext) -> Status {
    match support(id) {
        Support::Hidden => return Status::Hidden,
        Support::Unsupported(reason) => return Status::Unsupported(reason),
        Support::Live => {}
    }
    if ctx.dialog_open {
        return Status::Disabled("Close the open dialog first");
    }
    match command(id).scope {
        Scope::Workspace if ctx.screen != Scope::Workspace => {
            return Status::Disabled("Open a connection first");
        }
        Scope::Connections if ctx.screen != Scope::Connections => {
            return Status::Disabled("Only available on the connections screen");
        }
        _ => {}
    }
    match id {
        CommandId::GoConnections if ctx.screen == Scope::Connections => {
            Status::Disabled("Already on the connections screen")
        }
        CommandId::RefreshMetadata if ctx.tables_loading => {
            Status::Disabled("Tables are already loading")
        }
        CommandId::CloseTab if ctx.tab_count == 0 => Status::Disabled("No tab is open"),
        CommandId::RunQuery if !ctx.active_is_query => {
            Status::Disabled("Switch to a query tab first")
        }
        CommandId::RunQuery if ctx.running => Status::Disabled("A query is already running"),
        CommandId::RunQuery if ctx.editor_empty => Status::Disabled("Write some SQL first"),
        CommandId::ThemeLight if !ctx.dark => Status::Disabled("Already using the light theme"),
        CommandId::ThemeDark if ctx.dark => Status::Disabled("Already using the dark theme"),
        _ => Status::Enabled,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Keys
// ─────────────────────────────────────────────────────────────────────────

/// Turn one key event into a registry chord, if it is one the registry knows.
///
/// `text` is the event's text as the toolkit reports it. `command` is Ctrl or
/// Meta: Slint reports Cmd and Ctrl differently per platform, so either one
/// is the registry's "command" modifier (a bare Ctrl binding means Cmd on a
/// Mac, the same rule the React shell had).
///
/// `?` is produced with Shift on most layouts, so its Shift is implied by the
/// character and ignored; letters keep Shift because `Cmd+Shift+N` and
/// `Cmd+N` are different commands.
pub fn chord_from_input(
    text: &str,
    ctrl: bool,
    alt: bool,
    shift: bool,
    meta: bool,
) -> Option<Chord> {
    let mut chars = text.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let command = ctrl || meta;

    // Some platforms report Ctrl+K as the control character U+000B. Enter,
    // Tab, Backspace and Escape are control characters too and are keys in
    // their own right, so only the other ones are letters.
    let c = match c as u32 {
        n @ 1..=26 if command && !matches!(n, 8 | 9 | 10 | 13) => (b'a' + (n as u8 - 1)) as char,
        _ => c,
    };

    let (key, shift) = match c {
        '\n' | '\r' => (KeyCode::Enter, shift),
        '\u{1b}' => (KeyCode::Escape, shift),
        '\t' => (KeyCode::Tab, shift),
        '\u{8}' => (KeyCode::Backspace, shift),
        '\u{7f}' => (KeyCode::Delete, shift),
        ' ' => (KeyCode::Space, shift),
        ',' => (KeyCode::Comma, shift),
        '?' => (KeyCode::Questionmark, false),
        c if c.is_ascii_alphabetic() => (letter(c.to_ascii_lowercase())?, shift),
        _ => return None,
    };
    Some(Chord {
        command,
        shift,
        alt,
        key,
    })
}

/// `KeyCode::ALL` starts with A..=Z in order.
fn letter(c: char) -> Option<KeyCode> {
    let index = (c as u8).checked_sub(b'a')? as usize;
    (index < 26).then(|| KeyCode::ALL[index])
}

/// What currently owns the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    None,
    Palette,
    Shortcuts,
    /// Any other dialog: connection editor, confirmation, About.
    Dialog,
}

/// The command a key press runs, if any.
///
/// With nothing open, the registry decides, on the current screen, and only
/// for commands this shell implements (so `Cmd+B` does nothing until a
/// sidebar toggle exists, and the help does not list it either). Under the
/// palette only its own toggle key does anything; under any other dialog
/// nothing does, so `Cmd+N` cannot open a second editor over the first.
pub fn command_for_key(chord: Chord, overlay: Overlay, screen: Scope) -> Option<CommandId> {
    match overlay {
        Overlay::None => {
            command_for_chord(chord, screen).filter(|id| matches!(support(*id), Support::Live))
        }
        Overlay::Palette => (Some(CommandId::OpenCommandPalette)
            == command_for_chord(chord, screen))
        .then_some(CommandId::OpenCommandPalette),
        Overlay::Shortcuts | Overlay::Dialog => None,
    }
}

/// The caps for a chord: `["Shift", "⌘", "N"]` on macOS, `["Ctrl", "Shift",
/// "N"]` elsewhere. Same words as [`Chord::format`], one cap each.
pub fn chord_parts(chord: Chord, mac: bool) -> Vec<String> {
    let key = crate::command_palette::key_label(chord.key).to_string();
    let mut parts = Vec::new();
    if mac {
        if chord.alt {
            parts.push("Option".to_string());
        }
        if chord.shift {
            parts.push("Shift".to_string());
        }
        if chord.command {
            parts.push(COMMAND_GLYPH.to_string());
        }
    } else {
        if chord.command {
            parts.push("Ctrl".to_string());
        }
        if chord.shift {
            parts.push("Shift".to_string());
        }
        if chord.alt {
            parts.push("Alt".to_string());
        }
    }
    parts.push(key);
    parts
}

// ─────────────────────────────────────────────────────────────────────────
// Palette
// ─────────────────────────────────────────────────────────────────────────

/// Row heights and list height in logical pixels. The shell hands these to
/// the window so the list is drawn at exactly the size [`reveal_offset`]
/// assumes.
pub const ROW_HEIGHT: f32 = 36.0;
pub const HEADER_HEIGHT: f32 = 30.0;
pub const LIST_MAX_HEIGHT: f32 = 360.0;

/// One drawn row: a group header or a command.
#[derive(Debug, Clone, PartialEq)]
pub struct PaletteRow {
    pub header: bool,
    /// Header text for a header; the command title otherwise.
    pub title: String,
    /// Empty for a header.
    pub slug: &'static str,
    pub description: &'static str,
    /// Chord caps, empty when the command has no chord.
    pub keys: Vec<String>,
    pub enabled: bool,
    /// Why a listed command cannot run; empty when it can.
    pub reason: &'static str,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PaletteView {
    pub rows: Vec<PaletteRow>,
    /// How many command rows (not headers) there are.
    pub command_count: usize,
    /// Index into `rows` of the selected command, if any command is enabled.
    pub selected_row: Option<usize>,
}

impl PaletteView {
    /// Top edge and height of `rows[index]` in the list's content.
    pub fn row_span(&self, index: usize) -> (f32, f32) {
        let mut top = 0.0;
        for (i, r) in self.rows.iter().enumerate() {
            let h = if r.header { HEADER_HEIGHT } else { ROW_HEIGHT };
            if i == index {
                return (top, h);
            }
            top += h;
        }
        (top, 0.0)
    }

    pub fn content_height(&self) -> f32 {
        self.rows
            .iter()
            .map(|r| if r.header { HEADER_HEIGHT } else { ROW_HEIGHT })
            .sum()
    }

    /// The scroll position (a Slint `viewport-y`: zero or negative) that keeps
    /// the selected row on screen, starting from `current`. When the selected
    /// row is the first of its group its header is brought into view too, so
    /// wrapping around to the top shows the top.
    pub fn reveal_offset(&self, current: f32) -> f32 {
        let Some(sel) = self.selected_row else {
            return 0.0;
        };
        let (mut top, h) = self.row_span(sel);
        let bottom = top + h;
        if sel > 0 && self.rows[sel - 1].header {
            top -= HEADER_HEIGHT;
        }
        let viewport = LIST_MAX_HEIGHT.min(self.content_height());
        let max_scroll = (self.content_height() - viewport).max(0.0);
        let mut offset = -current;
        if top < offset {
            offset = top;
        } else if bottom > offset + viewport {
            offset = bottom - viewport;
        }
        -offset.clamp(0.0, max_scroll)
    }
}

/// The lowest score that counts as a match. The registry's ranking ends in a
/// fuzzy "letters in order" fallback (scores under 200) that lets "tab" find
/// "Go to About"; the palette's contract is substring search, so those are
/// dropped. Everything from a description match (400) up stays.
const MIN_SCORE: i32 = 300;

/// The palette: whether it is open, what was typed, and which enabled
/// command is highlighted.
#[derive(Debug, Default, Clone)]
pub struct PaletteState {
    open: bool,
    query: String,
    /// The command the user moved to (arrows or pointer). `None` means "the
    /// best match": typing resets to it, so Enter runs what the ranking
    /// thinks you meant even when a weaker match sorts higher on screen
    /// because its group comes first.
    chosen: Option<CommandId>,
}

impl PaletteState {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.chosen = None;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn set_query(&mut self, query: &str) {
        if self.query != query {
            self.query = query.to_string();
            self.chosen = None;
        }
    }

    /// Commands matching the query, in display order, with their status.
    fn entries(&self, ctx: &CommandContext) -> Vec<(Group, Vec<(CommandId, Status)>)> {
        palette_groups(&self.query, ctx.screen)
            .into_iter()
            .filter_map(|(group, items)| {
                let items: Vec<(CommandId, Status)> = items
                    .into_iter()
                    .filter(|c| score(c, &self.query).is_some_and(|s| s >= MIN_SCORE || s == 0))
                    .map(|c| (c.id, status(c.id, ctx)))
                    .filter(|(_, s)| s.is_listed())
                    .collect();
                (!items.is_empty()).then_some((group, items))
            })
            .collect()
    }

    fn enabled_ids(&self, ctx: &CommandContext) -> Vec<CommandId> {
        self.entries(ctx)
            .into_iter()
            .flat_map(|(_, items)| items)
            .filter(|(_, s)| s.is_enabled())
            .map(|(id, _)| id)
            .collect()
    }

    /// The best-ranked enabled command; earliest on screen wins a tie, so an
    /// empty query selects the first row.
    fn best(&self, ctx: &CommandContext) -> Option<CommandId> {
        let mut best: Option<(i32, CommandId)> = None;
        for id in self.enabled_ids(ctx) {
            let s = score(command(id), &self.query).unwrap_or(0);
            if best.is_none_or(|(b, _)| s > b) {
                best = Some((s, id));
            }
        }
        best.map(|(_, id)| id)
    }

    /// The command Enter would run.
    pub fn selected_id(&self, ctx: &CommandContext) -> Option<CommandId> {
        match self.chosen {
            Some(id) if self.enabled_ids(ctx).contains(&id) => Some(id),
            _ => self.best(ctx),
        }
    }

    /// Arrow keys: step through the enabled commands in the order they are
    /// drawn, wrapping at both ends.
    pub fn move_selection(&mut self, delta: i32, ctx: &CommandContext) {
        let ids = self.enabled_ids(ctx);
        if ids.is_empty() {
            self.chosen = None;
            return;
        }
        let current = self
            .selected_id(ctx)
            .and_then(|id| ids.iter().position(|x| *x == id))
            .unwrap_or(0) as i32;
        let next = (current + delta).rem_euclid(ids.len() as i32) as usize;
        self.chosen = Some(ids[next]);
    }

    /// Mouse hover: highlight the command in `row` if it can run.
    ///
    /// Returns whether the selection changed, so the shell can skip a redraw
    /// when the pointer moves within the row that is already selected.
    pub fn select_row(&mut self, row: usize, ctx: &CommandContext) -> bool {
        let Some(id) = self.view(ctx, false).rows.get(row).and_then(|r| {
            (!r.header && r.enabled)
                .then(|| CommandId::from_slug(r.slug))
                .flatten()
        }) else {
            return false;
        };
        if self.selected_id(ctx) == Some(id) {
            return false;
        }
        self.chosen = Some(id);
        true
    }

    /// The rows to draw. `mac` picks the chord spelling.
    pub fn view(&self, ctx: &CommandContext, mac: bool) -> PaletteView {
        let selected_id = self.selected_id(ctx);
        let mut rows = Vec::new();
        let mut command_count = 0;
        let mut selected_row = None;
        for (group, items) in self.entries(ctx) {
            rows.push(PaletteRow {
                header: true,
                title: group.title().to_string(),
                slug: "",
                description: "",
                keys: Vec::new(),
                enabled: false,
                reason: "",
                selected: false,
            });
            for (id, st) in items {
                let cmd = command(id);
                let selected = st.is_enabled() && Some(id) == selected_id;
                if selected {
                    selected_row = Some(rows.len());
                }
                command_count += 1;
                rows.push(PaletteRow {
                    header: false,
                    title: cmd.title.to_string(),
                    slug: id.slug(),
                    description: cmd.description,
                    keys: cmd.chord.map(|c| chord_parts(c, mac)).unwrap_or_default(),
                    enabled: st.is_enabled(),
                    reason: st.reason(),
                    selected,
                });
            }
        }
        PaletteView {
            rows,
            command_count,
            selected_row,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Shortcuts help
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct ShortcutRow {
    pub header: bool,
    pub title: String,
    pub description: &'static str,
    pub keys: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct ShortcutsState {
    open: bool,
    query: String,
}

impl ShortcutsState {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_string();
    }

    /// Every binding this shell actually listens for, grouped, filtered by
    /// the search text. Derived from the same registry [`command_for_key`]
    /// reads, minus commands the shell does not implement.
    pub fn view(&self, mac: bool) -> Vec<ShortcutRow> {
        let mut rows = Vec::new();
        for (group, items) in help_groups(&self.query) {
            let items: Vec<_> = items
                .into_iter()
                .filter(|c| matches!(support(c.id), Support::Live))
                .collect();
            if items.is_empty() {
                continue;
            }
            rows.push(ShortcutRow {
                header: true,
                title: group.title().to_string(),
                description: "",
                keys: Vec::new(),
            });
            for c in items {
                rows.push(ShortcutRow {
                    header: false,
                    title: c.title.to_string(),
                    description: c.description,
                    keys: chord_parts(c.chord.expect("help lists chorded commands"), mac),
                });
            }
        }
        rows
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_palette::COMMANDS;
    use std::collections::HashSet;

    fn titles(view: &PaletteView) -> Vec<String> {
        view.rows
            .iter()
            .filter(|r| !r.header)
            .map(|r| r.title.clone())
            .collect()
    }

    fn ws() -> CommandContext {
        CommandContext::workspace()
    }

    fn conns() -> CommandContext {
        CommandContext::connections()
    }

    // ── ids ──────────────────────────────────────────────────────────────

    #[test]
    fn all_lists_exactly_the_registry() {
        assert_eq!(CommandId::ALL.len(), COMMANDS.len());
        let all: HashSet<_> = CommandId::ALL.iter().collect();
        assert_eq!(all.len(), CommandId::ALL.len(), "duplicate in ALL");
        for c in COMMANDS {
            assert!(all.contains(&c.id), "{:?} missing from ALL", c.id);
        }
    }

    #[test]
    fn slugs_are_unique_and_round_trip() {
        let mut seen = HashSet::new();
        for id in CommandId::ALL {
            assert!(seen.insert(id.slug()), "{} is used twice", id.slug());
            assert_eq!(CommandId::from_slug(id.slug()), Some(*id));
        }
        assert_eq!(CommandId::from_slug("nope"), None);
        assert_eq!(CommandId::from_slug(""), None);
    }

    // ── status ───────────────────────────────────────────────────────────

    #[test]
    fn workspace_commands_say_why_they_are_off_on_the_connection_list() {
        assert_eq!(
            status(CommandId::NewTab, &conns()),
            Status::Disabled("Open a connection first")
        );
        assert_eq!(status(CommandId::NewTab, &ws()), Status::Enabled);
    }

    #[test]
    fn context_decides_close_run_refresh_and_theme() {
        let mut c = ws();
        c.tab_count = 0;
        assert_eq!(status(CommandId::CloseTab, &c).reason(), "No tab is open");
        c.tab_count = 1;
        assert!(status(CommandId::CloseTab, &c).is_enabled());

        // Empty editor, then a running query, then a table tab.
        assert_eq!(
            status(CommandId::RunQuery, &ws()).reason(),
            "Write some SQL first"
        );
        let mut c = ws();
        c.editor_empty = false;
        assert!(status(CommandId::RunQuery, &c).is_enabled());
        c.running = true;
        assert_eq!(
            status(CommandId::RunQuery, &c).reason(),
            "A query is already running"
        );
        c.running = false;
        c.active_is_query = false;
        assert_eq!(
            status(CommandId::RunQuery, &c).reason(),
            "Switch to a query tab first"
        );

        let mut c = ws();
        c.tables_loading = true;
        assert!(!status(CommandId::RefreshMetadata, &c).is_enabled());

        assert!(status(CommandId::ThemeLight, &ws()).is_enabled());
        assert!(!status(CommandId::ThemeDark, &ws()).is_enabled());
        assert!(status(CommandId::ThemeToggle, &ws()).is_enabled());
        assert!(!status(CommandId::GoConnections, &conns()).is_enabled());
        assert!(status(CommandId::GoConnections, &ws()).is_enabled());
    }

    #[test]
    fn nothing_runs_under_an_open_dialog() {
        let mut c = ws();
        c.dialog_open = true;
        for id in [
            CommandId::NewTab,
            CommandId::NewConnection,
            CommandId::ThemeToggle,
        ] {
            assert_eq!(
                status(id, &c).reason(),
                "Close the open dialog first",
                "{:?}",
                id
            );
        }
        // Still hidden, still unsupported: the reason that matters wins.
        assert_eq!(status(CommandId::GoPlugins, &c), Status::Hidden);
    }

    #[test]
    fn the_diagram_and_designer_are_live_in_a_workspace_and_refused_elsewhere() {
        for id in [CommandId::GoErDiagram, CommandId::GoSchemaDesigner] {
            assert_eq!(status(id, &ws()), Status::Enabled, "{:?}", id);
            assert_eq!(status(id, &conns()).reason(), "Open a connection first", "{:?}", id);
            let mut c = ws();
            c.dialog_open = true;
            assert_eq!(status(id, &c).reason(), "Close the open dialog first", "{:?}", id);
        }
        // Offered in the workspace palette, not on the connection list.
        let mut p = PaletteState::default();
        p.open();
        let titles = |ctx: &CommandContext| -> Vec<String> {
            p.view(ctx, false).rows.iter().map(|r| r.title.clone()).collect()
        };
        assert!(titles(&ws()).contains(&"Go to ER Diagram".to_string()));
        assert!(titles(&ws()).contains(&"Go to Schema Designer".to_string()));
        assert!(!titles(&conns()).iter().any(|x| x.contains("ER Diagram")));
    }

    #[test]
    fn unbuilt_screens_are_hidden_and_new_window_is_unsupported_with_a_reason() {
        for id in [
            CommandId::GoPlugins,
            CommandId::ToggleSidebar,
            CommandId::WindowClose,
        ] {
            assert_eq!(status(id, &ws()), Status::Hidden, "{:?}", id);
        }
        assert_eq!(
            status(CommandId::NewWindow, &ws()),
            Status::Unsupported("Multiple windows are not supported yet")
        );
    }

    #[test]
    fn the_ai_assistant_is_live_in_a_workspace_and_refused_elsewhere() {
        let id = CommandId::GoAiAssistant;
        assert_eq!(status(id, &ws()), Status::Enabled);
        assert_eq!(status(id, &conns()).reason(), "Open a connection first");
        let mut c = ws();
        c.dialog_open = true;
        assert_eq!(status(id, &c).reason(), "Close the open dialog first");
        // Listed in the workspace palette, not on the connection list.
        let mut p = PaletteState::default();
        p.open();
        let titles = |ctx: &CommandContext| -> Vec<String> {
            p.view(ctx, false).rows.iter().map(|r| r.title.clone()).collect()
        };
        assert!(titles(&ws()).contains(&"Go to AI Assistant".to_string()));
        assert!(!titles(&conns()).iter().any(|x| x.contains("AI Assistant")));
    }

    #[test]
    fn the_activity_monitor_and_migrations_open_from_either_screen_but_not_under_a_dialog() {
        for id in [CommandId::GoActivityMonitor, CommandId::GoMigrations] {
            assert_eq!(status(id, &conns()), Status::Enabled, "{:?}", id);
            assert_eq!(status(id, &ws()), Status::Enabled, "{:?}", id);
            let mut c = ws();
            c.dialog_open = true;
            assert_eq!(status(id, &c).reason(), "Close the open dialog first");
        }
    }

    // ── keys ─────────────────────────────────────────────────────────────

    #[test]
    fn command_k_in_every_spelling_a_platform_reports() {
        let want = Some(Chord::cmd(KeyCode::K));
        assert_eq!(chord_from_input("k", true, false, false, false), want);
        assert_eq!(chord_from_input("k", false, false, false, true), want);
        assert_eq!(chord_from_input("K", true, false, false, false), want);
        // The control character some platforms deliver for Ctrl+K.
        assert_eq!(chord_from_input("\u{b}", true, false, false, false), want);
    }

    #[test]
    fn shift_matters_for_letters_but_not_for_question_mark() {
        assert_eq!(
            chord_from_input("N", true, false, true, false),
            Some(Chord::cmd_shift(KeyCode::N))
        );
        assert_eq!(
            chord_from_input("n", true, false, false, false),
            Some(Chord::cmd(KeyCode::N))
        );
        assert_eq!(
            chord_from_input("?", false, false, true, false),
            Some(Chord::bare(KeyCode::Questionmark))
        );
        assert_eq!(
            chord_from_input("?", false, false, false, false),
            Some(Chord::bare(KeyCode::Questionmark))
        );
    }

    #[test]
    fn enter_and_other_control_characters_are_not_mistaken_for_letters() {
        // Slint sends Return as "\n"; with Ctrl held that is Ctrl+Enter, not Ctrl+J.
        assert_eq!(
            chord_from_input("\n", true, false, false, false),
            Some(Chord::cmd(KeyCode::Enter))
        );
        assert_eq!(
            chord_from_input("\u{1b}", false, false, false, false),
            Some(Chord::bare(KeyCode::Escape))
        );
        assert_eq!(
            chord_from_input("\t", true, false, false, false),
            Some(Chord::cmd(KeyCode::Tab))
        );
    }

    #[test]
    fn things_that_are_not_chords_are_none() {
        assert_eq!(chord_from_input("", false, false, false, false), None);
        assert_eq!(chord_from_input("ab", false, false, false, false), None);
        assert_eq!(chord_from_input("5", true, false, false, false), None);
        // A modifier key on its own (Slint sends these as text too).
        assert_eq!(chord_from_input("\u{11}", false, false, false, false), None);
    }

    #[test]
    fn every_registry_chord_survives_the_round_trip_from_its_own_input() {
        // The text a user would type for a chord, run back through the
        // converter, must yield the chord. Guards the letter table.
        for c in COMMANDS {
            let Some(chord) = c.chord else { continue };
            let text = match chord.key {
                KeyCode::Enter => "\n".to_string(),
                KeyCode::Escape => "\u{1b}".to_string(),
                KeyCode::Comma => ",".to_string(),
                KeyCode::Questionmark => "?".to_string(),
                other => crate::command_palette::key_label(other).to_lowercase(),
            };
            assert_eq!(
                chord_from_input(&text, chord.command, chord.alt, chord.shift, false),
                Some(chord),
                "{}",
                c.title
            );
        }
    }

    #[test]
    fn keys_run_live_commands_for_the_screen_and_nothing_else() {
        let k = Chord::cmd(KeyCode::K);
        assert_eq!(
            command_for_key(k, Overlay::None, Scope::Connections),
            Some(CommandId::OpenCommandPalette)
        );
        assert_eq!(
            command_for_key(
                Chord::bare(KeyCode::Questionmark),
                Overlay::None,
                Scope::Workspace
            ),
            Some(CommandId::ShowShortcuts)
        );
        // Workspace chords are dead on the connection list.
        assert_eq!(
            command_for_key(Chord::cmd(KeyCode::T), Overlay::None, Scope::Connections),
            None
        );
        assert_eq!(
            command_for_key(Chord::cmd(KeyCode::T), Overlay::None, Scope::Workspace),
            Some(CommandId::NewTab)
        );
        // Bound in the registry, not implemented here: no effect.
        assert_eq!(
            command_for_key(Chord::cmd(KeyCode::B), Overlay::None, Scope::Workspace),
            None
        );
        assert_eq!(
            command_for_key(
                Chord::cmd_shift(KeyCode::N),
                Overlay::None,
                Scope::Workspace
            ),
            None
        );
        assert_eq!(
            command_for_key(
                Chord::bare(KeyCode::Escape),
                Overlay::None,
                Scope::Connections
            ),
            None
        );
    }

    #[test]
    fn under_a_dialog_only_the_palette_toggle_works_and_only_under_the_palette() {
        let k = Chord::cmd(KeyCode::K);
        assert_eq!(
            command_for_key(k, Overlay::Palette, Scope::Workspace),
            Some(CommandId::OpenCommandPalette)
        );
        assert_eq!(
            command_for_key(Chord::cmd(KeyCode::N), Overlay::Palette, Scope::Workspace),
            None
        );
        for o in [Overlay::Shortcuts, Overlay::Dialog] {
            for c in COMMANDS.iter().filter_map(|c| c.chord) {
                assert_eq!(
                    command_for_key(c, o, Scope::Workspace),
                    None,
                    "{:?} {:?}",
                    o,
                    c
                );
            }
        }
    }

    #[test]
    fn chord_parts_match_the_formatted_chord() {
        let c = Chord::cmd_shift(KeyCode::N);
        assert_eq!(chord_parts(c, true), vec!["Shift", COMMAND_GLYPH, "N"]);
        assert_eq!(chord_parts(c, false), vec!["Ctrl", "Shift", "N"]);
        assert_eq!(
            chord_parts(Chord::bare(KeyCode::Questionmark), false),
            vec!["?"]
        );
        // Same words as Chord::format, just split.
        assert_eq!(chord_parts(c, false).join("+"), c.format(false));
    }

    // ── palette ──────────────────────────────────────────────────────────

    #[test]
    fn empty_query_lists_live_commands_under_headers_in_group_order() {
        let mut p = PaletteState::default();
        p.open();
        let v = p.view(&conns(), false);
        let headers: Vec<&str> = v
            .rows
            .iter()
            .filter(|r| r.header)
            .map(|r| r.title.as_str())
            .collect();
        assert_eq!(headers, vec!["Navigation", "Actions", "Theme"]);
        let t = titles(&v);
        assert!(t.contains(&"New Connection".to_string()));
        assert!(t.contains(&"Open Settings".to_string()));
        // Workspace-scoped commands are not offered on the connection list.
        assert!(!t.contains(&"New Tab".to_string()));
        // Hidden commands never appear.
        assert!(!t.iter().any(|x| x.contains("ER Diagram")));
        assert_eq!(v.command_count, t.len());
    }

    #[test]
    fn workspace_adds_its_groups() {
        let p = PaletteState::default();
        let v = p.view(&ws(), false);
        let headers: Vec<&str> = v
            .rows
            .iter()
            .filter(|r| r.header)
            .map(|r| r.title.as_str())
            .collect();
        assert_eq!(
            headers,
            vec![
                "Navigation",
                "Actions",
                "Connections",
                "Query Workspace",
                "Theme"
            ]
        );
    }

    #[test]
    fn filtering_is_substring_and_drops_empty_groups() {
        let mut p = PaletteState::default();
        p.open();
        p.set_query("tab");
        let v = p.view(&ws(), false);
        let t = titles(&v);
        assert!(t.contains(&"New Tab".to_string()));
        assert!(t.contains(&"Close Tab".to_string()));
        // "New Connection" matches too: its description says "database".
        assert!(!t.contains(&"Toggle Theme".to_string()));
        assert!(v.rows.iter().filter(|r| r.header).count() >= 1);

        p.set_query("zzzzzz");
        let v = p.view(&ws(), false);
        assert!(v.rows.is_empty());
        assert_eq!(v.command_count, 0);
        assert_eq!(v.selected_row, None);
        assert_eq!(p.selected_id(&ws()), None);
    }

    #[test]
    fn letters_in_order_are_not_a_match() {
        // The registry's fuzzy fallback would let "tab" find "Go to About"
        // (t..a..b..); the palette searches by substring.
        let mut p = PaletteState::default();
        p.set_query("tab");
        let t = titles(&p.view(&ws(), false));
        assert!(!t.contains(&"Go to About".to_string()), "{t:?}");
        assert!(t.contains(&"New Tab".to_string()));
        // A real substring in a description still counts.
        assert!(
            t.contains(&"Refresh Metadata".to_string()),
            "\"databases\" contains tab"
        );
    }

    #[test]
    fn enter_targets_the_best_match_even_when_a_weaker_group_is_drawn_first() {
        let mut p = PaletteState::default();
        p.open();
        p.set_query("tab");
        let ctx = ws();
        // Commands whose titles or descriptions merely contain "tab" ("New
        // Table", "Go to Schema Designer": "tables") are drawn first, in
        // earlier groups; "New Tab" is the real answer.
        let v = p.view(&ctx, false);
        let first_command = v.rows.iter().find(|r| !r.header).unwrap();
        assert_ne!(first_command.slug, "new-tab");
        assert_eq!(p.selected_id(&ctx), Some(CommandId::NewTab));
        assert!(v.rows.iter().find(|r| r.selected).unwrap().slug == "new-tab");

        // Arrows then walk the drawn order from where the selection is.
        p.move_selection(1, &ctx);
        assert_eq!(p.selected_id(&ctx), Some(CommandId::CloseTab));
        // Typing goes back to the best match.
        p.set_query("ta");
        assert_eq!(p.selected_id(&ctx), Some(CommandId::NewTab));
    }

    #[test]
    fn the_first_enabled_command_is_selected_and_typing_resets_it() {
        let mut p = PaletteState::default();
        p.open();
        let v = p.view(&conns(), false);
        let first = v.rows.iter().position(|r| !r.header && r.enabled).unwrap();
        assert_eq!(v.selected_row, Some(first));
        assert_eq!(v.rows.iter().filter(|r| r.selected).count(), 1);

        p.move_selection(2, &conns());
        assert_ne!(p.view(&conns(), false).selected_row, Some(first));
        p.set_query("theme");
        assert_eq!(
            p.view(&conns(), false).selected_row,
            p.view(&conns(), false)
                .rows
                .iter()
                .position(|r| !r.header && r.enabled)
        );
    }

    #[test]
    fn arrows_wrap_in_both_directions() {
        let mut p = PaletteState::default();
        p.open();
        let ctx = conns();
        let ids: Vec<CommandId> = p.enabled_ids(&ctx);
        let n = ids.len() as i32;
        assert!(n > 3);

        assert_eq!(p.selected_id(&ctx), Some(ids[0]));
        p.move_selection(-1, &ctx);
        assert_eq!(
            p.selected_id(&ctx),
            Some(ids[ids.len() - 1]),
            "up from first wraps to last"
        );
        p.move_selection(1, &ctx);
        assert_eq!(
            p.selected_id(&ctx),
            Some(ids[0]),
            "down from last wraps to first"
        );
        for _ in 0..n {
            p.move_selection(1, &ctx);
        }
        assert_eq!(
            p.selected_id(&ctx),
            Some(ids[0]),
            "a full lap returns to the start"
        );
    }

    #[test]
    fn arrows_skip_disabled_rows_but_the_rows_stay_visible_with_their_reason() {
        let mut p = PaletteState::default();
        p.open();
        p.set_query("theme");
        let ctx = ws(); // dark: "Switch to Dark Theme" is disabled
        let v = p.view(&ctx, false);
        let dark = v.rows.iter().find(|r| r.slug == "theme-dark").unwrap();
        assert!(!dark.enabled);
        assert_eq!(dark.reason, "Already using the dark theme");
        let enabled: Vec<&str> = v
            .rows
            .iter()
            .filter(|r| r.enabled)
            .map(|r| r.slug)
            .collect();
        assert!(!enabled.contains(&"theme-dark"));
        for _ in 0..6 {
            p.move_selection(1, &ctx);
            assert_ne!(p.selected_id(&ctx), Some(CommandId::ThemeDark));
        }
    }

    #[test]
    fn all_disabled_means_nothing_is_selected_and_enter_has_no_target() {
        let mut p = PaletteState::default();
        p.open();
        p.set_query("new window");
        let v = p.view(&ws(), false);
        let row = v
            .rows
            .iter()
            .find(|r| r.slug == "new-window")
            .expect("listed, greyed");
        assert!(!row.enabled);
        assert_eq!(row.reason, "Multiple windows are not supported yet");
        assert_eq!(v.selected_row, None);
        assert_eq!(p.selected_id(&ws()), None);
        p.move_selection(1, &ws());
        assert_eq!(p.selected_id(&ws()), None);
    }

    #[test]
    fn hover_selects_an_enabled_row_and_ignores_headers_and_disabled_rows() {
        let mut p = PaletteState::default();
        p.open();
        let ctx = ws();
        let v = p.view(&ctx, false);
        let target = v.rows.iter().position(|r| r.slug == "close-tab").unwrap();
        assert!(p.select_row(target, &ctx));
        assert_eq!(p.selected_id(&ctx), Some(CommandId::CloseTab));
        assert!(
            !p.select_row(target, &ctx),
            "same row again changes nothing"
        );

        p.select_row(0, &ctx); // a header
        assert_eq!(p.selected_id(&ctx), Some(CommandId::CloseTab));

        let disabled = v.rows.iter().position(|r| r.slug == "theme-dark").unwrap();
        p.select_row(disabled, &ctx);
        assert_eq!(p.selected_id(&ctx), Some(CommandId::CloseTab));
        p.select_row(9999, &ctx);
        assert_eq!(p.selected_id(&ctx), Some(CommandId::CloseTab));
    }

    #[test]
    fn rows_carry_the_chord_caps() {
        let p = PaletteState::default();
        let v = p.view(&ws(), false);
        let new_tab = v.rows.iter().find(|r| r.slug == "new-tab").unwrap();
        assert_eq!(new_tab.keys, vec!["Ctrl", "T"]);
        let v = p.view(&ws(), true);
        let new_tab = v.rows.iter().find(|r| r.slug == "new-tab").unwrap();
        assert_eq!(new_tab.keys, vec![COMMAND_GLYPH, "T"]);
        let theme = v.rows.iter().find(|r| r.slug == "theme-toggle").unwrap();
        assert!(theme.keys.is_empty());
    }

    // ── scrolling ────────────────────────────────────────────────────────

    #[test]
    fn short_lists_never_scroll() {
        let mut p = PaletteState::default();
        p.set_query("settings");
        let v = p.view(&conns(), false);
        assert!(v.content_height() <= LIST_MAX_HEIGHT);
        assert_eq!(v.reveal_offset(0.0), 0.0);
        assert_eq!(v.reveal_offset(-50.0), 0.0);
    }

    #[test]
    fn arrowing_to_the_last_row_scrolls_it_into_view_and_wrapping_scrolls_back() {
        let mut p = PaletteState::default();
        p.open();
        let ctx = ws();
        let v = p.view(&ctx, false);
        assert!(
            v.content_height() > LIST_MAX_HEIGHT,
            "need a list that scrolls"
        );

        // Up from the first row wraps to the last one.
        p.move_selection(-1, &ctx);
        let v = p.view(&ctx, false);
        let off = v.reveal_offset(0.0);
        let (top, h) = v.row_span(v.selected_row.unwrap());
        assert!(off < 0.0);
        assert!(
            top + h <= -off + LIST_MAX_HEIGHT + 0.01,
            "selected row is below the viewport"
        );
        assert!(top >= -off - 0.01, "selected row is above the viewport");
        assert!(
            -off <= v.content_height() - LIST_MAX_HEIGHT + 0.01,
            "scrolled past the end"
        );

        // Down wraps to the first row: back to the very top, header included.
        p.move_selection(1, &ctx);
        let v = p.view(&ctx, false);
        assert_eq!(v.reveal_offset(off), 0.0);
    }

    #[test]
    fn stepping_down_scrolls_only_when_the_row_leaves_the_viewport() {
        let mut p = PaletteState::default();
        p.open();
        let ctx = ws();
        let mut offset = 0.0f32;
        let n = p.enabled_ids(&ctx).len();
        for _ in 0..n - 1 {
            p.move_selection(1, &ctx);
            let v = p.view(&ctx, false);
            let next = v.reveal_offset(offset);
            assert!(next <= offset + 0.01, "moving down never scrolls up");
            let (top, h) = v.row_span(v.selected_row.unwrap());
            assert!(top >= -next - 0.01 && top + h <= -next + LIST_MAX_HEIGHT + 0.01);
            offset = next;
        }
    }

    // ── shortcuts help ───────────────────────────────────────────────────

    #[test]
    fn help_lists_only_bindings_that_fire() {
        let mut s = ShortcutsState::default();
        s.open();
        let rows = s.view(false);
        let titles: Vec<&str> = rows
            .iter()
            .filter(|r| !r.header)
            .map(|r| r.title.as_str())
            .collect();
        for t in [
            "New Connection",
            "Command Palette",
            "Keyboard Shortcuts",
            "Run Query",
            "New Tab",
            "Close Tab",
            "Open Settings",
        ] {
            assert!(titles.contains(&t), "{t} missing");
        }
        // Registered but not implemented here.
        assert!(!titles.contains(&"Toggle Sidebar"));
        assert!(!titles.contains(&"New Window"));
        assert!(!titles.contains(&"Back"));

        // Every listed row's caps resolve back to a command that fires.
        for c in COMMANDS.iter().filter(|c| titles.contains(&c.title)) {
            let chord = c.chord.unwrap();
            assert!(
                COMMANDS
                    .iter()
                    .any(|d| d.chord == Some(chord) && matches!(support(d.id), Support::Live)),
                "{}",
                c.title
            );
        }
    }

    #[test]
    fn help_has_a_header_before_each_group_and_caps_on_every_row() {
        let rows = ShortcutsState::default().view(false);
        assert!(rows[0].header);
        for (i, r) in rows.iter().enumerate() {
            if r.header {
                assert!(
                    i + 1 < rows.len() && !rows[i + 1].header,
                    "empty group {}",
                    r.title
                );
                assert!(r.keys.is_empty());
            } else {
                assert!(!r.keys.is_empty(), "{} has no caps", r.title);
            }
        }
    }

    #[test]
    fn help_search_filters_and_can_come_up_empty() {
        let mut s = ShortcutsState::default();
        s.set_query("tab");
        let rows = s.view(false);
        let t: Vec<&str> = rows
            .iter()
            .filter(|r| !r.header)
            .map(|r| r.title.as_str())
            .collect();
        assert!(t.contains(&"New Tab") && t.contains(&"Close Tab"));
        assert!(!t.contains(&"Open Settings"));

        s.set_query("zzzz");
        assert!(s.view(false).is_empty());
    }

    #[test]
    fn help_follows_the_platform_spelling() {
        let s = ShortcutsState::default();
        let mac = s.view(true);
        let other = s.view(false);
        let find = |rows: &[ShortcutRow]| {
            rows.iter()
                .find(|r| r.title == "New Tab")
                .unwrap()
                .keys
                .clone()
        };
        assert_eq!(find(&mac), vec![COMMAND_GLYPH, "T"]);
        assert_eq!(find(&other), vec!["Ctrl", "T"]);
    }

    #[test]
    fn open_and_close_reset_what_they_should() {
        let mut p = PaletteState::default();
        p.open();
        p.set_query("abc");
        p.close();
        assert!(!p.is_open());
        p.open();
        assert!(p.is_open());
        assert_eq!(p.query(), "");

        let mut s = ShortcutsState::default();
        s.open();
        s.set_query("abc");
        s.close();
        s.open();
        assert_eq!(s.query(), "");
        assert!(s.is_open());
    }
}
