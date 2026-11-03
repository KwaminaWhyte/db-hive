//! DB Hive's visual design system.
//!
//! One place decides how the whole app looks. Screens take colours from
//! `Palette` and text from `text::*`; nothing hardcodes a colour, so light
//! and dark both work and a change here lands everywhere at once.
//!
//! The colours began as the web shell's OKLCH design tokens, converted to
//! sRGB once. They are design constants, not data: converting a colour space
//! every frame would be waste.
//!
//! ## Surfaces
//!
//! Depth is carried by four fills rather than by borders alone, because a
//! dark UI built only from 1px lines reads as flat:
//!
//! ```text
//!   background  the window itself
//!   sidebar     chrome around the content
//!   card        content sitting on the background
//!   elevated    something on top of a card (menus, popups, selected rows)
//! ```

use dbhive_ui_model::tokens;
use egui::{Color32, FontFamily, FontId, Rounding, Shadow, Stroke, TextStyle, Visuals};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
}

pub struct Palette {
    // Surfaces, lightest-on-dark last.
    pub background: Color32,
    pub sidebar: Color32,
    pub card: Color32,
    pub elevated: Color32,
    pub popover: Color32,

    // Text, in descending emphasis.
    pub foreground: Color32,
    pub muted_foreground: Color32,
    pub dim_foreground: Color32,

    // Brand.
    pub primary: Color32,
    pub primary_foreground: Color32,
    /// Primary at low opacity, for selected rows and subtle highlights.
    pub primary_soft: Color32,

    // Interaction.
    pub secondary: Color32,
    pub hover: Color32,
    pub pressed: Color32,

    // Lines.
    pub border: Color32,
    pub border_strong: Color32,

    // Status, each with a tint for banner backgrounds.
    pub destructive: Color32,
    pub destructive_soft: Color32,
    pub warning: Color32,
    pub warning_soft: Color32,
    pub success: Color32,
    pub success_soft: Color32,

    /// Kept for compatibility with existing call sites.
    pub muted: Color32,
    pub accent: Color32,
}

// The colours themselves live in dbhive_ui_model::tokens, shared with the
// Slint shell and guarded by the contrast tests there. This converts them.
const fn c(rgb: tokens::Rgb) -> Color32 {
    Color32::from_rgb(rgb.0, rgb.1, rgb.2)
}

const fn from_tokens(t: &tokens::Palette) -> Palette {
    Palette {
        background: c(t.background),
        sidebar: c(t.sidebar),
        card: c(t.card),
        elevated: c(t.elevated),
        popover: c(t.popover),
        foreground: c(t.foreground),
        muted_foreground: c(t.muted_foreground),
        dim_foreground: c(t.dim_foreground),
        primary: c(t.primary),
        primary_foreground: c(t.primary_foreground),
        primary_soft: c(t.primary_soft),
        secondary: c(t.secondary),
        hover: c(t.hover),
        pressed: c(t.pressed),
        border: c(t.border),
        border_strong: c(t.border_strong),
        destructive: c(t.destructive),
        destructive_soft: c(t.destructive_soft),
        warning: c(t.warning),
        warning_soft: c(t.warning_soft),
        success: c(t.success),
        success_soft: c(t.success_soft),
        muted: c(t.muted),
        accent: c(t.accent),
    }
}

pub const DARK: Palette = from_tokens(&tokens::DARK);
pub const LIGHT: Palette = from_tokens(&tokens::LIGHT);

impl Mode {
    pub fn palette(self) -> &'static Palette {
        match self {
            Mode::Light => &LIGHT,
            Mode::Dark => &DARK,
        }
    }

    pub fn is_dark(self) -> bool {
        matches!(self, Mode::Dark)
    }
}

/// Corner radii. One vocabulary, so nothing is rounded by accident.
pub mod radius {
    use egui::Rounding;
    /// Inputs, buttons, small controls.
    pub const CONTROL: Rounding = Rounding::same(7.0);
    /// Cards, panels, list rows.
    pub const CARD: Rounding = Rounding::same(10.0);
    /// Windows and popups.
    pub const WINDOW: Rounding = Rounding::same(12.0);
    /// Badges and pills.
    pub const PILL: Rounding = Rounding::same(999.0);
}

