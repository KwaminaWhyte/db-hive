//! Command palette and the keyboard shortcut registry: the egui modals and
//! shortcut dispatch.
//!
//! The registry ([`COMMANDS`]), `CommandId`, `Chord`, the matcher and the
//! palette/help grouping live in `dbhive_ui_model::command_palette` and are
//! re-exported here, so `command_palette::CommandId` keeps working. This
//! file keeps the egui glue: converting a registry [`Chord`] to an egui
//! shortcut, consuming input, and drawing both modals.
//!
//! Both modals are global, so the entry point takes the `Context` rather than
//! a `Ui` and returns a [`PaletteAction`] describing what the rest of the app
//! must do.

pub use dbhive_ui_model::command_palette::*;

use crate::app::DbHiveApp;
use crate::screens::Screen;
use crate::theme::{self, Palette};

// ─────────────────────────────────────────────────────────────────────────
// egui key conversion
// ─────────────────────────────────────────────────────────────────────────

/// The egui key for a registry key.
pub fn egui_key(key: KeyCode) -> egui::Key {
    use egui::Key as E;
    match key {
        KeyCode::A => E::A,
        KeyCode::B => E::B,
        KeyCode::C => E::C,
        KeyCode::D => E::D,
        KeyCode::E => E::E,
        KeyCode::F => E::F,
        KeyCode::G => E::G,
        KeyCode::H => E::H,
        KeyCode::I => E::I,
        KeyCode::J => E::J,
        KeyCode::K => E::K,
        KeyCode::L => E::L,
        KeyCode::M => E::M,
        KeyCode::N => E::N,
        KeyCode::O => E::O,
        KeyCode::P => E::P,
        KeyCode::Q => E::Q,
        KeyCode::R => E::R,
        KeyCode::S => E::S,
        KeyCode::T => E::T,
        KeyCode::U => E::U,
        KeyCode::V => E::V,
        KeyCode::W => E::W,
        KeyCode::X => E::X,
        KeyCode::Y => E::Y,
        KeyCode::Z => E::Z,
        KeyCode::Comma => E::Comma,
        KeyCode::Questionmark => E::Questionmark,
        KeyCode::Enter => E::Enter,
        KeyCode::Escape => E::Escape,
        KeyCode::Space => E::Space,
        KeyCode::Backspace => E::Backspace,
        KeyCode::Delete => E::Delete,
        KeyCode::Tab => E::Tab,
    }
}

/// The egui modifiers for a chord. `command` maps to egui's logical
/// `COMMAND` (Cmd on macOS, Ctrl elsewhere), never to a physical key.
pub fn egui_modifiers(chord: Chord) -> egui::Modifiers {
    egui::Modifiers {
        alt: chord.alt,
        ctrl: false,
        shift: chord.shift,
        mac_cmd: false,
        command: chord.command,
    }
}

/// The egui shortcut a chord is dispatched as.
pub fn egui_shortcut(chord: Chord) -> egui::KeyboardShortcut {
    egui::KeyboardShortcut::new(egui_modifiers(chord), egui_key(chord.key))
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
        let shortcut = egui_shortcut(chord);
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

// The registry, matcher and formatting tests live with the logic in
// `dbhive_ui_model::command_palette`. These pin the egui conversion.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_labels_match_what_egui_would_have_printed() {
        // Before the lift, labels fell through to `Key::symbol_or_name` for
        // everything but the named keys. The conversion must keep them equal.
        for &key in KeyCode::ALL {
            let expected = match egui_key(key) {
                egui::Key::Escape => "Esc",
                egui::Key::Enter => "Enter",
                egui::Key::Space => "Space",
                egui::Key::Backspace => "Backspace",
                egui::Key::Delete => "Delete",
                egui::Key::Tab => "Tab",
                other => other.symbol_or_name(),
            };
            assert_eq!(key_label(key), expected, "{:?}", key);
        }
    }

    #[test]
    fn key_conversion_is_injective() {
        let mut seen = std::collections::HashSet::new();
        for &key in KeyCode::ALL {
            assert!(seen.insert(egui_key(key)), "{:?} collides", key);
        }
    }

    #[test]
    fn modifiers_use_the_logical_command_key() {
        let m = egui_modifiers(Chord::cmd_shift(KeyCode::N));
        assert_eq!(
            m,
            egui::Modifiers { alt: false, ctrl: false, shift: true, mac_cmd: false, command: true }
        );
        assert_eq!(egui_modifiers(Chord::bare(KeyCode::Escape)), egui::Modifiers::NONE);
        assert_eq!(egui_modifiers(Chord::cmd(KeyCode::K)), egui::Modifiers::COMMAND);
    }

    #[test]
    fn every_registry_chord_becomes_the_same_egui_shortcut() {
        for cmd in COMMANDS.iter().filter(|c| c.chord.is_some()) {
            let chord = cmd.chord.unwrap();
            let s = egui_shortcut(chord);
            assert_eq!(s.logical_key, egui_key(chord.key));
            assert_eq!(s.modifiers, egui_modifiers(chord));
        }
    }

    #[test]
    fn command_glyph_matches_the_theme() {
        assert_eq!(COMMAND_GLYPH, theme::glyph::COMMAND);
    }
}
