//! Window placement remembered across launches.
//!
//! macOS keeps its own frame through NSWindow's autosave, so the stored
//! placement is only consulted on the platforms that lack one.

#![cfg_attr(target_os = "macos", allow(dead_code))]

use std::sync::Mutex;

use gpui::{App, Bounds, Pixels, Point, Size, WindowBounds, px, size};
use serde::{Deserialize, Serialize};

use crate::editor::config_paths;

/// Latest placement seen while rendering, written once the app quits so a
/// drag or resize does not touch the disk on every frame.
static LAST_BOUNDS: Mutex<Option<WindowBounds>> = Mutex::new(None);

pub(crate) fn track(bounds: WindowBounds) {
    if let Ok(mut last) = LAST_BOUNDS.lock() {
        *last = Some(bounds);
    }
}

pub(crate) fn save_tracked() {
    let bounds = LAST_BOUNDS.lock().ok().and_then(|last| *last);
    if let Some(bounds) = bounds {
        save(bounds);
    }
}

#[derive(Serialize, Deserialize)]
struct StoredPlacement {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    #[serde(default)]
    maximized: bool,
}

fn placement_path() -> std::path::PathBuf {
    config_paths::config_dir().join("window.json")
}

/// Placement to open the main window with, or `None` to use the default.
pub(crate) fn restore(cx: &App) -> Option<WindowBounds> {
    if cfg!(target_os = "macos") {
        return None;
    }

    let raw = std::fs::read_to_string(placement_path()).ok()?;
    let stored: StoredPlacement = serde_json::from_str(&raw).ok()?;
    if !(stored.width.is_finite() && stored.height.is_finite())
        || stored.width < 1.0
        || stored.height < 1.0
    {
        return None;
    }

    let bounds = Bounds {
        origin: Point::new(px(stored.x), px(stored.y)),
        size: Size::new(px(stored.width), px(stored.height)),
    };
    // A monitor may be gone since the last run; anything off-screen is dropped.
    if !is_visible_on_some_display(bounds, cx) {
        return None;
    }

    Some(if stored.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    })
}

pub(crate) fn save(bounds: WindowBounds) {
    let (frame, maximized) = match bounds {
        WindowBounds::Windowed(frame) => (frame, false),
        WindowBounds::Maximized(frame) => (frame, true),
        // A fullscreen frame is not a useful thing to reopen into.
        WindowBounds::Fullscreen(_) => return,
    };

    let stored = StoredPlacement {
        x: frame.origin.x.into(),
        y: frame.origin.y.into(),
        width: frame.size.width.into(),
        height: frame.size.height.into(),
        maximized,
    };
    let Ok(serialized) = serde_json::to_string(&stored) else {
        return;
    };

    let path = placement_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = crate::editor::file_ops::atomic_write(&path, serialized.as_bytes());
}

fn is_visible_on_some_display(bounds: Bounds<Pixels>, cx: &App) -> bool {
    // Require a usable corner rather than any overlap, so a window cannot come
    // back with only a sliver on screen.
    const MIN_VISIBLE: Pixels = px(64.0);
    let visible: Size<Pixels> = size(MIN_VISIBLE, MIN_VISIBLE);

    cx.displays().iter().any(|display| {
        let display_bounds = display.bounds();
        intersection_size(bounds, display_bounds).is_some_and(|overlap| {
            overlap.width >= visible.width && overlap.height >= visible.height
        })
    })
}

fn intersection_size(a: Bounds<Pixels>, b: Bounds<Pixels>) -> Option<Size<Pixels>> {
    let left = a.origin.x.max(b.origin.x);
    let top = a.origin.y.max(b.origin.y);
    let right = (a.origin.x + a.size.width).min(b.origin.x + b.size.width);
    let bottom = (a.origin.y + a.size.height).min(b.origin.y + b.size.height);
    if right <= left || bottom <= top {
        return None;
    }
    Some(size(right - left, bottom - top))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds {
            origin: Point::new(px(x), px(y)),
            size: Size::new(px(width), px(height)),
        }
    }

    #[test]
    fn overlapping_rectangles_report_their_shared_area() {
        let overlap = intersection_size(
            bounds(0.0, 0.0, 100.0, 100.0),
            bounds(50.0, 50.0, 100.0, 100.0),
        );
        assert_eq!(overlap, Some(size(px(50.0), px(50.0))));
    }

    #[test]
    fn separated_rectangles_do_not_overlap() {
        assert_eq!(
            intersection_size(bounds(0.0, 0.0, 10.0, 10.0), bounds(20.0, 20.0, 10.0, 10.0)),
            None
        );
        // Touching edges share no area.
        assert_eq!(
            intersection_size(bounds(0.0, 0.0, 10.0, 10.0), bounds(10.0, 0.0, 10.0, 10.0)),
            None
        );
    }

    #[test]
    fn stored_placement_round_trips() {
        let stored = StoredPlacement {
            x: 12.0,
            y: 34.0,
            width: 1024.0,
            height: 768.0,
            maximized: true,
        };
        let raw = serde_json::to_string(&stored).unwrap();
        let parsed: StoredPlacement = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.x, 12.0);
        assert_eq!(parsed.height, 768.0);
        assert!(parsed.maximized);
    }
}
