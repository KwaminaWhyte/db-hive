//! Command palette and the keyboard shortcut registry.
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
//! Both modals are global, so the entry point takes the `Context` rather than
//! a `Ui` and returns a [`PaletteAction`] describing what the rest of the app
//! must do.

use crate::app::DbHiveApp;
use crate::screens::Screen;
use crate::theme::{self, Palette};

// ─────────────────────────────────────────────────────────────────────────
// Commands
// ─────────────────────────────────────────────────────────────────────────

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

/// A key chord.
///
/// `command` is egui's `Modifiers::COMMAND`: Cmd on macOS, Ctrl everywhere
/// else. That is the same "a bare Ctrl binding means Cmd on a Mac" rule the
/// React `parseShortcut` hand-rolled, except egui does it at the input layer
/// so it cannot disagree with what the OS delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: egui::Key,
}

impl Chord {
    pub const fn bare(key: egui::Key) -> Self {
        Self { command: false, shift: false, alt: false, key }
    }

    pub const fn cmd(key: egui::Key) -> Self {
        Self { command: true, shift: false, alt: false, key }
    }

    pub const fn cmd_shift(key: egui::Key) -> Self {
        Self { command: true, shift: true, alt: false, key }
    }

    pub fn modifiers(&self) -> egui::Modifiers {
        egui::Modifiers {
            alt: self.alt,
            ctrl: false,
            shift: self.shift,
            mac_cmd: false,
            command: self.command,
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

    /// Format for display: `⌘⇧N` on macOS, `Ctrl+Shift+N` elsewhere.
    ///
    /// `mac` is a parameter rather than a `cfg!` so the formatting is
    /// testable for both platforms from either.
    pub fn format(&self, mac: bool) -> String {
        let key = key_label(self.key);
        if mac {
            let mut s = String::new();
            if self.command {
                s.push('\u{2318}'); // ⌘
            }
            if self.shift {
                s.push('\u{21E7}'); // ⇧
            }
            if self.alt {
                s.push('\u{2325}'); // ⌥
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
/// one existed.
fn key_label(key: egui::Key) -> &'static str {
    use egui::Key;
    match key {
        Key::Escape => "Esc",
        Key::Enter => "Enter",
        Key::Space => "Space",
        Key::Backspace => "Backspace",
        Key::Delete => "Delete",
        Key::Tab => "Tab",
        // `symbol_or_name` already gives "," for Comma and "?" for
        // Questionmark, and the bare letter for letter keys.
        other => other.symbol_or_name(),
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
    /// Whether the command is offered in the palette. `Escape` → back is a
    /// real binding worth documenting but is meaningless as a menu entry.
    pub in_palette: bool,
}

const fn cmd(
    id: CommandId,
    title: &'static str,
    description: &'static str,
    group: Group,
) -> Command {
    Command { id, title, description, group, chord: None, scope: Scope::Global, in_palette: true }
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
    cmd(
        CommandId::GoVisualQueryBuilder,
        "Go to Visual Query Builder",
        "Build queries without writing SQL",
        Group::Navigation,
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
        Chord::cmd(egui::Key::Comma),
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
        Chord::cmd(egui::Key::N),
    ),
    with_chord(
        cmd(CommandId::NewWindow, "New Window", "Open a second DB Hive window", Group::Actions),
        Chord::cmd_shift(egui::Key::N),
    ),
    scoped(
        cmd(CommandId::NewTable, "New Table", "Create a table in the current schema", Group::Actions),
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
        Chord::bare(egui::Key::Questionmark),
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
        Chord::cmd(egui::Key::K),
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
            cmd(CommandId::ConnectionsBack, "Back", "Return to the connection list", Group::Connections),
            Chord::bare(egui::Key::Escape),
        ),
        Scope::Connections,
    )),
    // ── Query workspace ──────────────────────────────────────────────────
    scoped(
        with_chord(
            cmd(CommandId::RunQuery, "Run Query", "Execute the current statement", Group::Query),
            Chord::cmd(egui::Key::Enter),
        ),
        Scope::Workspace,
    ),
    scoped(
        with_chord(
            cmd(CommandId::NewTab, "New Tab", "Open another query tab", Group::Query),
            Chord::cmd(egui::Key::T),
        ),
        Scope::Workspace,
    ),
    scoped(
        with_chord(
            cmd(CommandId::CloseTab, "Close Tab", "Close the current query tab", Group::Query),
            Chord::cmd(egui::Key::W),
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
            Chord::cmd(egui::Key::B),
        ),
        Scope::Workspace,
    ),
    // ── Theme ────────────────────────────────────────────────────────────
    cmd(CommandId::ThemeLight, "Switch to Light Theme", "Use the light palette", Group::Theme),
    cmd(CommandId::ThemeDark, "Switch to Dark Theme", "Use the dark palette", Group::Theme),
    cmd(CommandId::ThemeToggle, "Toggle Theme", "Swap between light and dark", Group::Theme),
    // ── Window ───────────────────────────────────────────────────────────
    cmd(CommandId::WindowMinimize, "Minimize Window", "Send the window to the dock", Group::Window),
    cmd(
        CommandId::WindowMaximizeToggle,
        "Toggle Maximize Window",
        "Maximize or restore the window",
        Group::Window,
    ),
    cmd(CommandId::WindowClose, "Close Window", "Quit DB Hive", Group::Window),
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
        return Some(700);
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
// Screen state
// ─────────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct CommandPalette {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub shortcuts_open: bool,
    pub shortcuts_query: String,
    /// Set the frame the palette opens so the search box takes focus once
    /// rather than stealing it back on every frame.
    focus_search: bool,
    /// Set when the arrow keys moved the selection, so the list scrolls only
    /// on keyboard navigation and not while the mouse hovers.
    scroll_to_selected: bool,
}

impl CommandPalette {
    pub fn is_open(&self) -> bool {
        self.open || self.shortcuts_open
    }

    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.focus_search = true;
        self.scroll_to_selected = false;
    }

    pub fn close(&mut self) {
        self.open = false;
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Frame entry point
// ─────────────────────────────────────────────────────────────────────────

/// Run shortcut dispatch and draw both global modals.
///
/// Call once per frame, after the screens have drawn, and act on the result.
/// Commands that this module can carry out entirely on its own (the two
/// modals, the theme, the window controls) are already done by the time it
/// returns; the action is still reported so the caller can persist or log it.
pub fn show(app: &mut DbHiveApp, ctx: &egui::Context) -> PaletteAction {
    let screen = match app.screen {
        Screen::Workspace => Scope::Workspace,
        Screen::Connections => Scope::Connections,
    };

    let mut fired: Option<CommandId> = None;

    // Global chords are dead while a modal owns the keyboard, otherwise
    // typing "?" into the palette's search box would reopen the help sheet.
    if !app.screens.command_palette.is_open() {
        fired = consume_shortcuts(ctx, screen);
    }

    if app.screens.command_palette.open {
        if let Some(id) = draw_palette(app, ctx, screen) {
            fired = Some(id);
        }
    }
    if app.screens.command_palette.shortcuts_open {
        draw_shortcuts(app, ctx);
    }

    match fired {
        Some(id) => run(app, ctx, id),
        None => PaletteAction::None,
    }
}

/// Match this frame's key presses against the registry.
fn consume_shortcuts(ctx: &egui::Context, screen: Scope) -> Option<CommandId> {
    // A shortcut without a modifier is a character the user may be trying to
    // type. The React shell guarded on the focused element being an input;
    // egui's equivalent is "something has keyboard focus".
    let typing = ctx.memory(|m| m.focused().is_some());

    for cmd in dispatch_order(screen) {
        let chord = cmd.chord.unwrap();
        if typing && !chord.command && !chord.alt {
            continue;
        }
        let shortcut = egui::KeyboardShortcut::new(chord.modifiers(), chord.key);
        if ctx.input_mut(|i| i.consume_shortcut(&shortcut)) {
            return Some(cmd.id);
        }
    }
    None
}

/// Carry out the part of a command this module owns, then report it.
fn run(app: &mut DbHiveApp, ctx: &egui::Context, id: CommandId) -> PaletteAction {
    match id {
        CommandId::OpenCommandPalette => {
            app.screens.command_palette.open();
            // Nothing for the caller to do: the palette is our own modal.
            return PaletteAction::None;
        }
        CommandId::ShowShortcuts => {
            app.screens.command_palette.shortcuts_open = true;
            app.screens.command_palette.shortcuts_query.clear();
            return PaletteAction::None;
        }
        CommandId::ThemeLight => set_mode(app, ctx, theme::Mode::Light),
        CommandId::ThemeDark => set_mode(app, ctx, theme::Mode::Dark),
        CommandId::ThemeToggle => {
            let next = match app.mode {
                theme::Mode::Dark => theme::Mode::Light,
                theme::Mode::Light => theme::Mode::Dark,
            };
            set_mode(app, ctx, next);
        }
        CommandId::WindowMinimize => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        CommandId::WindowMaximizeToggle => {
            let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
        CommandId::WindowClose => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        _ => {}
    }
    PaletteAction::Run(id)
}

fn set_mode(app: &mut DbHiveApp, ctx: &egui::Context, mode: theme::Mode) {
    app.mode = mode;
    theme::apply(ctx, mode);
}

// ─────────────────────────────────────────────────────────────────────────
// Drawing
// ─────────────────────────────────────────────────────────────────────────

/// A translucent wash over the app, derived from the palette background so
/// it reads correctly in both themes. Returns true if it was clicked.
fn scrim(ctx: &egui::Context, palette: &Palette, id: &str) -> bool {
    let rect = ctx.screen_rect();
    let bg = palette.background;
    let wash = egui::Color32::from_rgba_unmultiplied(bg.r(), bg.g(), bg.b(), 190);
    egui::Area::new(egui::Id::new(id))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .interactable(true)
        .show(ctx, |ui| {
            ui.painter().rect_filled(rect, egui::Rounding::ZERO, wash);
            ui.allocate_response(rect.size(), egui::Sense::click()).clicked()
        })
        .inner
}

/// Frame shared by both modals.
fn modal_frame(palette: &Palette) -> egui::Frame {
    egui::Frame::none()
        .fill(palette.popover)
        .stroke(egui::Stroke::new(1.0_f32, palette.border))
        .rounding(egui::Rounding::same(12.0))
        .inner_margin(egui::Margin::same(0.0))
        .shadow(egui::epaint::Shadow {
            offset: egui::vec2(0.0, 8.0),
            blur: 24.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(90),
        })
}

fn draw_palette(app: &mut DbHiveApp, ctx: &egui::Context, screen: Scope) -> Option<CommandId> {
    let palette = app.mode.palette();
    let mut chosen: Option<CommandId> = None;
    let mut close = false;

    if scrim(ctx, palette, "cmd_palette_scrim") {
        close = true;
    }

    // Read the navigation keys before the text field is built, or the search
    // box swallows Enter and the arrows never reach us.
    let results = palette_results(&app.screens.command_palette.query, screen);
    let (up, down, enter, esc, toggle) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::K),
        )
    });

    let state = &mut app.screens.command_palette;
    let count = results.len();
    if count > 0 {
        // Wraparound, like the React palette.
        if down {
            state.selected = (state.selected + 1) % count;
            state.scroll_to_selected = true;
        }
        if up {
            state.selected = (state.selected + count - 1) % count;
            state.scroll_to_selected = true;
        }
        state.selected = state.selected.min(count - 1);
        if enter {
            chosen = Some(results[state.selected].id);
            close = true;
        }
    } else {
        state.selected = 0;
    }
    if esc || toggle {
        close = true;
    }

    let screen_rect = ctx.screen_rect();
    let width = 560.0_f32.min(screen_rect.width() - 48.0).max(280.0);
    let pos = egui::pos2(
        screen_rect.center().x - width / 2.0,
        screen_rect.top() + (screen_rect.height() * 0.16).max(48.0),
    );

    egui::Area::new(egui::Id::new("cmd_palette"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            ui.set_width(width);
            modal_frame(palette).show(ui, |ui| {
                ui.set_width(width);

                // Search row.
                ui.allocate_ui_with_layout(
                    egui::vec2(width, 44.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add_space(14.0);
                        let state = &mut app.screens.command_palette;
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut state.query)
                                .hint_text("Type a command or search...")
                                .frame(false)
                                .desired_width(width - 80.0),
                        );
                        if state.focus_search {
                            edit.request_focus();
                            state.focus_search = false;
                        }
                        if edit.changed() {
                            state.selected = 0;
                        }
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.add_space(14.0);
                                ui.label(crate::theme::text::muted(palette, "Esc"));
                            },
                        );
                    },
                );

                ui.separator();

                let selected = app.screens.command_palette.selected;
                let scroll_to = app.screens.command_palette.scroll_to_selected;
                let mut clicked: Option<CommandId> = None;
                let mut hovered: Option<usize> = None;

                egui::ScrollArea::vertical()
                    .max_height(380.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.add_space(6.0);
                        if results.is_empty() {
                            ui.add_space(24.0);
                            ui.vertical_centered(|ui| {
                                ui.label(crate::theme::text::muted(
                                    palette,
                                    "No commands match that search",
                                ));
                            });
                            ui.add_space(24.0);
                            return;
                        }

                        let mut flat = 0usize;
                        for (group, items) in palette_groups(
                            &app.screens.command_palette.query,
                            screen,
                        ) {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.add_space(14.0);
                                ui.label(crate::theme::text::muted(
                                    palette,
                                    group.title().to_uppercase(),
                                ));
                            });
                            for item in items {
                                let index = flat;
                                flat += 1;
                                let response =
                                    command_row(ui, palette, item, index == selected);
                                if index == selected && scroll_to {
                                    response.scroll_to_me(Some(egui::Align::Center));
                                }
                                if response.hovered() {
                                    hovered = Some(index);
                                }
                                if response.clicked() {
                                    clicked = Some(item.id);
                                }
                            }
                        }
                        ui.add_space(6.0);
                    });

                ui.separator();
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    ui.label(crate::theme::text::muted(
                        palette,
                        "Up / Down to navigate   Enter to run   Esc to close",
                    ));
                    ui.add_space(14.0);
                });

                let state = &mut app.screens.command_palette;
                state.scroll_to_selected = false;
                if let Some(index) = hovered {
                    state.selected = index;
                }
                if let Some(id) = clicked {
                    chosen = Some(id);
                    close = true;
                }
            });
        });

    if close {
        app.screens.command_palette.close();
    }
    chosen
}