/// Type scale.
///
/// egui's defaults are uniform and small, which flattens hierarchy: a heading
/// and a label look nearly alike. These sizes give headings, body, labels and
/// code distinct weights.
pub mod size {
    pub const DISPLAY: f32 = 22.0;
    pub const HEADING: f32 = 17.0;
    pub const SUBHEADING: f32 = 14.5;
    pub const BODY: f32 = 13.5;
    pub const LABEL: f32 = 12.5;
    pub const SMALL: f32 = 11.5;
    pub const MONO: f32 = 12.5;
}

/// Apply the palette, type scale and spacing to a context.
pub fn apply(ctx: &egui::Context, mode: Mode) {
    apply_with_accent(ctx, mode, mode.palette().primary, size::MONO);
}

/// Apply the theme with a user-chosen accent colour and editor font size.
///
/// Settings calls this so the accent picker actually does something; the
/// web shell's equivalent control was wired to nothing.
pub fn apply_with_accent(ctx: &egui::Context, mode: Mode, accent: Color32, mono_size: f32) {
    let p = mode.palette();
    let mut v = if mode.is_dark() {
        Visuals::dark()
    } else {
        Visuals::light()
    };

    v.dark_mode = mode.is_dark();
    v.override_text_color = Some(p.foreground);
    v.panel_fill = p.background;
    v.window_fill = p.popover;
    v.extreme_bg_color = p.card;
    v.faint_bg_color = if mode.is_dark() {
        Color32::from_rgb(16, 17, 23)
    } else {
        Color32::from_rgb(247, 248, 250)
    };
    v.code_bg_color = p.card;

    v.window_rounding = radius::WINDOW;
    v.window_stroke = Stroke::new(1.0_f32, p.border_strong);
    v.menu_rounding = radius::CARD;

    // Depth. egui draws no shadow by default, so menus and dialogs sit flat
    // against the page and read as part of it.
    v.window_shadow = Shadow {
        offset: egui::vec2(0.0_f32, 8.0_f32),
        blur: 24.0,
        spread: 0.0,
        color: Color32::from_black_alpha(if mode.is_dark() { 140 } else { 40 }),
    };
    v.popup_shadow = Shadow {
        offset: egui::vec2(0.0_f32, 4.0_f32),
        blur: 16.0,
        spread: 0.0,
        color: Color32::from_black_alpha(if mode.is_dark() { 120 } else { 30 }),
    };

    v.selection.bg_fill = p.primary_soft;
    v.selection.stroke = Stroke::new(1.0_f32, accent);
    v.hyperlink_color = accent;
    v.error_fg_color = p.destructive;
    v.warn_fg_color = p.warning;

    // Resting state: a quiet surface, not a raised button. Toolbars full of
    // heavy buttons is what made the old shell look busy.
    v.widgets.noninteractive.bg_fill = p.background;
    v.widgets.noninteractive.weak_bg_fill = p.background;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.muted_foreground);
    v.widgets.noninteractive.rounding = radius::CONTROL;
    v.widgets.noninteractive.expansion = 0.0;

    v.widgets.inactive.bg_fill = p.secondary;
    v.widgets.inactive.weak_bg_fill = p.secondary;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.foreground);
    v.widgets.inactive.rounding = radius::CONTROL;
    v.widgets.inactive.expansion = 0.0;

    // Hover lifts the surface and firms the border. The previous theme
    // filled hover with the accent, which shouted on every stray mouse move.
    v.widgets.hovered.bg_fill = p.hover;
    v.widgets.hovered.weak_bg_fill = p.hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.border_strong);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, p.foreground);
    v.widgets.hovered.rounding = radius::CONTROL;
    v.widgets.hovered.expansion = 1.0;

    v.widgets.active.bg_fill = p.pressed;
    v.widgets.active.weak_bg_fill = p.pressed;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, accent);
    // NOTE: egui derives `RichText::strong()` from this stroke, not just
    // pressed-button text. It is deliberately a readable foreground colour
    // rather than text-on-accent; `text::strong` sets its own colour anyway.
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, p.foreground);
    v.widgets.active.rounding = radius::CONTROL;
    v.widgets.active.expansion = 0.0;

    v.widgets.open.bg_fill = p.elevated;
    v.widgets.open.weak_bg_fill = p.elevated;
    v.widgets.open.bg_stroke = Stroke::new(1.0_f32, p.border_strong);
    v.widgets.open.fg_stroke = Stroke::new(1.0_f32, p.foreground);
    v.widgets.open.rounding = radius::CONTROL;

    ctx.set_visuals(v);

    let mut style = (*ctx.style()).clone();

    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(size::HEADING, FontFamily::Proportional),
        ),
        (
            TextStyle::Body,
            FontId::new(size::BODY, FontFamily::Proportional),
        ),
        (
            TextStyle::Button,
            FontId::new(size::LABEL, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(size::SMALL, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(mono_size, FontFamily::Monospace),
        ),
    ]
    .into();

    // Room to breathe. The old spacing was tight enough that labels and the
    // controls they belong to ran together.
    style.spacing.item_spacing = egui::vec2(10.0_f32, 8.0_f32);
    style.spacing.button_padding = egui::vec2(11.0_f32, 6.0_f32);
    style.spacing.menu_margin = egui::Margin::symmetric(6.0_f32, 6.0_f32);
    style.spacing.window_margin = egui::Margin::same(16.0_f32);
    style.spacing.indent = 18.0;
    style.spacing.interact_size = egui::vec2(26.0_f32, 26.0_f32);
    style.spacing.combo_height = 320.0;
    style.spacing.tooltip_width = 420.0;
    style.spacing.menu_width = 200.0;

    // A thin, unobtrusive scrollbar; the default is wide and always visible.
    style.spacing.scroll = egui::style::ScrollStyle {
        bar_width: 8.0,
        handle_min_length: 24.0,
        bar_inner_margin: 2.0,
        bar_outer_margin: 0.0,
        floating: true,
        floating_width: 6.0,
        floating_allocated_width: 0.0,
        ..Default::default()
    };

    style.visuals.striped = true;
    ctx.set_style(style);
}

