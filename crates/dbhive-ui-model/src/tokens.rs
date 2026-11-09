//! Design tokens: colours, radii and the type scale.
//!
//! The single source of truth for how DB Hive looks. The theme builds
//! its visuals from these, and the Slint shell generates its `Theme` global
//! from them at build time, so the app and the contrast tests below,
//! always see the same values.

/// An sRGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#rrggbb`, for generated Slint.
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

pub struct Palette {
    // Surfaces, lightest-on-dark last.
    pub background: Rgb,
    pub sidebar: Rgb,
    pub card: Rgb,
    pub elevated: Rgb,
    pub popover: Rgb,

    // Text, in descending emphasis.
    pub foreground: Rgb,
    pub muted_foreground: Rgb,
    pub dim_foreground: Rgb,

    // Brand.
    pub primary: Rgb,
    pub primary_foreground: Rgb,
    /// Primary at low opacity, for selected rows and subtle highlights.
    pub primary_soft: Rgb,

    // Interaction.
    pub secondary: Rgb,
    pub hover: Rgb,
    pub pressed: Rgb,

    // Lines.
    pub border: Rgb,
    pub border_strong: Rgb,

    // Status, each with a tint for banner backgrounds.
    pub destructive: Rgb,
    pub destructive_soft: Rgb,
    pub warning: Rgb,
    pub warning_soft: Rgb,
    pub success: Rgb,
    pub success_soft: Rgb,

    /// Kept for compatibility with existing call sites.
    pub muted: Rgb,
    pub accent: Rgb,
}

pub const DARK: Palette = Palette {
    background: Rgb(9, 10, 14),
    sidebar: Rgb(13, 14, 19),
    card: Rgb(19, 20, 26),
    elevated: Rgb(26, 28, 35),
    popover: Rgb(24, 25, 32),

    foreground: Rgb(240, 241, 244),
    muted_foreground: Rgb(150, 153, 162),
    dim_foreground: Rgb(104, 108, 118),

    primary: Rgb(248, 176, 42),
    primary_foreground: Rgb(24, 17, 2),
    primary_soft: Rgb(48, 39, 18),

    secondary: Rgb(30, 32, 40),
    hover: Rgb(38, 41, 50),
    pressed: Rgb(46, 49, 60),

    border: Rgb(38, 40, 49),
    border_strong: Rgb(58, 61, 72),

    destructive: Rgb(248, 113, 113),
    destructive_soft: Rgb(56, 26, 28),
    warning: Rgb(240, 189, 70),
    warning_soft: Rgb(54, 42, 15),
    success: Rgb(74, 205, 122),
    success_soft: Rgb(20, 48, 32),

    muted: Rgb(30, 32, 40),
    accent: Rgb(38, 41, 50),
};

pub const LIGHT: Palette = Palette {
    // Light mode inverts how depth works: the page is grey and content is
    // white, and the topmost layer is separated by shadow rather than by a
    // lighter fill, because there is nothing lighter than white.
    background: Rgb(245, 246, 249),
    sidebar: Rgb(239, 241, 245),
    card: Rgb(255, 255, 255),
    elevated: Rgb(255, 255, 255),
    popover: Rgb(255, 255, 255),

    foreground: Rgb(17, 19, 24),
    muted_foreground: Rgb(98, 103, 113),
    dim_foreground: Rgb(142, 147, 158),

    primary: Rgb(226, 148, 12),
    // Dark text on honey, not white: white on this amber is 3.2:1, which is
    // below the readable floor. It also matches the brand, where the primary
    // button has always been dark text on gold.
    primary_foreground: Rgb(28, 18, 0),
    primary_soft: Rgb(254, 243, 220),

    secondary: Rgb(241, 243, 246),
    hover: Rgb(236, 238, 242),
    pressed: Rgb(226, 229, 235),

    border: Rgb(224, 227, 232),
    border_strong: Rgb(198, 203, 212),

    destructive: Rgb(198, 40, 40),
    destructive_soft: Rgb(253, 235, 235),
    warning: Rgb(140, 88, 0),
    warning_soft: Rgb(254, 246, 227),
    success: Rgb(21, 128, 61),
    success_soft: Rgb(233, 248, 238),

    muted: Rgb(241, 243, 246),
    accent: Rgb(236, 238, 242),
};

/// Corner radii, in logical pixels.
pub mod radius {
    pub const CONTROL: f32 = 7.0;
    pub const CARD: f32 = 10.0;
    pub const WINDOW: f32 = 12.0;
    pub const PILL: f32 = 999.0;
}

/// Type scale, in logical pixels.
pub mod size {
    pub const DISPLAY: f32 = 22.0;
    pub const HEADING: f32 = 17.0;
    pub const SUBHEADING: f32 = 14.5;
    pub const BODY: f32 = 13.5;
    pub const LABEL: f32 = 12.5;
    pub const SMALL: f32 = 11.5;
    pub const MONO: f32 = 12.5;
}

/// Every palette field, by name, for code generation.
pub fn fields(p: &Palette) -> Vec<(&'static str, Rgb)> {
    vec![
        ("background", p.background),
        ("sidebar", p.sidebar),
        ("card", p.card),
        ("elevated", p.elevated),
        ("popover", p.popover),
        ("foreground", p.foreground),
        ("muted_foreground", p.muted_foreground),
        ("dim_foreground", p.dim_foreground),
        ("primary", p.primary),
        ("primary_foreground", p.primary_foreground),
        ("primary_soft", p.primary_soft),
        ("secondary", p.secondary),
        ("hover", p.hover),
        ("pressed", p.pressed),
        ("border", p.border),
        ("border_strong", p.border_strong),
        ("destructive", p.destructive),
        ("destructive_soft", p.destructive_soft),
        ("warning", p.warning),
        ("warning_soft", p.warning_soft),
        ("success", p.success),
        ("success_soft", p.success_soft),
        ("muted", p.muted),
        ("accent", p.accent),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Rgb) -> f32 {
        let f = |v: u8| {
            let s = v as f32 / 255.0;
            if s <= 0.03928 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.0) + 0.7152 * f(c.1) + 0.0722 * f(c.2)
    }

    fn contrast(a: Rgb, b: Rgb) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        let (hi, lo) = if x > y { (x, y) } else { (y, x) };
        (hi + 0.05) / (lo + 0.05)
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

    #[test]
    fn fields_lists_every_palette_colour() {
        // A field missing here would silently not reach the generated
        // Slint theme.
        let src = include_str!("tokens.rs");
        let declared = src
            .split("pub struct Palette {")
            .nth(1)
            .unwrap()
            .split('}')
            .next()
            .unwrap()
            .matches(": Rgb,")
            .count();
        assert_eq!(fields(&DARK).len(), declared);
    }
}