/// One palette row. Selection is painted rather than using
/// `selectable_label` so the title, description and chord can share a line.
fn command_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    item: &'static Command,
    selected: bool,
) -> egui::Response {
    let fill = if selected { palette.accent } else { egui::Color32::TRANSPARENT };
    let response = egui::Frame::none()
        .fill(fill)
        .rounding(egui::Rounding::same(6.0))
        .inner_margin(egui::Margin::symmetric(14.0, 6.0))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                if selected {
                    ui.colored_label(palette.primary, "\u{203A}");
                } else {
                    ui.label(" ");
                }
                ui.label(crate::theme::text::strong(palette, item.title));
                if !item.description.is_empty() {
                    ui.label(crate::theme::text::muted(palette, item.description));
                }
                if let Some(chord) = item.chord {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(crate::theme::text::muted(palette, chord.display()));
                    });
                }
            });
        })
        .response;
    response.interact(egui::Sense::click())
}

fn draw_shortcuts(app: &mut DbHiveApp, ctx: &egui::Context) {
    let palette = app.mode.palette();
    let mut close = scrim(ctx, palette, "shortcuts_scrim");

    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        close = true;
    }

    let screen_rect = ctx.screen_rect();
    let width = 620.0_f32.min(screen_rect.width() - 48.0).max(280.0);
    let pos = egui::pos2(
        screen_rect.center().x - width / 2.0,
        screen_rect.top() + (screen_rect.height() * 0.12).max(40.0),
    );

    egui::Area::new(egui::Id::new("shortcuts_modal"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            ui.set_width(width);
            modal_frame(palette).show(ui, |ui| {
                ui.set_width(width);
                let inner = egui::Margin::symmetric(18.0, 14.0);
                egui::Frame::none().inner_margin(inner).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(crate::theme::text::heading(palette, "Keyboard Shortcuts"));
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if ui.button("Close").clicked() {
                                    close = true;
                                }
                                ui.label(crate::theme::text::muted(
                                    palette,
                                    if IS_MAC { "macOS" } else { "Windows / Linux" },
                                ));
                            },
                        );
                    });
                    ui.label(crate::theme::text::muted(
                        palette,
                        "Every binding DB Hive actually listens for.",
                    ));
                    ui.add_space(8.0);

                    let state = &mut app.screens.command_palette;
                    ui.add(
                        egui::TextEdit::singleline(&mut state.shortcuts_query)
                            .hint_text("Search shortcuts...")
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(8.0);

                    let groups = help_groups(&app.screens.command_palette.shortcuts_query);
                    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                        if groups.is_empty() {
                            ui.add_space(20.0);
                            ui.vertical_centered(|ui| {
                                ui.label(crate::theme::text::muted(
                                    palette,
                                    "No shortcuts match that search",
                                ));
                            });
                            return;
                        }
                        for (group, items) in groups {
                            ui.add_space(6.0);
                            ui.label(crate::theme::text::strong(palette, group.title()));
                            ui.add_space(2.0);
                            for item in items {
                                ui.horizontal(|ui| {
                                    ui.label(item.title);
                                    ui.label(crate::theme::text::muted(
                                        palette,
                                        item.description,
                                    ));
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(crate::theme::text::strong(
                                                palette,
                                                item.chord.unwrap().display(),
                                            ));
                                        },
                                    );
                                });
                            }
                            ui.add_space(4.0);
                            ui.separator();
                        }
                    });
                });
            });
        });

    if close {
        app.screens.command_palette.shortcuts_open = false;
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Key;
    use std::collections::HashSet;

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
        assert_eq!(COMMANDS.len(), ALL_IDS.len(), "registry and ALL_IDS disagree");
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
        assert_eq!(listed.len(), COMMANDS.iter().filter(|c| c.chord.is_some()).count());
    }

    #[test]
    fn help_list_never_invents_a_binding() {
        for (_, items) in help_groups("") {
            for item in items {
                assert!(item.chord.is_some(), "{} listed without a binding", item.title);
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
            help_groups(q).into_iter().flat_map(|(_, i)| i).map(|c| c.id).collect()
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
        assert_eq!(command_for_chord(Chord::cmd(Key::Enter), Scope::Connections), None);
    }

    #[test]
    fn escape_only_means_back_on_the_connection_list() {
        assert_eq!(
            command_for_chord(Chord::bare(Key::Escape), Scope::Connections),
            Some(CommandId::ConnectionsBack)
        );
        assert_eq!(command_for_chord(Chord::bare(Key::Escape), Scope::Workspace), None);
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
        let shift_n = order.iter().position(|c| c.id == CommandId::NewWindow).unwrap();
        let plain_n = order.iter().position(|c| c.id == CommandId::NewConnection).unwrap();
        assert!(shift_n < plain_n);
    }

    // ── Chord formatting ─────────────────────────────────────────────────

    #[test]
    fn chords_format_with_cmd_on_mac_and_ctrl_elsewhere() {
        let k = Chord::cmd(Key::K);
        assert_eq!(k.format(true), "\u{2318}K");
        assert_eq!(k.format(false), "Ctrl+K");

        let new_window = Chord::cmd_shift(Key::N);
        assert_eq!(new_window.format(true), "\u{2318}\u{21E7}N");
        assert_eq!(new_window.format(false), "Ctrl+Shift+N");
    }

    #[test]
    fn alt_formats_as_the_option_glyph_on_mac() {
        let chord = Chord { command: true, shift: false, alt: true, key: Key::F };
        assert_eq!(chord.format(true), "\u{2318}\u{2325}F");
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
        assert!(exact > prefix, "exact {} should beat prefix {}", exact, prefix);

        // "New Connection" matches at a word boundary; "Disconnect" only
        // mid-word.
        let word_prefix = score(by_id(CommandId::NewConnection), "conn").unwrap();
        let substring = score(by_id(CommandId::Disconnect), "conn").unwrap();
        assert!(word_prefix > substring, "word {} should beat substring {}", word_prefix, substring);
    }

    #[test]
    fn title_matches_outrank_description_matches() {
        // "queries" is in the SQL editor's description, not its title.
        let title = score(by_id(CommandId::GoSettings), "settings").unwrap();
        let desc = score(by_id(CommandId::GoQueryEditor), "queries").unwrap();
        assert!(title > desc, "title {} should beat description {}", title, desc);

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
        let ids: HashSet<CommandId> =
            palette_results("", Scope::Connections).iter().map(|c| c.id).collect();
        assert!(!ids.contains(&CommandId::ConnectionsBack));
    }

    #[test]
    fn palette_hides_commands_that_are_dead_on_this_screen() {
        let connections: HashSet<CommandId> =
            palette_results("", Scope::Connections).iter().map(|c| c.id).collect();
        assert!(!connections.contains(&CommandId::Disconnect));
        assert!(!connections.contains(&CommandId::RunQuery));

        let workspace: HashSet<CommandId> =
            palette_results("", Scope::Workspace).iter().map(|c| c.id).collect();
        assert!(workspace.contains(&CommandId::Disconnect));
        assert!(workspace.contains(&CommandId::RunQuery));
    }

    #[test]
    fn palette_groups_stay_in_display_order() {
        let order: Vec<Group> =
            palette_groups("", Scope::Workspace).into_iter().map(|(g, _)| g).collect();
        let expected: Vec<Group> =
            GROUP_ORDER.into_iter().filter(|g| order.contains(g)).collect();
        assert_eq!(order, expected);
    }

    #[test]
    fn flat_results_match_the_grouped_rows_the_ui_draws() {
        let grouped: Vec<CommandId> = palette_groups("go", Scope::Workspace)
            .into_iter()
            .flat_map(|(_, i)| i)
            .map(|c| c.id)
            .collect();
        let flat: Vec<CommandId> =
            palette_results("go", Scope::Workspace).iter().map(|c| c.id).collect();
        assert_eq!(grouped, flat);
    }
}
