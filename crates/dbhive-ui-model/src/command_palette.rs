//! Command palette and the keyboard shortcut registry, lifted from the egui
//! `command_palette` screen. See lib.rs.
//!
//! Parity target: the React `CommandPalette` (2.16) plus `shortcutRegistry` /
//! `KeyboardShortcutsModal` (2.17).
//!
//! The React shell kept three separate lists: the palette's 16 commands, the
//! 11-entry shortcut registry, and the `useKeyboardShortcuts` call sites that
//! actually bound the keys. They drifted, which is how the registry ended up
//! documenting bindings that no longer existed. Here there is exactly one
//! table, [`COMMANDS`]: the palette lists from it, the help modal lists from
//! it, and shortcut dispatch matches against it. A binding that is not in the
//! table cannot fire, and a binding in the table cannot be missing from the
//! help modal.
//!
//! Keys are [`KeyCode`], not a toolkit type; each shell converts at the point
//! where it reads input.

/// Every action reachable from the palette or a keyboard shortcut.
///
/// The variants are the wiring contract: `show` returns
/// `PaletteAction::Run(id)` and the caller decides what each one means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandId {
    // Navigation
    GoConnections,
    GoQueryEditor,
    GoVisualQueryBuilder,
    GoSchemaDesigner,
    GoErDiagram,
    GoTableInspector,
    GoActivityMonitor,
    GoHistory,
    GoAiAssistant,
    GoImportExport,
    GoBackup,
    GoMigrations,
    GoPlugins,
    GoSettings,
    GoAbout,

    // Actions
    NewConnection,
    NewWindow,
    NewTable,
    RefreshMetadata,
    ShowShortcuts,
    OpenCommandPalette,

    // Connections
    Connect,
    Disconnect,
    ConnectionsBack,

    // Query workspace
    RunQuery,
    NewTab,
    CloseTab,
    ToggleSidebar,

    // Theme
    ThemeLight,
    ThemeDark,
    ThemeToggle,

    // Window
    WindowMinimize,
    WindowMaximizeToggle,
    WindowClose,
}

/// What the caller must do after a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteAction {
    None,
    Run(CommandId),
}

/// Palette section header, and the help modal's category. One enum for both:
/// two would drift the same way the React lists did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    Navigation,
    Actions,
    Connections,
    Query,
    Theme,
    Window,
}

impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Group::Navigation => "Navigation",
            Group::Actions => "Actions",
            Group::Connections => "Connections",
            Group::Query => "Query Workspace",
            Group::Theme => "Theme",
            Group::Window => "Window",
        }
    }
}

/// Display order for both the palette and the help modal.
pub const GROUP_ORDER: [Group; 6] = [
    Group::Navigation,
    Group::Actions,
    Group::Connections,
    Group::Query,
    Group::Theme,
    Group::Window,
];

/// Where a shortcut is live.
///
/// `Ctrl+Enter` must not run a query from the connection list, and `Escape`
/// must only mean "back" on the connection list. The React shell got this by
/// binding keys inside the route component; here the scope is data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Active on every screen.
    Global,
    /// Active only on the saved-connections screen.
    Connections,
    /// Active only in the connected workspace.
    Workspace,
}

/// A key, independent of any UI toolkit.
///
/// Covers the keys the registry binds (plus the few named keys that have a
/// dedicated label). Each shell converts its own key type to and from this;
/// the egui shell's conversion lives in `screens::command_palette`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Comma,
    Questionmark,
    Enter,
    Escape,
    Space,
    Backspace,
    Delete,
    Tab,
}

impl KeyCode {
    /// Every variant, so shells can prove their conversion is total.
    pub const ALL: &'static [KeyCode] = &[
        KeyCode::A,
        KeyCode::B,
        KeyCode::C,
        KeyCode::D,
        KeyCode::E,
        KeyCode::F,
        KeyCode::G,
        KeyCode::H,
        KeyCode::I,
        KeyCode::J,
        KeyCode::K,
        KeyCode::L,
        KeyCode::M,
        KeyCode::N,
        KeyCode::O,
        KeyCode::P,
        KeyCode::Q,
        KeyCode::R,
        KeyCode::S,
        KeyCode::T,
        KeyCode::U,
        KeyCode::V,
        KeyCode::W,
        KeyCode::X,
        KeyCode::Y,
        KeyCode::Z,
        KeyCode::Comma,
        KeyCode::Questionmark,
        KeyCode::Enter,
        KeyCode::Escape,
        KeyCode::Space,
        KeyCode::Backspace,
        KeyCode::Delete,
        KeyCode::Tab,
    ];
}