/// The icon vocabulary.
///
/// egui bundles a proportional font plus a small emoji-icon font, and that
/// is all the glyph coverage there is. A character outside it renders as an
/// empty box, silently: `✕`, `▲`, `●` and the macOS modifier symbols all did,
/// in shipped UI, until a test caught them.
///
/// Every icon the app draws is named here and covered by
/// `every_glyph_we_use_actually_renders`. Do not inline a symbol at a call
/// site; add it here so the test sees it.
pub mod glyph {
    /// Dismiss, close, remove.
    pub const CLOSE: &str = "✖";
    /// Success, confirmation.
    pub const CHECK: &str = "✔";
    /// A warning that is not an error.
    pub const WARNING: &str = "⚠";
    /// Neutral information.
    pub const INFO: &str = "ℹ";
    /// Overflow menu.
    pub const MORE: &str = "…";
    /// Favourite, on and off.
    pub const STAR_ON: &str = "★";
    pub const STAR_OFF: &str = "☆";
    /// A status dot.
    pub const DOT: &str = "•";
    /// Sort direction.
    pub const SORT_ASC: &str = "⏶";
    pub const SORT_DESC: &str = "⏷";
    /// Move an item up or down an ordered list.
    pub const MOVE_UP: &str = "⬆";
    pub const MOVE_DOWN: &str = "⬇";
    /// Points at a consequence: "source → target".
    pub const ARROW: &str = "➡";
    /// Refresh.
    pub const REFRESH: &str = "🔃";
    /// Add.
    pub const ADD: &str = "➕";
    /// Search.
    pub const SEARCH: &str = "🔍";
    /// Delete.
    pub const TRASH: &str = "🗑";
    /// Settings.
    pub const SETTINGS: &str = "⚙";
    /// Run.
    pub const RUN: &str = "⏵";
    /// Theme toggle, by target mode.
    pub const TO_LIGHT: &str = "☀";
    pub const TO_DARK: &str = "◑";
    /// The Command key. The only modifier symbol the bundled fonts carry,
    /// which is why the others are spelled out in shortcut labels.
    pub const COMMAND: &str = "⌘";
    /// The app mark.
    pub const HIVE: &str = "🐝";

