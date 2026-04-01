use std::collections::BTreeSet;
use std::sync::mpsc;

use core_graphics::display::CGDisplay;
use serde::{Deserialize, Serialize};

type CGError = i32;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct DisplayFingerprint {
    pub vendor_id: u32,
    pub model_id: u32,
    pub serial_number: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct DisplayConfiguration(pub BTreeSet<DisplayFingerprint>);

impl DisplayConfiguration {
    pub fn config_key(&self) -> String {
        serde_json::to_string(&self.0).unwrap_or_default()
    }

    /// Human-readable label from display resolutions, e.g. "1800×1169 + 1920×1200".
    pub fn display_label(&self) -> String {
        self.0
            .iter()
            .map(|f| format!("{}×{}", f.width, f.height))
            .collect::<Vec<_>>()
            .join(" + ")
    }
}

pub fn get_current_configuration() -> Result<DisplayConfiguration, String> {
    let display_ids =
        CGDisplay::active_displays().map_err(|e| format!("Failed to get active displays: {e}"))?;

    let mut fingerprints = BTreeSet::new();
    for &display_id in &display_ids {
        let display = CGDisplay::new(display_id);
        let bounds = display.bounds();
        fingerprints.insert(DisplayFingerprint {
            vendor_id: display.vendor_number(),
            model_id: display.model_number(),
            serial_number: display.serial_number(),
            width: bounds.size.width as u32,
            height: bounds.size.height as u32,
        });
    }

    Ok(DisplayConfiguration(fingerprints))
}

#[derive(Debug)]
pub enum DisplayEvent {
    ConfigurationChanged,
}

extern "C" {
    fn CGDisplayRegisterReconfigurationCallback(
        callback: extern "C" fn(display: u32, flags: u32, user_info: *mut std::ffi::c_void),
        user_info: *mut std::ffi::c_void,
    ) -> CGError;
}

const K_CG_DISPLAY_BEGIN_CONFIGURATION_FLAG: u32 = 1;

extern "C" fn display_reconfiguration_callback(
    _display: u32,
    flags: u32,
    user_info: *mut std::ffi::c_void,
) {
    // Only act on end of reconfiguration (when begin flag is NOT set)
    if flags & K_CG_DISPLAY_BEGIN_CONFIGURATION_FLAG != 0 {
        return;
    }

    let sender = unsafe { &*(user_info as *const mpsc::Sender<DisplayEvent>) };
    let _ = sender.send(DisplayEvent::ConfigurationChanged);
}

pub fn register_display_callback(sender: mpsc::Sender<DisplayEvent>) -> Result<(), String> {
    let sender_box = Box::new(sender);
    let sender_ptr = Box::into_raw(sender_box) as *mut std::ffi::c_void;

    unsafe {
        let err =
            CGDisplayRegisterReconfigurationCallback(display_reconfiguration_callback, sender_ptr);
        if err != 0 {
            // Reclaim the box to avoid leak
            let _ = Box::from_raw(sender_ptr as *mut mpsc::Sender<DisplayEvent>);
            return Err(format!("Failed to register display callback: {err}"));
        }
    }

    // Intentionally leak the sender - it must live for the lifetime of the app
    Ok(())
}