/// The macOS Command symbol. Must match the egui shell's
/// `theme::glyph::COMMAND`; a test there pins it.
pub const COMMAND_GLYPH: &str = "\u{2318}";

/// A key chord.
///
/// `command` means Cmd on macOS, Ctrl everywhere else (egui's
/// `Modifiers::COMMAND`). That is the same "a bare Ctrl binding means Cmd on
/// a Mac" rule the React `parseShortcut` hand-rolled; the shell resolves it
/// at the input layer so it cannot disagree with what the OS delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: KeyCode,
}

impl Chord {
    pub const fn bare(key: KeyCode) -> Self {
        Self {
            command: false,
            shift: false,
            alt: false,
            key,
        }
    }

    pub const fn cmd(key: KeyCode) -> Self {
        Self {
            command: true,
            shift: false,
            alt: false,
            key,
        }
    }

    pub const fn cmd_shift(key: KeyCode) -> Self {
        Self {
            command: true,
            shift: true,
            alt: false,
            key,
        }
    }

    /// How specific this chord is, used to order dispatch.
    ///
    /// egui's `matches_logically` deliberately ignores *extra* Shift and Alt,
    /// so `Cmd+N` also matches a `Cmd+Shift+N` press. The more-modified chord
    /// therefore has to be tested first or "New Window" would never fire.
    fn specificity(&self) -> u8 {
        self.command as u8 + self.shift as u8 + self.alt as u8
    }

    /// Format for display: `Shift ⌘N` on macOS, `Ctrl+Shift+N` elsewhere.
    ///
    /// Only the Command symbol is used. The bundled fonts carry no Shift,
    /// Option or Control symbols, and a missing glyph renders as an empty box
    /// rather than failing, so those modifiers are written as words. See the
    /// egui shell's `theme::glyph`.
    ///
    /// `mac` is a parameter rather than a `cfg!` so the formatting is
    /// testable for both platforms from either.
    pub fn format(&self, mac: bool) -> String {
        let key = key_label(self.key);
        if mac {
            // Apple's order is Control, Option, Shift, Command, key.
            let mut s = String::new();
            if self.alt {
                s.push_str("Option ");
            }
            if self.shift {
                s.push_str("Shift ");
            }
            if self.command {
                s.push_str(COMMAND_GLYPH);
            }
            s.push_str(key);
            s
        } else {
            let mut parts: Vec<&str> = Vec::new();
            if self.command {
                parts.push("Ctrl");
            }
            if self.shift {
                parts.push("Shift");
            }
            if self.alt {
                parts.push("Alt");
            }
            parts.push(key);
            parts.join("+")
        }
    }

    /// Format for the platform this binary was built for.
    pub fn display(&self) -> String {
        self.format(IS_MAC)
    }
}

/// True when built for macOS. The React shell sniffed the user agent; a
/// native binary knows at compile time.
pub const IS_MAC: bool = cfg!(target_os = "macos");

/// Human label for a key, matching the React `KEY_DISPLAY_NAMES` map where
/// one existed. Letters are the bare uppercase letter, and punctuation is
/// its symbol, exactly as egui's `Key::symbol_or_name` spelled them.
pub fn key_label(key: KeyCode) -> &'static str {
    match key {
        KeyCode::Escape => "Esc",
        KeyCode::Enter => "Enter",
        KeyCode::Space => "Space",
        KeyCode::Backspace => "Backspace",
        KeyCode::Delete => "Delete",
        KeyCode::Tab => "Tab",
        KeyCode::Comma => ",",
        KeyCode::Questionmark => "?",
        KeyCode::A => "A",
        KeyCode::B => "B",
        KeyCode::C => "C",
        KeyCode::D => "D",
        KeyCode::E => "E",
        KeyCode::F => "F",
        KeyCode::G => "G",
        KeyCode::H => "H",
        KeyCode::I => "I",
        KeyCode::J => "J",
        KeyCode::K => "K",
        KeyCode::L => "L",
        KeyCode::M => "M",
        KeyCode::N => "N",
        KeyCode::O => "O",
        KeyCode::P => "P",
        KeyCode::Q => "Q",
        KeyCode::R => "R",
        KeyCode::S => "S",
        KeyCode::T => "T",
        KeyCode::U => "U",
        KeyCode::V => "V",
        KeyCode::W => "W",
        KeyCode::X => "X",
        KeyCode::Y => "Y",
        KeyCode::Z => "Z",
    }
}

