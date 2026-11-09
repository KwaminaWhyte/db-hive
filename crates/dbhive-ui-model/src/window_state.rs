//! Per-window geometry, saved so each window reopens where it was.
//!
//! Every window has a label. The first window is `main` (the unsuffixed key,
//! the same one the old Tauri shell used for its first window); windows
//! opened later are `window-2`, `window-3` and so on, reusing a free number
//! when an earlier window closed.
//!
//! Sizes and positions are stored in logical pixels, not the physical ones
//! Slint reports. A window dragged between a Retina screen and a normal one
//! would otherwise come back at half or double its size.
//!
//! Nothing here reads the screen: a position that is merely off-screen
//! (a monitor was unplugged) is kept as saved, because the window manager
//! usually pulls a window back onto a visible display. Only values no
//! display could hold are dropped.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The smallest window the shell allows, matching the minimum inner size the
/// Slint window declares.
pub const MIN_WIDTH: u32 = 900;
pub const MIN_HEIGHT: u32 = 600;
/// No display is wider than this; anything larger is corruption.
pub const MAX_SIDE: u32 = 16_384;
/// Positions beyond this in either direction are corruption, not a monitor.
const POSITION_LIMIT: i32 = 32_768;

/// The label of the first window, kept unsuffixed for continuity.
pub const MAIN_LABEL: &str = "main";

/// One window's last geometry, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub maximized: bool,
}

impl WindowGeometry {
    /// Clamp a geometry read back from the window into a shape worth saving.
    ///
    /// Returns `None` when the size is unusable, which means "keep whatever
    /// the window manager decides" rather than writing a zero-sized window.
    pub fn sanitized(self) -> Option<Self> {
        if self.width < MIN_WIDTH || self.height < MIN_HEIGHT {
            // A window smaller than the minimum is almost always a window
            // that was never shown yet (Slint reports 0x0).
            return None;
        }
        let clamp_side = |v: u32| v.min(MAX_SIDE);
        let clamp_pos = |v: i32| v.clamp(-POSITION_LIMIT, POSITION_LIMIT);
        Some(Self {
            x: clamp_pos(self.x),
            y: clamp_pos(self.y),
            width: clamp_side(self.width),
            height: clamp_side(self.height),
            maximized: self.maximized,
        })
    }

    /// Save-side conversion: physical pixels to logical, for storage.
    pub fn from_physical(x: i32, y: i32, width: u32, height: u32, scale: f32) -> Self {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        Self {
            x: (x as f32 / scale).round() as i32,
            y: (y as f32 / scale).round() as i32,
            width: (width as f32 / scale).round().max(0.0) as u32,
            height: (height as f32 / scale).round().max(0.0) as u32,
            maximized: false,
        }
    }

    /// Restore-side conversion: logical pixels to physical.
    pub fn to_physical(self, scale: f32) -> (i32, i32, u32, u32) {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        (
            (self.x as f32 * scale).round() as i32,
            (self.y as f32 * scale).round() as i32,
            (self.width as f32 * scale).round().max(0.0) as u32,
            (self.height as f32 * scale).round().max(0.0) as u32,
        )
    }
}

/// Every window's geometry, keyed by label.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowStates {
    #[serde(default)]
    windows: BTreeMap<String, WindowGeometry>,
}

