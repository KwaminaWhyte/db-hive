//! The workspace's resizable panes: the schema sidebar's width and the SQL
//! editor's height.
//!
//! Both are dragged with a thin splitter bar between the panes. The clamp
//! lives here so the view cannot shrink a pane into nothing, and so the
//! saved values survive a hand-edited `layout.json`.

use serde::{Deserialize, Serialize};

pub const SIDEBAR_MIN: f32 = 200.0;
pub const SIDEBAR_MAX: f32 = 480.0;
pub const SIDEBAR_DEFAULT: f32 = 264.0;

pub const EDITOR_MIN: f32 = 120.0;
pub const EDITOR_MAX: f32 = 600.0;
pub const EDITOR_DEFAULT: f32 = 220.0;

/// The two pane sizes, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PaneSizes {
    pub sidebar: f32,
    pub editor: f32,
}

impl Default for PaneSizes {
    fn default() -> Self {
        Self {
            sidebar: SIDEBAR_DEFAULT,
            editor: EDITOR_DEFAULT,
        }
    }
}

impl PaneSizes {
    /// Clamp a sidebar width into the allowed range. Not-a-number falls back
    /// to the default rather than poisoning the layout.
    pub fn clamp_sidebar(width: f32) -> f32 {
        clamp(width, SIDEBAR_MIN, SIDEBAR_MAX, SIDEBAR_DEFAULT)
    }

    pub fn clamp_editor(height: f32) -> f32 {
        clamp(height, EDITOR_MIN, EDITOR_MAX, EDITOR_DEFAULT)
    }

    /// What the store held, clamped. A missing or malformed object is the
    /// default layout, not an error.
    pub fn from_json(value: Option<&serde_json::Value>) -> Self {
        let parsed = value
            .cloned()
            .and_then(|v| serde_json::from_value::<PaneSizes>(v).ok())
            .unwrap_or_default();
        Self {
            sidebar: Self::clamp_sidebar(parsed.sidebar),
            editor: Self::clamp_editor(parsed.editor),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

fn clamp(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if !value.is_finite() || value <= 0.0 {
        return fallback;
    }
    value.clamp(min, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_old_fixed_sizes() {
        let d = PaneSizes::default();
        assert_eq!(d.sidebar, 264.0);
        assert_eq!(d.editor, 220.0);
    }

    #[test]
    fn dragging_past_the_limits_stops_at_them() {
        assert_eq!(PaneSizes::clamp_sidebar(10.0), SIDEBAR_MIN);
        assert_eq!(PaneSizes::clamp_sidebar(9999.0), SIDEBAR_MAX);
        assert_eq!(PaneSizes::clamp_editor(1.0), EDITOR_MIN);
        assert_eq!(PaneSizes::clamp_editor(9999.0), EDITOR_MAX);
    }

    #[test]
    fn nonsense_values_fall_back_to_the_default() {
        assert_eq!(PaneSizes::clamp_sidebar(f32::NAN), SIDEBAR_DEFAULT);
        assert_eq!(PaneSizes::clamp_sidebar(0.0), SIDEBAR_DEFAULT);
        assert_eq!(PaneSizes::clamp_editor(-4.0), EDITOR_DEFAULT);
    }

    #[test]
    fn a_saved_layout_round_trips() {
        let sizes = PaneSizes {
            sidebar: 320.0,
            editor: 300.0,
        };
        let back = PaneSizes::from_json(Some(&sizes.to_json()));
        assert_eq!(back, sizes);
    }

    #[test]
    fn an_edited_file_is_clamped_not_rejected() {
        let json = serde_json::json!({"sidebar": 5.0, "editor": 9000.0});
        assert_eq!(
            PaneSizes::from_json(Some(&json)),
            PaneSizes {
                sidebar: SIDEBAR_MIN,
                editor: EDITOR_MAX
            }
        );
    }

    #[test]
    fn garbage_means_the_default_layout() {
        assert_eq!(
            PaneSizes::from_json(Some(&serde_json::json!("nope"))),
            PaneSizes::default()
        );
        assert_eq!(PaneSizes::from_json(None), PaneSizes::default());
    }
}