/// One entry in the single source of truth.
pub struct Command {
    pub id: CommandId,
    pub title: &'static str,
    pub description: &'static str,
    pub group: Group,
    pub chord: Option<Chord>,
    pub scope: Scope,
    /// Whether the command is offered in the palette. `Escape` ➡ back is a
    /// real binding worth documenting but is meaningless as a menu entry.
    pub in_palette: bool,
}

const fn cmd(
    id: CommandId,
    title: &'static str,
    description: &'static str,
    group: Group,
) -> Command {
    Command {
        id,
        title,
        description,
        group,
        chord: None,
        scope: Scope::Global,
        in_palette: true,
    }
}

const fn with_chord(mut c: Command, chord: Chord) -> Command {
    c.chord = Some(chord);
    c
}

const fn scoped(mut c: Command, scope: Scope) -> Command {
    c.scope = scope;
    c
}

const fn hidden(mut c: Command) -> Command {
    c.in_palette = false;
    c
}

/// The registry. Palette, help modal and shortcut dispatch all read this.
pub const COMMANDS: &[Command] = &[
    // ── Navigation ───────────────────────────────────────────────────────
    cmd(
        CommandId::GoConnections,
        "Go to Connections",
        "Saved connection manager",
        Group::Navigation,
    ),
    cmd(
        CommandId::GoQueryEditor,
        "Go to SQL Editor",
        "Write and execute queries",
        Group::Navigation,
    ),
    scoped(
        cmd(
            CommandId::GoVisualQueryBuilder,
            "Go to Visual Query Builder",
            "Build queries without writing SQL",
            Group::Navigation,
        ),
        Scope::Workspace,
    ),
    cmd(
        CommandId::GoSchemaDesigner,
        "Go to Schema Designer",
        "Design tables and relationships visually",
        Group::Navigation,
    ),
    cmd(
        CommandId::GoErDiagram,
        "Go to ER Diagram",
        "Entity-relationship diagram for the current schema",
        Group::Navigation,
    ),
    cmd(
        CommandId::GoTableInspector,
        "Go to Table Inspector",
        "Browse and edit table data",
        Group::Navigation,
    ),
    cmd(
        CommandId::GoActivityMonitor,
        "Go to Activity Monitor",
        "Server stats and running processes",
        Group::Navigation,
    ),
    cmd(
        CommandId::GoHistory,
        "Go to Query History",
        "Previously executed queries",
        Group::Navigation,
    ),
    cmd(
        CommandId::GoAiAssistant,
        "Go to AI Assistant",
        "Generate and explain SQL",
        Group::Navigation,
    ),
    cmd(
        CommandId::GoImportExport,
        "Go to Import / Export",
        "Move data in and out of a table",
        Group::Navigation,
    ),
    cmd(
        CommandId::GoBackup,
        "Go to Backup Manager",
        "Create and restore backups",
        Group::Navigation,
    ),
    cmd(
        CommandId::GoMigrations,
        "Schema Migrations",
        "Compare two connections and generate migration SQL",
        Group::Navigation,
    ),
    cmd(
        CommandId::GoPlugins,
        "Go to Plugin Manager",
        "Manage extensions",
        Group::Navigation,
    ),
    with_chord(
        cmd(
            CommandId::GoSettings,
            "Open Settings",
            "Configure the application",
            Group::Navigation,
        ),
        Chord::cmd(KeyCode::Comma),
    ),
    cmd(
        CommandId::GoAbout,
        "Go to About",
        "Version and license information",
        Group::Navigation,
    ),
    // ── Actions ──────────────────────────────────────────────────────────
    with_chord(
        cmd(
            CommandId::NewConnection,
            "New Connection",
            "Create a new database connection",
            Group::Actions,
        ),
        Chord::cmd(KeyCode::N),
    ),
    with_chord(
        cmd(
            CommandId::NewWindow,
            "New Window",
            "Open a second DB Hive window",
            Group::Actions,
        ),
        Chord::cmd_shift(KeyCode::N),
    ),
    scoped(
        cmd(
            CommandId::NewTable,
            "New Table",
            "Create a table in the current schema",
            Group::Actions,
        ),
        Scope::Workspace,
    ),
    scoped(
        cmd(
            CommandId::RefreshMetadata,
            "Refresh Metadata",
            "Reload databases, schemas and tables",
            Group::Actions,
        ),
        Scope::Workspace,
    ),
    with_chord(
        cmd(
            CommandId::ShowShortcuts,
            "Keyboard Shortcuts",
            "Show every keyboard shortcut",
            Group::Actions,
        ),
        Chord::bare(KeyCode::Questionmark),
    ),
    // Listed so the palette documents its own opening key; running it from
    // inside the palette simply keeps the palette open.
    with_chord(
        cmd(
            CommandId::OpenCommandPalette,
            "Command Palette",
            "Search and run commands from anywhere",
            Group::Actions,
        ),
        Chord::cmd(KeyCode::K),
    ),
    // ── Connections ──────────────────────────────────────────────────────
    cmd(
        CommandId::Connect,
        "Connect to Database",
        "Open the saved connection picker",
        Group::Connections,
    ),
    scoped(
        cmd(
            CommandId::Disconnect,
            "Disconnect",
            "Close the current database connection",
            Group::Connections,
        ),
        Scope::Workspace,
    ),
    hidden(scoped(
        with_chord(
            cmd(
                CommandId::ConnectionsBack,
                "Back",
                "Return to the connection list",
                Group::Connections,
            ),
            Chord::bare(KeyCode::Escape),
        ),
        Scope::Connections,
    )),
    // ── Query workspace ──────────────────────────────────────────────────
    scoped(
        with_chord(
            cmd(
                CommandId::RunQuery,
                "Run Query",
                "Execute the current statement",
                Group::Query,
            ),
            Chord::cmd(KeyCode::Enter),
        ),
        Scope::Workspace,
    ),
    scoped(
        with_chord(
            cmd(
                CommandId::NewTab,
                "New Tab",
                "Open another query tab",
                Group::Query,
            ),
            Chord::cmd(KeyCode::T),
        ),
        Scope::Workspace,
    ),
    scoped(
        with_chord(
            cmd(
                CommandId::CloseTab,
                "Close Tab",
                "Close the current query tab",
                Group::Query,
            ),
            Chord::cmd(KeyCode::W),
        ),
        Scope::Workspace,
    ),
    scoped(
        with_chord(
            cmd(
                CommandId::ToggleSidebar,
                "Toggle Sidebar",
                "Show or hide the schema explorer",
                Group::Query,
            ),
            Chord::cmd(KeyCode::B),
        ),
        Scope::Workspace,
    ),
    // ── Theme ────────────────────────────────────────────────────────────
    cmd(
        CommandId::ThemeLight,
        "Switch to Light Theme",
        "Use the light palette",
        Group::Theme,
    ),
    cmd(
        CommandId::ThemeDark,
        "Switch to Dark Theme",
        "Use the dark palette",
        Group::Theme,
    ),
    cmd(
        CommandId::ThemeToggle,
        "Toggle Theme",
        "Swap between light and dark",
        Group::Theme,
    ),
    // ── Window ───────────────────────────────────────────────────────────
    cmd(
        CommandId::WindowMinimize,
        "Minimize Window",
        "Send the window to the dock",
        Group::Window,
    ),
    cmd(
        CommandId::WindowMaximizeToggle,
        "Toggle Maximize Window",
        "Maximize or restore the window",
        Group::Window,
    ),
    cmd(
        CommandId::WindowClose,
        "Close Window",
        "Quit DB Hive",
        Group::Window,
    ),
];