impl WindowStates {
    /// Parse what the store held. A bad entry is dropped on its own rather
    /// than losing every window's geometry.
    pub fn from_json(value: Option<&serde_json::Value>) -> Self {
        let Some(value) = value else {
            return Self::default();
        };
        let mut states: Self = serde_json::from_value(value.clone()).unwrap_or_default();
        states.windows.retain(|_, g| g.sanitized().is_some());
        states
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    pub fn geometry(&self, label: &str) -> Option<WindowGeometry> {
        self.windows.get(label).copied()
    }

    /// Record a window's geometry. Returns whether anything changed, so the
    /// caller only writes the file when it has to.
    pub fn record(&mut self, label: &str, geometry: WindowGeometry) -> bool {
        let Some(geometry) = geometry.sanitized() else {
            return false;
        };
        if self.windows.get(label) == Some(&geometry) {
            return false;
        }
        self.windows.insert(label.to_string(), geometry);
        true
    }

    pub fn remove(&mut self, label: &str) -> bool {
        self.windows.remove(label).is_some()
    }

    /// The next free label for a newly opened window: `window-2` after
    /// `main`, then the lowest free number.
    pub fn next_label(&self) -> String {
        let mut n = 2;
        loop {
            let candidate = format!("window-{n}");
            if !self.windows.contains_key(&candidate) {
                return candidate;
            }
            n += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geom(width: u32, height: u32) -> WindowGeometry {
        WindowGeometry {
            x: 40,
            y: 60,
            width,
            height,
            maximized: false,
        }
    }

    #[test]
    fn a_normal_geometry_round_trips() {
        let mut states = WindowStates::default();
        assert!(states.record(MAIN_LABEL, geom(1280, 800)));
        assert_eq!(states.geometry(MAIN_LABEL), Some(geom(1280, 800)));

        let json = states.to_json();
        let back = WindowStates::from_json(Some(&json));
        assert_eq!(back, states);
    }

    #[test]
    fn recording_the_same_geometry_reports_no_change() {
        let mut states = WindowStates::default();
        assert!(states.record(MAIN_LABEL, geom(1280, 800)));
        assert!(!states.record(MAIN_LABEL, geom(1280, 800)));
    }

    #[test]
    fn tiny_windows_are_not_saved() {
        let mut states = WindowStates::default();
        assert!(!states.record("main", geom(0, 0)));
        assert!(!states.record("main", geom(800, 800)));
        assert_eq!(states.geometry("main"), None);
    }

    #[test]
    fn absurd_sizes_and_positions_are_clamped() {
        let g = WindowGeometry {
            x: -900_000,
            y: 900_000,
            width: 99_999,
            height: 99_999,
            maximized: true,
        }
        .sanitized()
        .unwrap();
        assert_eq!(g.x, -POSITION_LIMIT);
        assert_eq!(g.y, POSITION_LIMIT);
        assert_eq!(g.width, MAX_SIDE);
        assert_eq!(g.height, MAX_SIDE);
        assert!(g.maximized);
    }

    #[test]
    fn one_bad_entry_does_not_lose_the_others() {
        let json = serde_json::json!({
            "windows": {
                "main": {"x": 0, "y": 0, "width": 1280, "height": 800, "maximized": false},
                "window-2": {"x": 0, "y": 0, "width": 10, "height": 10, "maximized": false}
            }
        });
        let states = WindowStates::from_json(Some(&json));
        assert!(states.geometry(MAIN_LABEL).is_some());
        assert!(states.geometry("window-2").is_none());
    }

    #[test]
    fn garbage_json_means_defaults_not_a_crash() {
        let json = serde_json::json!("not an object");
        assert_eq!(
            WindowStates::from_json(Some(&json)),
            WindowStates::default()
        );
        assert_eq!(WindowStates::from_json(None), WindowStates::default());
    }

    #[test]
    fn labels_are_reused_only_after_a_window_closes() {
        let mut states = WindowStates::default();
        assert_eq!(states.next_label(), "window-2");
        states.record("window-2", geom(1000, 700));
        assert_eq!(states.next_label(), "window-3");
        states.remove("window-2");
        assert_eq!(states.next_label(), "window-2");
    }

    #[test]
    fn logical_and_physical_pixels_convert_both_ways() {
        let physical = WindowGeometry::from_physical(80, 120, 2560, 1600, 2.0);
        assert_eq!(physical.x, 40);
        assert_eq!(physical.width, 1280);
        assert_eq!(physical.to_physical(2.0), (80, 120, 2560, 1600));
    }

    #[test]
    fn a_zero_scale_factor_is_treated_as_one() {
        let g = WindowGeometry::from_physical(10, 20, 1000, 700, 0.0);
        assert_eq!((g.x, g.y, g.width, g.height), (10, 20, 1000, 700));
        assert_eq!(g.to_physical(0.0), (10, 20, 1000, 700));
    }

    #[test]
    fn maximized_is_saved_but_does_not_affect_size() {
        let mut states = WindowStates::default();
        let mut g = geom(1280, 800);
        g.maximized = true;
        states.record(MAIN_LABEL, g);
        assert!(states.geometry(MAIN_LABEL).unwrap().maximized);
        assert_eq!(states.geometry(MAIN_LABEL).unwrap().width, 1280);
    }
}