    /// Everything above, for the coverage test.
    pub const ALL: &[&str] = &[
        CLOSE, CHECK, WARNING, INFO, MORE, STAR_ON, STAR_OFF, DOT, SORT_ASC, SORT_DESC, MOVE_UP,
        MOVE_DOWN, ARROW, REFRESH, ADD, SEARCH, TRASH, SETTINGS, RUN, TO_LIGHT, TO_DARK, COMMAND,
        HIVE,
    ];
}

/// Text helpers.
///
/// egui's `RichText::strong()` takes its colour from the active widget
/// stroke, which is not always what a heading wants. Text goes through here
/// so emphasis is consistent and always readable on the surface it sits on.
pub mod text {
    use super::{size, Palette};
    use egui::RichText;

    /// The largest text on a screen. One per screen.
    pub fn display(p: &Palette, s: impl Into<String>) -> RichText {
        RichText::new(s)
            .size(size::DISPLAY)
            .strong()
            .color(p.foreground)
    }

    /// A screen or dialog title.
    pub fn heading(p: &Palette, s: impl Into<String>) -> RichText {
        RichText::new(s)
            .size(size::HEADING)
            .strong()
            .color(p.foreground)
    }

    /// A section title within a screen.
    pub fn subheading(p: &Palette, s: impl Into<String>) -> RichText {
        RichText::new(s)
            .size(size::SUBHEADING)
            .strong()
            .color(p.foreground)
    }

    /// Emphasised body text.
    pub fn strong(p: &Palette, s: impl Into<String>) -> RichText {
        RichText::new(s).strong().color(p.foreground)
    }

    /// Ordinary body text.
    pub fn body(p: &Palette, s: impl Into<String>) -> RichText {
        RichText::new(s).color(p.foreground)
    }

    /// Supporting text: captions, counts, hints.
    pub fn muted(p: &Palette, s: impl Into<String>) -> RichText {
        RichText::new(s).size(size::SMALL).color(p.muted_foreground)
    }

    /// The quietest text there is. Placeholders and disabled hints.
    pub fn dim(p: &Palette, s: impl Into<String>) -> RichText {
        RichText::new(s).size(size::SMALL).color(p.dim_foreground)
    }

    /// A field label above an input.
    pub fn label(p: &Palette, s: impl Into<String>) -> RichText {
        RichText::new(s).size(size::LABEL).color(p.muted_foreground)
    }

    /// Code and identifiers.
    pub fn mono(p: &Palette, s: impl Into<String>) -> RichText {
        RichText::new(s).monospace().color(p.foreground)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Contrast and surface tests live with the tokens in dbhive_ui_model.

    /// Every icon must exist in the fonts actually bundled with the build.
    ///
    /// A missing glyph does not fail, warn or fall back: egui draws an empty
    /// box. This is the only thing standing between the icon vocabulary and
    /// that happening again.
    #[test]
    fn every_glyph_we_use_actually_renders() {
        let ctx = egui::Context::default();
        // Fonts are built on the first frame.
        let _ = ctx.run(egui::RawInput::default(), |_| {});

        let mut missing = Vec::new();
        ctx.fonts(|f| {
            for icon in glyph::ALL {
                for size in [super::size::SMALL, super::size::BODY, super::size::HEADING] {
                    let id = egui::FontId::proportional(size);
                    for c in icon.chars() {
                        if !f.has_glyph(&id, c) {
                            missing.push(format!("{:?} ({})", c, icon));
                        }
                    }
                }
            }
        });
        assert!(
            missing.is_empty(),
            "these glyphs render as empty boxes: {}",
            missing.join(", ")
        );
    }
}