/// Look a command up by id.
pub fn command(id: CommandId) -> &'static Command {
    COMMANDS
        .iter()
        .find(|c| c.id == id)
        .expect("every CommandId is present in COMMANDS; the registry test enforces it")
}

/// Whether a command's scope is live on `screen`.
fn scope_active(scope: Scope, screen: Scope) -> bool {
    scope == Scope::Global || scope == screen
}

/// The command bound to `chord` on a screen, if any.
///
/// Chords are compared exactly here; the fuzzier `matches_logically` rules
/// only apply when reading real input, and [`dispatch_order`] compensates.
pub fn command_for_chord(chord: Chord, screen: Scope) -> Option<CommandId> {
    COMMANDS
        .iter()
        .find(|c| c.chord == Some(chord) && scope_active(c.scope, screen))
        .map(|c| c.id)
}

/// Chorded commands live on `screen`, most specific chord first.
pub fn dispatch_order(screen: Scope) -> Vec<&'static Command> {
    let mut v: Vec<&'static Command> = COMMANDS
        .iter()
        .filter(|c| c.chord.is_some() && scope_active(c.scope, screen))
        .collect();
    v.sort_by(|a, b| {
        b.chord
            .unwrap()
            .specificity()
            .cmp(&a.chord.unwrap().specificity())
    });
    v
}

