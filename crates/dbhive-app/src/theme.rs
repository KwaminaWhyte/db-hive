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

pub const DARK: Palette = Palette {
    background: Color32::from_rgb(9, 10, 14),
    sidebar: Color32::from_rgb(13, 14, 19),
    card: Color32::from_rgb(19, 20, 26),
    elevated: Color32::from_rgb(26, 28, 35),
    popover: Color32::from_rgb(24, 25, 32),

    foreground: Color32::from_rgb(240, 241, 244),
    muted_foreground: Color32::from_rgb(150, 153, 162),
    dim_foreground: Color32::from_rgb(104, 108, 118),

    primary: Color32::from_rgb(248, 176, 42),
    primary_foreground: Color32::from_rgb(24, 17, 2),
    primary_soft: Color32::from_rgb(48, 39, 18),

    secondary: Color32::from_rgb(30, 32, 40),
    hover: Color32::from_rgb(38, 41, 50),
    pressed: Color32::from_rgb(46, 49, 60),

    border: Color32::from_rgb(38, 40, 49),
    border_strong: Color32::from_rgb(58, 61, 72),

    destructive: Color32::from_rgb(248, 113, 113),
    destructive_soft: Color32::from_rgb(56, 26, 28),
    warning: Color32::from_rgb(240, 189, 70),
    warning_soft: Color32::from_rgb(54, 42, 15),
    success: Color32::from_rgb(74, 205, 122),
    success_soft: Color32::from_rgb(20, 48, 32),

    muted: Color32::from_rgb(30, 32, 40),
    accent: Color32::from_rgb(38, 41, 50),
};

pub const LIGHT: Palette = Palette {
    // Light mode inverts how depth works: the page is grey and content is
    // white, and the topmost layer is separated by shadow rather than by a
    // lighter fill, because there is nothing lighter than white.
    background: Color32::from_rgb(245, 246, 249),
    sidebar: Color32::from_rgb(239, 241, 245),
    card: Color32::from_rgb(255, 255, 255),
    elevated: Color32::from_rgb(255, 255, 255),
    popover: Color32::from_rgb(255, 255, 255),

    foreground: Color32::from_rgb(17, 19, 24),
    muted_foreground: Color32::from_rgb(98, 103, 113),
    dim_foreground: Color32::from_rgb(142, 147, 158),

    primary: Color32::from_rgb(226, 148, 12),
    // Dark text on honey, not white: white on this amber is 3.2:1, which is
    // below the readable floor. It also matches the brand, where the primary
    // button has always been dark text on gold.
    primary_foreground: Color32::from_rgb(28, 18, 0),
    primary_soft: Color32::from_rgb(254, 243, 220),

    secondary: Color32::from_rgb(241, 243, 246),
    hover: Color32::from_rgb(236, 238, 242),
    pressed: Color32::from_rgb(226, 229, 235),

    border: Color32::from_rgb(224, 227, 232),
    border_strong: Color32::from_rgb(198, 203, 212),

    destructive: Color32::from_rgb(198, 40, 40),
    destructive_soft: Color32::from_rgb(253, 235, 235),
    warning: Color32::from_rgb(140, 88, 0),
    warning_soft: Color32::from_rgb(254, 246, 227),
    success: Color32::from_rgb(21, 128, 61),
    success_soft: Color32::from_rgb(233, 248, 238),

    muted: Color32::from_rgb(241, 243, 246),
    accent: Color32::from_rgb(236, 238, 242),
};

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
        (TextStyle::Heading, FontId::new(size::HEADING, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(size::BODY, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(size::LABEL, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(size::SMALL, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(mono_size, FontFamily::Monospace)),
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

    /// Text must not vanish into the surface it sits on.
    ///
    /// A contrast floor catches the class of bug that made bold labels
    /// near-invisible on dark cards: a colour that is legal but unreadable.
    fn luminance(c: Color32) -> f32 {
        let f = |v: u8| {
            let s = v as f32 / 255.0;
            if s <= 0.03928 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        let (hi, lo) = if x > y { (x, y) } else { (y, x) };
        (hi + 0.05) / (lo + 0.05)
    }

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

    #[test]
    fn body_text_is_readable_on_every_surface() {
        for (name, p) in [("dark", &DARK), ("light", &LIGHT)] {
            for (surface, c) in [
                ("background", p.background),
                ("sidebar", p.sidebar),
                ("card", p.card),
                ("elevated", p.elevated),
                ("popover", p.popover),
            ] {
                let ratio = contrast(p.foreground, c);
                assert!(
                    ratio >= 7.0,
                    "{} foreground on {} is only {:.1}:1",
                    name,
                    surface,
                    ratio
                );
            }
        }
    }

    #[test]
    fn supporting_text_still_clears_the_accessibility_floor() {
        // 4.5:1 is the WCAG AA threshold for normal text. Muted text is
        // quieter than body text, not unreadable.
        for (name, p) in [("dark", &DARK), ("light", &LIGHT)] {
            let ratio = contrast(p.muted_foreground, p.card);
            assert!(
                ratio >= 4.5,
                "{} muted text on card is only {:.1}:1",
                name,
                ratio
            );
        }
    }

    #[test]
    fn text_on_the_primary_colour_is_readable() {
        for (name, p) in [("dark", &DARK), ("light", &LIGHT)] {
            let ratio = contrast(p.primary_foreground, p.primary);
            assert!(
                ratio >= 4.5,
                "{} text on primary is only {:.1}:1",
                name,
                ratio
            );
        }
    }

    #[test]
    fn status_colours_read_against_their_own_tint() {
        for (name, p) in [("dark", &DARK), ("light", &LIGHT)] {
            for (what, fg, bg) in [
                ("destructive", p.destructive, p.destructive_soft),
                ("warning", p.warning, p.warning_soft),
                ("success", p.success, p.success_soft),
            ] {
                let ratio = contrast(fg, bg);
                assert!(
                    ratio >= 4.5,
                    "{} {} on its tint is only {:.1}:1",
                    name,
                    what,
                    ratio
                );
            }
        }
    }

    #[test]
    fn content_is_separated_from_the_page_it_sits_on() {
        // A card must be visibly distinct from the background, or the layout
        // reads as one flat sheet.
        for (name, p) in [("dark", &DARK), ("light", &LIGHT)] {
            let delta = (luminance(p.card) - luminance(p.background)).abs();
            assert!(
                delta > 0.002,
                "{}: card and background are indistinguishable",
                name
            );
        }
    }

    #[test]
    fn dark_mode_has_a_third_surface_level() {
        // Dark mode raises the topmost layer with a lighter fill. Light mode
        // cannot: there is nothing lighter than white, so it uses shadow
        // instead, which is why this only applies to dark.
        assert!(
            luminance(DARK.elevated) > luminance(DARK.card),
            "elevated must sit above card in dark mode"
        );
    }
}
