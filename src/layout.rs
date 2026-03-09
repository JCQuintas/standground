use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::display::{get_current_configuration, DisplayConfiguration};
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

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct WindowMatchKey {
    pub bundle_id: String,
    pub window_title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedWindow {
    pub bundle_id: String,
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
    pub windows: Vec<SavedWindow>,
    pub saved_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LayoutStore {
    pub layouts: HashMap<String, SavedLayout>,
}

pub fn save_current_layout(store: &mut LayoutStore) -> Result<usize, String> {
    let config = get_current_configuration()?;
    let windows = enumerate_windows();

    // Build a map from raw space_id → ordinal index so we store
    // a stable reference that survives reboots.
    let (all_spaces, _) = get_all_space_ids();
    let space_id_to_index: HashMap<u64, usize> = all_spaces
        .iter()
        .enumerate()
        .map(|(i, &sid)| (sid, i))
        .collect();

    let saved_windows: Vec<SavedWindow> = windows
        .iter()
        .map(|w| SavedWindow {
            bundle_id: w.bundle_id.clone(),
            window_title: w.window_title.clone(),
            bounds: w.bounds.clone(),
            space_index: space_id_to_index.get(&w.space_id).copied().unwrap_or(0),
        })
        .collect();

    let count = saved_windows.len();
    let layout = SavedLayout {
        display_config: config.clone(),
        windows: saved_windows,
        saved_at: Utc::now(),
    };

    store.layouts.insert(config.config_key(), layout);
    Ok(count)
}

pub fn restore_layout(store: &LayoutStore) -> Result<(usize, usize), String> {
    let config = get_current_configuration()?;
    let key = config.config_key();

    let layout = store
        .layouts
        .get(&key)
        .ok_or_else(|| "No saved layout for current display configuration".to_string())?;

    let total = layout.windows.len();

    // Get all spaces in order and display UUID for switching
    let (all_spaces, display_uuid) = get_all_space_ids();
    let display_uuid = display_uuid.unwrap_or_default();
    let original_space = get_active_space();

    // Build global lookup indexes from all saved windows
    let mut by_key: HashMap<WindowMatchKey, &SavedWindow> = HashMap::new();
    let mut by_bundle: HashMap<String, Vec<&SavedWindow>> = HashMap::new();
    for sw in &layout.windows {
        let key = WindowMatchKey {
            bundle_id: sw.bundle_id.clone(),
            window_title: sw.window_title.clone(),
        };
        by_key.insert(key, sw);
        by_bundle.entry(sw.bundle_id.clone()).or_default().push(sw);
    }

    // Enumerate all current windows and match them to saved windows.
    // Move any that are on the wrong space before repositioning.
    let current_windows = enumerate_windows();

    for w in &current_windows {
        if let Some(saved) = find_matching_saved(w, &by_key, &by_bundle) {
            let target_space = all_spaces.get(saved.space_index).copied().unwrap_or(0);
            if target_space != 0 && w.space_id != 0 && w.space_id != target_space {
                move_window_to_space(w.window_id, w.space_id, target_space);
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

        // Build lookup indexes for this space's saved windows
        let mut space_by_key: HashMap<WindowMatchKey, &SavedWindow> = HashMap::new();
        let mut space_by_bundle: HashMap<String, Vec<&SavedWindow>> = HashMap::new();
        for sw in saved_windows {
            let key = WindowMatchKey {
                bundle_id: sw.bundle_id.clone(),
                window_title: sw.window_title.clone(),
            };
            space_by_key.insert(key, sw);
            space_by_bundle
                .entry(sw.bundle_id.clone())
                .or_default()
                .push(sw);
        }

        // Re-enumerate windows (they may have moved spaces)
        let current_windows = enumerate_windows();

        for w in &current_windows {
            let saved = find_matching_saved(w, &space_by_key, &space_by_bundle);

            if let Some(saved) = saved {
                if set_window_position(w.pid, w.window_id, &saved.bounds) {
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

/// Match a current window to a saved window.
/// Priority: exact (bundle_id + title), then bundle_id-only if unambiguous.
fn find_matching_saved<'a>(
    w: &WindowInfo,
    by_key: &HashMap<WindowMatchKey, &'a SavedWindow>,
    by_bundle: &HashMap<String, Vec<&'a SavedWindow>>,
) -> Option<&'a SavedWindow> {
    let exact_key = WindowMatchKey {
        bundle_id: w.bundle_id.clone(),
        window_title: w.window_title.clone(),
    };

    if let Some(sw) = by_key.get(&exact_key) {
        return Some(sw);
    }

    if let Some(saved_list) = by_bundle.get(&w.bundle_id) {
        if saved_list.len() == 1 {
            return Some(saved_list[0]);
        }
    }

    None
}