// ─────────────────────────────────────────────────────────────────────────
// Matching
// ─────────────────────────────────────────────────────────────────────────

/// Score a command against a search needle. Higher is better; `None` means
/// no match.
///
/// The React palette used a plain case-insensitive `includes`, which put
/// "Schema Migrations" above "Open Settings" for the query "settings" purely
/// by array order. Ranking fixes that: exact title beats prefix beats word
/// prefix beats substring beats description beats a fuzzy subsequence.
pub fn score(cmd: &Command, needle: &str) -> Option<i32> {
    let needle = needle.trim().to_lowercase();
    if needle.is_empty() {
        return Some(0);
    }
    let title = cmd.title.to_lowercase();

    if title == needle {
        return Some(1000);
    }
    if title.starts_with(&needle) {
        // Shorter titles win: "New Tab" ranks above "New Table" for "new ta".
        let slack = (title.len() - needle.len()).min(99) as i32;
        return Some(900 - slack);
    }
    if title.split_whitespace().any(|w| w.starts_with(&needle)) {
        // Same rule as above: "New Tab" ranks above "New Table" for "tab".
        let slack = (title.len() - needle.len()).min(99) as i32;
        return Some(700 - slack);
    }
    if title.contains(&needle) {
        return Some(600);
    }
    if cmd.group.title().to_lowercase().contains(&needle) {
        return Some(500);
    }
    if cmd.description.to_lowercase().contains(&needle) {
        return Some(400);
    }
    if let Some(chord) = cmd.chord {
        if chord.display().to_lowercase().contains(&needle) {
            return Some(350);
        }
    }
    subsequence_gaps(&title, &needle).map(|gaps| 200 - gaps.min(199) as i32)
}

/// If `needle` appears in `haystack` as an ordered subsequence, return how
/// many times the match had to skip forward. Fewer skips is a tighter match.
fn subsequence_gaps(haystack: &str, needle: &str) -> Option<usize> {
    let hay: Vec<char> = haystack.chars().collect();
    let mut gaps = 0usize;
    let mut i = 0usize;
    for nc in needle.chars() {
        let start = i;
        loop {
            if i >= hay.len() {
                return None;
            }
            if hay[i] == nc {
                break;
            }
            i += 1;
        }
        if i > start {
            gaps += 1;
        }
        i += 1;
    }
    Some(gaps)
}

/// Palette results: ranked, then bucketed into groups in display order.
///
/// Only commands live on `screen` are offered, so "Disconnect" does not
/// appear when nothing is connected.
pub fn palette_groups(query: &str, screen: Scope) -> Vec<(Group, Vec<&'static Command>)> {
    let mut scored: Vec<(i32, usize, &'static Command)> = COMMANDS
        .iter()
        .enumerate()
        .filter(|(_, c)| c.in_palette && scope_active(c.scope, screen))
        .filter_map(|(i, c)| score(c, query).map(|s| (s, i, c)))
        .collect();
    // Registry order breaks ties, so an empty query renders the table as
    // written rather than in some hash order.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    let mut out: Vec<(Group, Vec<&'static Command>)> = Vec::new();
    for group in GROUP_ORDER {
        let items: Vec<&'static Command> = scored
            .iter()
            .filter(|(_, _, c)| c.group == group)
            .map(|(_, _, c)| *c)
            .collect();
        if !items.is_empty() {
            out.push((group, items));
        }
    }
    out
}

/// Ranked flat results, in the same order the palette walks with the arrow
/// keys.
pub fn palette_results(query: &str, screen: Scope) -> Vec<&'static Command> {
    palette_groups(query, screen)
        .into_iter()
        .flat_map(|(_, items)| items)
        .collect()
}

