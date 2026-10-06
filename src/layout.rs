use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};

use crate::display::{get_current_configuration, get_current_display_frames, DisplayConfiguration, DisplayFrame};
use crate::window::{
    enumerate_windows, get_active_space, get_all_space_ids, move_window_to_space,
    set_window_position, switch_to_space, WindowInfo,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedWindow {
    pub bundle_id: String,
    /// CGWindowID at save time. Exact match while the window stays open
    /// (dock/undock); meaningless after the app or system restarts.
    #[serde(default)]
    pub window_id: u32,
    pub window_title: String,
    pub bounds: WindowBounds,
    /// Ordinal index of the space (0-based) — stable across reboots,
    /// unlike the raw CGS space ID which is reassigned on every login.
    #[serde(alias = "space_id")]
    pub space_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedLayout {
    pub display_config: DisplayConfiguration,
    #[serde(default)]
    pub display_frames: Vec<DisplayFrame>,
    pub windows: Vec<SavedWindow>,
    pub saved_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LayoutStore {
    #[serde(default, deserialize_with = "deserialize_layouts")]
    pub layouts: HashMap<String, Vec<SavedLayout>>,
}

/// Deserialize layouts supporting both old format (single SavedLayout per key)
/// and new format (Vec<SavedLayout> per key).
fn deserialize_layouts<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, Vec<SavedLayout>>, D::Error>
where
    D: Deserializer<'de>,
{
    let map: HashMap<String, serde_json::Value> = HashMap::deserialize(deserializer)?;
    let mut result = HashMap::new();
    for (key, value) in map {
        let layouts = if value.is_array() {
            serde_json::from_value(value).map_err(serde::de::Error::custom)?
        } else {
            let single: SavedLayout =
                serde_json::from_value(value).map_err(serde::de::Error::custom)?;
            vec![single]
        };
        result.insert(key, layouts);
    }
    Ok(result)
}

pub fn save_current_layout(store: &mut LayoutStore) -> Result<usize, String> {
    let config = get_current_configuration()?;
    let display_frames = get_current_display_frames().unwrap_or_default();
    let windows = enumerate_windows();

    // Build a map from raw space_id → ordinal index so we store
    // a stable reference that survives reboots.
    let (all_spaces, _) = get_all_space_ids();
    let space_id_to_index: HashMap<u64, usize> = all_spaces
        .iter()
        .enumerate()
        .map(|(i, &sid)| (sid, i))
        .collect();

    // Windows on no Space are helpers (popups, placeholders, autofill), not real windows.
    let saved_windows: Vec<SavedWindow> = windows
        .iter()
        .filter(|w| w.space_id != 0)
        .map(|w| SavedWindow {
            bundle_id: w.bundle_id.clone(),
            window_id: w.window_id,
            window_title: w.window_title.clone(),
            bounds: w.bounds.clone(),
            space_index: space_id_to_index.get(&w.space_id).copied().unwrap_or(0),
        })
        .collect();

    let count = saved_windows.len();
    let layout = SavedLayout {
        display_config: config.clone(),
        display_frames,
        windows: saved_windows,
        saved_at: Utc::now(),
    };

    store.layouts.entry(config.config_key()).or_default().push(layout);
    Ok(count)
}

/// Get all saved layouts across all display configurations, sorted newest first.
/// Returns (config_key, layout) pairs.
pub fn get_all_layouts(store: &LayoutStore) -> Vec<(String, &SavedLayout)> {
    let mut all: Vec<_> = store
        .layouts
        .iter()
        .flat_map(|(key, layouts)| layouts.iter().map(move |l| (key.clone(), l)))
        .collect();
    all.sort_by(|a, b| b.1.saved_at.cmp(&a.1.saved_at));
    all
}

/// Restore the most recent layout for the current display configuration.
pub fn restore_layout(store: &LayoutStore) -> Result<(usize, usize), String> {
    let config = get_current_configuration()?;
    let key = config.config_key();

    let layouts = store
        .layouts
        .get(&key)
        .ok_or_else(|| "No saved layout for current display configuration".to_string())?;

    let layout = layouts
        .iter()
        .max_by_key(|l| l.saved_at)
        .ok_or_else(|| "No saved layout for current display configuration".to_string())?;

    restore_saved_layout(layout)
}

/// Restore a specific saved layout.
pub fn restore_saved_layout(layout: &SavedLayout) -> Result<(usize, usize), String> {
    let total = layout.windows.len();

    // Build display mapping to remap bounds for missing/changed displays
    let current_frames = get_current_display_frames().unwrap_or_default();
    let display_mapping = build_display_mapping(&layout.display_frames, &current_frames);

    // Get all spaces in order and display UUID for switching
    let (all_spaces, display_uuid) = get_all_space_ids();
    let display_uuid = display_uuid.unwrap_or_default();
    let original_space = get_active_space();

    // Enumerate all current windows and match them to saved windows.
    // Move any that are on the wrong space before repositioning.
    let all_saved: Vec<&SavedWindow> = layout.windows.iter().collect();
    let current_windows = enumerate_windows();

    for w in &current_windows {
        if let Some(saved) = find_matching_saved(w, &all_saved, &current_windows) {
            let target_space = all_spaces.get(saved.space_index).copied().unwrap_or(0);
            if target_space != 0 && w.space_id != 0 && w.space_id != target_space {
                move_window_to_space(w.window_id, target_space);
            }
        }
    }

    // Small delay to let space moves settle
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Now reposition windows: group saved windows by target space,
    // switch to each space to set positions.
    let mut saved_by_space: HashMap<u64, Vec<&SavedWindow>> = HashMap::new();
    for sw in &layout.windows {
        let space_id = all_spaces.get(sw.space_index).copied().unwrap_or(0);
        if space_id != 0 {
            saved_by_space.entry(space_id).or_default().push(sw);
        }
    }

    let mut restored = 0;

    for &space_id in &all_spaces {
        let saved_windows = match saved_by_space.get(&space_id) {
            Some(ws) => ws,
            None => continue,
        };

        // Switch to this space
        if !display_uuid.is_empty() && space_id != get_active_space() {
            switch_to_space(space_id, &display_uuid);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        // Re-enumerate windows (they may have moved spaces)
        let current_windows = enumerate_windows();

        for w in &current_windows {
            let saved = find_matching_saved(w, saved_windows, &current_windows);

            if let Some(saved) = saved {
                let adjusted = adjust_bounds(
                    &saved.bounds,
                    &layout.display_frames,
                    &current_frames,
                    &display_mapping,
                );
                if set_window_position(w.pid, w.window_id, &adjusted) {
                    restored += 1;
                }
            }
        }
    }

    // Switch back to the original space
    if !display_uuid.is_empty() && get_active_space() != original_space {
        switch_to_space(original_space, &display_uuid);
    }

    Ok((restored, total))
}

/// Build a mapping from saved display indices to current display indices.
/// Pass 1: exact fingerprint matches.
/// Pass 2: pair remaining saved displays with remaining current displays by closest resolution.
/// Pass 3: any still-unmatched saved displays map to the closest current display (shared).
fn build_display_mapping(
    saved_frames: &[DisplayFrame],
    current_frames: &[DisplayFrame],
) -> Vec<Option<usize>> {
    let mut result = vec![None; saved_frames.len()];
    let mut used = vec![false; current_frames.len()];

    // Pass 1: exact fingerprint matches
    for (si, sf) in saved_frames.iter().enumerate() {
        for (ci, cf) in current_frames.iter().enumerate() {
            if !used[ci] && sf.fingerprint == cf.fingerprint {
                result[si] = Some(ci);
                used[ci] = true;
                break;
            }
        }
    }

    // Pass 2: pair unmatched saved with unmatched current by closest resolution
    for si in 0..saved_frames.len() {
        if result[si].is_some() {
            continue;
        }
        let saved_pixels = saved_frames[si].width * saved_frames[si].height;
        let mut best = None;
        let mut best_diff = f64::MAX;
        for (ci, cf) in current_frames.iter().enumerate() {
            if used[ci] {
                continue;
            }
            let diff = (saved_pixels - cf.width * cf.height).abs();
            if diff < best_diff {
                best_diff = diff;
                best = Some(ci);
            }
        }
        if let Some(ci) = best {
            result[si] = Some(ci);
            used[ci] = true;
        }
    }

    // Pass 3: still-unmatched saved → closest current display (even if already used)
    for si in 0..saved_frames.len() {
        if result[si].is_some() || current_frames.is_empty() {
            continue;
        }
        let saved_pixels = saved_frames[si].width * saved_frames[si].height;
        let ci = current_frames
            .iter()
            .enumerate()
            .min_by_key(|(_, cf)| ((saved_pixels - cf.width * cf.height).abs() * 1000.0) as i64)
            .map(|(i, _)| i)
            .unwrap_or(0);
        result[si] = Some(ci);
    }

    result
}

/// Find which saved display frame contains a window's top-left corner.
fn find_display_for_bounds(frames: &[DisplayFrame], bounds: &WindowBounds) -> Option<usize> {
    frames.iter().position(|f| {
        bounds.x >= f.x
            && bounds.x < f.x + f.width
            && bounds.y >= f.y
            && bounds.y < f.y + f.height
    })
}

/// Proportionally remap window bounds from one display frame to another.
fn remap_bounds(bounds: &WindowBounds, from: &DisplayFrame, to: &DisplayFrame) -> WindowBounds {
    let rel_x = (bounds.x - from.x) / from.width;
    let rel_y = (bounds.y - from.y) / from.height;
    let rel_w = bounds.width / from.width;
    let rel_h = bounds.height / from.height;

    WindowBounds {
        x: to.x + rel_x * to.width,
        y: to.y + rel_y * to.height,
        width: rel_w * to.width,
        height: rel_h * to.height,
    }
}

/// Adjust window bounds using the display mapping. Falls back to original bounds
/// if no display frames are available (backward compat with old layouts).
fn adjust_bounds(
    bounds: &WindowBounds,
    saved_frames: &[DisplayFrame],
    current_frames: &[DisplayFrame],
    mapping: &[Option<usize>],
) -> WindowBounds {
    if saved_frames.is_empty() || current_frames.is_empty() {
        return bounds.clone();
    }

    if let Some(si) = find_display_for_bounds(saved_frames, bounds) {
        if let Some(Some(ci)) = mapping.get(si) {
            return remap_bounds(bounds, &saved_frames[si], &current_frames[*ci]);
        }
    }

    bounds.clone()
}

/// Delete a saved layout identified by its config key and timestamp.
pub fn delete_layout(
    store: &mut LayoutStore,
    config_key: &str,
    saved_at: DateTime<Utc>,
) -> Result<(), String> {
    let layouts = store
        .layouts
        .get_mut(config_key)
        .ok_or_else(|| "Layout config not found".to_string())?;

    let idx = layouts
        .iter()
        .position(|l| l.saved_at == saved_at)
        .ok_or_else(|| "Layout not found".to_string())?;

    layouts.remove(idx);

    if layouts.is_empty() {
        store.layouts.remove(config_key);
    }

    Ok(())
}

/// Match a current window to a saved window.
/// Priority: same window ID, then same non-empty title, then bundle_id-only
/// if the app restarted since saving and had a single saved window.
fn find_matching_saved<'a>(
    w: &WindowInfo,
    saved: &[&'a SavedWindow],
    current: &[WindowInfo],
) -> Option<&'a SavedWindow> {
    let same_app = || saved.iter().copied().filter(|s| s.bundle_id == w.bundle_id);

    if let Some(s) = same_app().find(|s| s.window_id != 0 && s.window_id == w.window_id) {
        return Some(s);
    }

    if !w.window_title.is_empty() {
        if let Some(s) = same_app().find(|s| s.window_title == w.window_title) {
            return Some(s);
        }
    }

    // Titles are empty without Screen Recording, so they can't tell same-app
    // windows apart. If any saved ID is still open, unmatched windows are new;
    // and guessing between several saved windows stacks them all on one frame.
    let still_open = same_app().any(|s| {
        current
            .iter()
            .any(|c| c.window_id == s.window_id && c.bundle_id == s.bundle_id)
    });
    let mut candidates = same_app();
    match (candidates.next(), candidates.next()) {
        (Some(s), None) if !still_open => Some(s),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(bundle: &str, id: u32, height: f64) -> SavedWindow {
        SavedWindow {
            bundle_id: bundle.into(),
            window_id: id,
            window_title: String::new(),
            bounds: WindowBounds {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height,
            },
            space_index: 0,
        }
    }

    fn live(bundle: &str, id: u32) -> WindowInfo {
        WindowInfo {
            pid: 1,
            window_id: id,
            bundle_id: bundle.into(),
            window_title: String::new(),
            bounds: WindowBounds {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            space_id: 1,
        }
    }

    #[test]
    fn matches_untitled_windows_without_collapsing() {
        // Real window plus a popup helper saved after it, both untitled (macOS 27 Chrome).
        let real = saved("chrome", 10, 1071.0);
        let popup = saved("chrome", 11, 139.0);
        let slack = saved("slack", 7, 847.0);
        let all = [&real, &popup, &slack];
        let current = [live("chrome", 10), live("chrome", 99), live("slack", 5)];
        let height = |w| find_matching_saved(w, &all, &current).map(|s| s.bounds.height);

        assert_eq!(height(&current[0]), Some(1071.0)); // same ID wins
        assert_eq!(height(&current[1]), None); // new window of a still-running app
        assert_eq!(height(&current[2]), Some(847.0)); // restarted app, single saved window

        // Old layouts (no IDs) with several same-app entries: leave the window alone.
        let (a, b) = (saved("chrome", 0, 1071.0), saved("chrome", 0, 139.0));
        assert!(find_matching_saved(&current[1], &[&a, &b], &current).is_none());
    }
}
