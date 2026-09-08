//! DB Hive palette, ported from the web shell's design tokens.
//!
//! The React shell defined its colours as OKLCH custom properties in
//! `src/index.css`. Those were converted to sRGB once and hard-coded here
//! rather than converted at runtime: they are design constants, not data,
//! and a colour-space conversion on every frame would be waste.
//!
//! Keep these in step with `src/index.css` while both shells exist. The
//! honey gold primary is the brand colour; do not substitute a stock accent.

use egui::{Color32, Rounding, Stroke, Visuals};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
}

pub struct Palette {
    pub background: Color32,
    pub foreground: Color32,
    pub card: Color32,
    pub popover: Color32,
    pub primary: Color32,
    pub primary_foreground: Color32,
    pub secondary: Color32,
    pub muted: Color32,
    pub muted_foreground: Color32,
    pub accent: Color32,
    pub border: Color32,
    pub destructive: Color32,
    pub warning: Color32,
    pub success: Color32,
    pub sidebar: Color32,
}

pub const LIGHT: Palette = Palette {
    background: Color32::from_rgb(252, 252, 252),
    foreground: Color32::from_rgb(9, 11, 15),
    card: Color32::from_rgb(255, 255, 255),
    popover: Color32::from_rgb(255, 255, 255),
    primary: Color32::from_rgb(235, 150, 0),
    primary_foreground: Color32::from_rgb(9, 11, 15),
    secondary: Color32::from_rgb(240, 242, 245),
    muted: Color32::from_rgb(244, 245, 249),
    muted_foreground: Color32::from_rgb(97, 99, 105),
    accent: Color32::from_rgb(255, 236, 193),
    border: Color32::from_rgb(221, 222, 225),
    destructive: Color32::from_rgb(238, 11, 42),
    warning: Color32::from_rgb(210, 133, 0),
    success: Color32::from_rgb(5, 137, 62),
    sidebar: Color32::from_rgb(247, 248, 252),
};

pub const DARK: Palette = Palette {
    background: Color32::from_rgb(5, 6, 9),
    foreground: Color32::from_rgb(248, 248, 248),
    card: Color32::from_rgb(12, 13, 18),
    popover: Color32::from_rgb(16, 17, 22),
    primary: Color32::from_rgb(248, 164, 0),
    primary_foreground: Color32::from_rgb(5, 6, 9),
    secondary: Color32::from_rgb(25, 26, 31),
    muted: Color32::from_rgb(25, 26, 31),
    muted_foreground: Color32::from_rgb(141, 143, 149),
    accent: Color32::from_rgb(30, 33, 43),
    border: Color32::from_rgb(32, 33, 39),
    destructive: Color32::from_rgb(249, 65, 68),
    warning: Color32::from_rgb(234, 181, 50),
    success: Color32::from_rgb(63, 193, 104),
    sidebar: Color32::from_rgb(8, 9, 13),
};

impl Mode {
    pub fn palette(self) -> &'static Palette {
        match self {
            Mode::Light => &LIGHT,
            Mode::Dark => &DARK,
        }
    }
}

/// Apply the palette to an egui context.
pub fn apply(ctx: &egui::Context, mode: Mode) {
    let p = mode.palette();
    let mut v = match mode {
        Mode::Light => Visuals::light(),
        Mode::Dark => Visuals::dark(),
    };

    v.override_text_color = Some(p.foreground);
    v.panel_fill = p.background;
    v.window_fill = p.popover;
    v.extreme_bg_color = p.card;
    v.faint_bg_color = p.muted;
    v.window_stroke = Stroke::new(1.0_f32, p.border);
    v.selection.bg_fill = p.primary.linear_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0_f32, p.primary);
    v.hyperlink_color = p.primary;
    v.error_fg_color = p.destructive;
    v.warn_fg_color = p.warning;

    let r = Rounding::same(6.0);
    v.widgets.noninteractive.bg_fill = p.background;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.muted_foreground);
    v.widgets.noninteractive.rounding = r;

    v.widgets.inactive.bg_fill = p.secondary;
    v.widgets.inactive.weak_bg_fill = p.secondary;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.foreground);
    v.widgets.inactive.rounding = r;

    v.widgets.hovered.bg_fill = p.accent;
    v.widgets.hovered.weak_bg_fill = p.accent;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.primary);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, p.foreground);
    v.widgets.hovered.rounding = r;

    v.widgets.active.bg_fill = p.primary;
    v.widgets.active.weak_bg_fill = p.primary;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, p.primary);
    // NOTE: egui derives `strong()` text colour from this stroke, not just
    // pressed-button text. Setting it to `primary_foreground` (dark, meant
    // for text sitting on honey) makes every bold label near-invisible on a
    // dark card. Anything bold therefore sets its colour explicitly; see
    // `text::strong` below.
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, p.primary_foreground);
    v.widgets.active.rounding = r;

    v.widgets.open.bg_fill = p.secondary;
    v.widgets.open.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.open.rounding = r;

    ctx.set_visuals(v);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.menu_margin = egui::Margin::same(6.0);
    ctx.set_style(style);
}

/// Text helpers.
///
/// egui's `RichText::strong()` picks up the active widget stroke, which this
/// palette uses for text-on-honey. Bold text goes through here instead so it
/// always lands on the readable foreground colour.
pub mod text {
    use super::Palette;
    use egui::RichText;

    pub fn strong(p: &Palette, s: impl Into<String>) -> RichText {
        RichText::new(s).strong().color(p.foreground)
    }

    pub fn heading(p: &Palette, s: impl Into<String>) -> RichText {
        RichText::new(s).heading().color(p.foreground)
    }

    pub fn muted(p: &Palette, s: impl Into<String>) -> RichText {
        RichText::new(s).small().color(p.muted_foreground)
    }
}