/// The help modal's content: every chorded command, grouped by category.
///
/// Derived from the same table as dispatch, so a binding cannot be
/// documented without existing or exist without being documented.
pub fn help_groups(query: &str) -> Vec<(Group, Vec<&'static Command>)> {
    let needle = query.trim().to_lowercase();
    let mut out: Vec<(Group, Vec<&'static Command>)> = Vec::new();
    for group in GROUP_ORDER {
        let items: Vec<&'static Command> = COMMANDS
            .iter()
            .filter(|c| c.group == group && c.chord.is_some())
            .filter(|c| {
                needle.is_empty()
                    || c.title.to_lowercase().contains(&needle)
                    || c.description.to_lowercase().contains(&needle)
                    || c.chord.unwrap().display().to_lowercase().contains(&needle)
            })
            .collect();
        if !items.is_empty() {
            out.push((group, items));
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use KeyCode as Key;

    /// Every variant of `CommandId`. Kept next to the registry test so a new
    /// variant fails loudly here rather than silently going unbound.
    const ALL_IDS: &[CommandId] = &[
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

    fn by_id(id: CommandId) -> &'static Command {
        command(id)
    }

    // ── Registry integrity ───────────────────────────────────────────────

    #[test]
    fn every_command_id_appears_exactly_once() {
        assert_eq!(
            COMMANDS.len(),
            ALL_IDS.len(),
            "registry and ALL_IDS disagree"
        );
        for id in ALL_IDS {
            let n = COMMANDS.iter().filter(|c| c.id == *id).count();
            assert_eq!(n, 1, "{:?} appears {} times in COMMANDS", id, n);
        }
    }

    #[test]
    fn no_two_commands_share_a_chord_in_the_same_scope() {
        // A chord may legitimately repeat across mutually exclusive scopes,
        // but never within one screen's live set.
        for screen in [Scope::Connections, Scope::Workspace] {
            let mut seen: HashSet<Chord> = HashSet::new();
            for cmd in dispatch_order(screen) {
                let chord = cmd.chord.unwrap();
                assert!(
                    seen.insert(chord),
                    "{} duplicates the chord {} on {:?}",
                    cmd.title,
                    chord.format(false),
                    screen
                );
            }
        }
    }

    #[test]
    fn every_chorded_command_is_reachable_from_the_help_list() {
        let listed: HashSet<CommandId> = help_groups("")
            .into_iter()
            .flat_map(|(_, items)| items)
            .map(|c| c.id)
            .collect();
        for cmd in COMMANDS.iter().filter(|c| c.chord.is_some()) {
            assert!(
                listed.contains(&cmd.id),
                "{} has a binding but is missing from the shortcuts modal",
                cmd.title
            );
        }
        assert_eq!(
            listed.len(),
            COMMANDS.iter().filter(|c| c.chord.is_some()).count()
        );
    }

    #[test]
    fn help_list_never_invents_a_binding() {
        for (_, items) in help_groups("") {
            for item in items {
                assert!(
                    item.chord.is_some(),
                    "{} listed without a binding",
                    item.title
                );
            }
        }
    }

    #[test]
    fn help_groups_use_the_display_order() {
        let order: Vec<Group> = help_groups("").into_iter().map(|(g, _)| g).collect();
        let expected: Vec<Group> = GROUP_ORDER
            .into_iter()
            .filter(|g| order.contains(g))
            .collect();
        assert_eq!(order, expected);
    }

    #[test]
    fn help_search_matches_title_description_and_chord() {
        let hits = |q: &str| -> Vec<CommandId> {
            help_groups(q)
                .into_iter()
                .flat_map(|(_, i)| i)
                .map(|c| c.id)
                .collect()
        };
        assert!(hits("tab").contains(&CommandId::NewTab));
        assert!(hits("schema explorer").contains(&CommandId::ToggleSidebar));
        assert!(hits(&Chord::cmd(Key::B).display()).contains(&CommandId::ToggleSidebar));
        assert!(hits("zzzz").is_empty());
    }

    // ── Scoping ──────────────────────────────────────────────────────────

    #[test]
    fn workspace_chords_are_dead_on_the_connection_list() {
        assert_eq!(
            command_for_chord(Chord::cmd(Key::Enter), Scope::Workspace),
            Some(CommandId::RunQuery)
        );
        assert_eq!(
            command_for_chord(Chord::cmd(Key::Enter), Scope::Connections),
            None
        );
    }

    #[test]
    fn escape_only_means_back_on_the_connection_list() {
        assert_eq!(
            command_for_chord(Chord::bare(Key::Escape), Scope::Connections),
            Some(CommandId::ConnectionsBack)
        );
        assert_eq!(
            command_for_chord(Chord::bare(Key::Escape), Scope::Workspace),
            None
        );
    }

    #[test]
    fn global_chords_are_live_everywhere() {
        for screen in [Scope::Connections, Scope::Workspace] {
            assert_eq!(
                command_for_chord(Chord::cmd(Key::K), screen),
                Some(CommandId::OpenCommandPalette)
            );
            assert_eq!(
                command_for_chord(Chord::cmd(Key::Comma), screen),
                Some(CommandId::GoSettings)
            );
        }
    }

    #[test]
    fn more_specific_chords_dispatch_first() {
        // egui's `matches_logically` ignores an extra Shift, so Cmd+Shift+N
        // must be tested before Cmd+N or "New Window" is unreachable.
        let order = dispatch_order(Scope::Connections);
        let shift_n = order
            .iter()
            .position(|c| c.id == CommandId::NewWindow)
            .unwrap();
        let plain_n = order
            .iter()
            .position(|c| c.id == CommandId::NewConnection)
            .unwrap();
        assert!(shift_n < plain_n);
    }

    // ── Chord formatting ─────────────────────────────────────────────────

    #[test]
    fn chords_format_with_cmd_on_mac_and_ctrl_elsewhere() {
        let k = Chord::cmd(Key::K);
        assert_eq!(k.format(true), "\u{2318}K");
        assert_eq!(k.format(false), "Ctrl+K");

        // Shift is spelled, not symbolised: the bundled fonts have no U+21E7
        // and a missing glyph renders as an empty box. See theme::glyph.
        let new_window = Chord::cmd_shift(Key::N);
        assert_eq!(new_window.format(true), "Shift \u{2318}N");
        assert_eq!(new_window.format(false), "Ctrl+Shift+N");
    }

    #[test]
    fn alt_is_spelled_option_on_mac_because_the_font_lacks_the_symbol() {
        let chord = Chord {
            command: true,
            shift: false,
            alt: true,
            key: Key::F,
        };
        assert_eq!(chord.format(true), "Option \u{2318}F");
        assert_eq!(chord.format(false), "Ctrl+Alt+F");
    }

    #[test]
    fn named_and_punctuation_keys_get_readable_labels() {
        assert_eq!(Chord::cmd(Key::Comma).format(false), "Ctrl+,");
        assert_eq!(Chord::cmd(Key::Comma).format(true), "\u{2318},");
        assert_eq!(Chord::cmd(Key::Enter).format(false), "Ctrl+Enter");
        assert_eq!(Chord::bare(Key::Escape).format(false), "Esc");
        assert_eq!(Chord::bare(Key::Questionmark).format(true), "?");
    }

    #[test]
    fn display_follows_the_build_target() {
        let expected = Chord::cmd(Key::K).format(cfg!(target_os = "macos"));
        assert_eq!(Chord::cmd(Key::K).display(), expected);
    }

    // ── Matcher ──────────────────────────────────────────────────────────

    #[test]
    fn empty_query_matches_everything_in_registry_order() {
        let results = palette_results("", Scope::Workspace);
        let expected: Vec<CommandId> = COMMANDS
            .iter()
            .filter(|c| c.in_palette && (c.scope == Scope::Global || c.scope == Scope::Workspace))
            .map(|c| c.id)
            .collect();
        assert_eq!(results.len(), expected.len());
        // Grouping reorders, so compare as sets plus per-group order below.
        let got: HashSet<CommandId> = results.iter().map(|c| c.id).collect();
        assert_eq!(got, expected.into_iter().collect::<HashSet<_>>());
    }

    #[test]
    fn matching_is_case_insensitive() {
        let lower = palette_results("settings", Scope::Workspace);
        let upper = palette_results("SETTINGS", Scope::Workspace);
        let mixed = palette_results("  SeTtInGs  ", Scope::Workspace);
        let ids = |v: Vec<&'static Command>| -> Vec<CommandId> { v.iter().map(|c| c.id).collect() };
        assert_eq!(ids(lower.clone()), ids(upper));
        assert_eq!(ids(lower), ids(mixed));
    }

    #[test]
    fn nothing_matches_a_nonsense_query() {
        assert!(palette_results("qqzzxwv", Scope::Workspace).is_empty());
        assert!(score(by_id(CommandId::GoSettings), "qqzzxwv").is_none());
    }

    #[test]
    fn exact_title_outranks_a_prefix_which_outranks_a_substring() {
        let exact = score(by_id(CommandId::NewTab), "new tab").unwrap();
        let prefix = score(by_id(CommandId::NewTable), "new tab").unwrap();
        assert!(
            exact > prefix,
            "exact {} should beat prefix {}",
            exact,
            prefix
        );

        // "New Connection" matches at a word boundary; "Disconnect" only
        // mid-word.
        let word_prefix = score(by_id(CommandId::NewConnection), "conn").unwrap();
        let substring = score(by_id(CommandId::Disconnect), "conn").unwrap();
        assert!(
            word_prefix > substring,
            "word {} should beat substring {}",
            word_prefix,
            substring
        );
    }

    #[test]
    fn title_matches_outrank_description_matches() {
        // "queries" is in the SQL editor's description, not its title.
        let title = score(by_id(CommandId::GoSettings), "settings").unwrap();
        let desc = score(by_id(CommandId::GoQueryEditor), "queries").unwrap();
        assert!(
            title > desc,
            "title {} should beat description {}",
            title,
            desc
        );

        // "Configure the application" only appears in the description.
        let only_desc = score(by_id(CommandId::GoSettings), "configure").unwrap();
        assert!(title > only_desc);
        // Still ranked above a bare fuzzy subsequence.
        assert!(only_desc > 200);
    }

    #[test]
    fn shorter_titles_win_a_prefix_tie() {
        let short = score(by_id(CommandId::NewTab), "new ta").unwrap();
        let long = score(by_id(CommandId::NewTable), "new ta").unwrap();
        assert!(short > long);
    }

    #[test]
    fn best_match_is_first_in_the_flat_result_list() {
        let results = palette_results("settings", Scope::Connections);
        assert_eq!(results.first().map(|c| c.id), Some(CommandId::GoSettings));

        let results = palette_results("disconnect", Scope::Workspace);
        assert_eq!(results.first().map(|c| c.id), Some(CommandId::Disconnect));
    }

    #[test]
    fn fuzzy_subsequence_is_the_last_resort() {
        // "gsd" is not a substring of anything, but is a subsequence of
        // "go to schema designer".
        let fuzzy = score(by_id(CommandId::GoSchemaDesigner), "gsd").unwrap();
        assert!(fuzzy <= 200);
        let substring = score(by_id(CommandId::GoSchemaDesigner), "schema").unwrap();
        assert!(substring > fuzzy);
    }

    #[test]
    fn subsequence_gaps_counts_skips() {
        assert_eq!(subsequence_gaps("abc", "abc"), Some(0));
        assert_eq!(subsequence_gaps("axbxc", "abc"), Some(2));
        assert_eq!(subsequence_gaps("abc", "acb"), None);
        assert_eq!(subsequence_gaps("abc", "abcd"), None);
    }

    #[test]
    fn hidden_commands_stay_out_of_the_palette() {
        let ids: HashSet<CommandId> = palette_results("", Scope::Connections)
            .iter()
            .map(|c| c.id)
            .collect();
        assert!(!ids.contains(&CommandId::ConnectionsBack));
    }

    #[test]
    fn palette_hides_commands_that_are_dead_on_this_screen() {
        let connections: HashSet<CommandId> = palette_results("", Scope::Connections)
            .iter()
            .map(|c| c.id)
            .collect();
        assert!(!connections.contains(&CommandId::Disconnect));
        assert!(!connections.contains(&CommandId::RunQuery));

        let workspace: HashSet<CommandId> = palette_results("", Scope::Workspace)
            .iter()
            .map(|c| c.id)
            .collect();
        assert!(workspace.contains(&CommandId::Disconnect));
        assert!(workspace.contains(&CommandId::RunQuery));
    }

    #[test]
    fn palette_groups_stay_in_display_order() {
        let order: Vec<Group> = palette_groups("", Scope::Workspace)
            .into_iter()
            .map(|(g, _)| g)
            .collect();
        let expected: Vec<Group> = GROUP_ORDER
            .into_iter()
            .filter(|g| order.contains(g))
            .collect();
        assert_eq!(order, expected);
    }

    #[test]
    fn flat_results_match_the_grouped_rows_the_ui_draws() {
        let grouped: Vec<CommandId> = palette_groups("go", Scope::Workspace)
            .into_iter()
            .flat_map(|(_, i)| i)
            .map(|c| c.id)
            .collect();
        let flat: Vec<CommandId> = palette_results("go", Scope::Workspace)
            .iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(grouped, flat);
    }
}
