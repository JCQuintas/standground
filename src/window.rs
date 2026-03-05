use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_graphics::window::{kCGWindowListExcludeDesktopElements, kCGWindowListOptionAll};

use std::collections::HashMap;

use crate::layout::WindowBounds;

#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub pid: i64,
    pub window_id: u32,
    pub bundle_id: String,
    pub window_title: String,
    pub bounds: WindowBounds,
    pub space_id: u64,
}

// System processes to exclude
const EXCLUDED_OWNERS: &[&str] = &[
    "Window Server",
    "WindowManager",
    "Dock",
    "SystemUIServer",
    "Control Center",
    "Notification Center",
    "Spotlight",
];

// Private CGS types
type CGSConnectionID = i32;

extern "C" {
    fn CGWindowListCopyWindowInfo(
        option: u32,
        relative_to_window: u32,
    ) -> core_foundation::base::CFTypeRef;
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;

    // Private CGS APIs for Spaces
    fn CGSMainConnectionID() -> CGSConnectionID;
    fn CGSCopySpacesForWindows(
        cid: CGSConnectionID,
        selector: i32,
        window_array: core_foundation::base::CFTypeRef,
    ) -> core_foundation::base::CFTypeRef;
    fn CGSGetActiveSpace(cid: CGSConnectionID) -> u64;
    fn CGSManagedDisplaySetCurrentSpace(
        cid: CGSConnectionID,
        display: core_foundation::base::CFTypeRef,
        space_id: u64,
    );
    fn CGSCopyManagedDisplaySpaces(cid: CGSConnectionID) -> core_foundation::base::CFTypeRef;
}

/// Get the Space ID for a given CGWindowID using private CGS API.
unsafe fn get_space_for_window(cid: CGSConnectionID, window_id: u32) -> u64 {
    let num = CFNumber::from(window_id as i32);
    let arr = core_foundation::array::CFArray::from_CFTypes(&[num.as_CFType()]);

    // selector 7 = all spaces (current + others)
    let spaces_ref = CGSCopySpacesForWindows(
        cid,
        7,
        arr.as_concrete_TypeRef() as core_foundation::base::CFTypeRef,
    );
    if spaces_ref.is_null() {
        return 0;
    }

    let count = CFArrayGetCount(spaces_ref);
    if count == 0 {
        core_foundation::base::CFRelease(spaces_ref);
        return 0;
    }

    let value = CFArrayGetValueAtIndex(spaces_ref, 0);
    if value.is_null() {
        core_foundation::base::CFRelease(spaces_ref);
        return 0;
    }

    let mut result: i64 = 0;
    let got = CFNumberGetValue(
        value,
        4, /* kCFNumberSInt64Type */
        &mut result as *mut _ as *mut _,
    );
    if !got {
        let mut result32: i32 = 0;
        if CFNumberGetValue(
            value,
            3, /* kCFNumberSInt32Type */
            &mut result32 as *mut _ as *mut _,
        ) {
            result = result32 as i64;
        }
    }

    core_foundation::base::CFRelease(spaces_ref);
    result as u64
}

/// Get the currently active space ID.
pub fn get_active_space() -> u64 {
    unsafe {
        let cid = CGSMainConnectionID();
        CGSGetActiveSpace(cid)
    }
}

/// Get all space IDs in order, and the display UUID for the main display.
pub fn get_all_space_ids() -> (Vec<u64>, Option<String>) {
    unsafe {
        let cid = CGSMainConnectionID();
        let displays_ref = CGSCopyManagedDisplaySpaces(cid);
        if displays_ref.is_null() {
            return (vec![], None);
        }

        let display_count = CFArrayGetCount(displays_ref);
        let mut all_spaces = Vec::new();
        let mut display_uuid = None;

        for d in 0..display_count {
            let display_dict = CFArrayGetValueAtIndex(displays_ref, d);
            if display_dict.is_null() {
                continue;
            }

            // Get display UUID (needed for CGSManagedDisplaySetCurrentSpace)
            if display_uuid.is_none() {
                display_uuid = cfdict_get_string(display_dict, "Display Identifier");
            }

            // Get "Spaces" array from this display
            let spaces_arr = match cfdict_get_raw(display_dict, "Spaces") {
                Some(v) => v,
                None => continue,
            };

            let spaces_count = CFArrayGetCount(spaces_arr);
            for s in 0..spaces_count {
                let space_dict = CFArrayGetValueAtIndex(spaces_arr, s);
                if space_dict.is_null() {
                    continue;
                }
                // Space ID is stored as "id64" in newer macOS, "ManagedSpaceID" in older
                if let Some(id) = cfdict_get_i64(space_dict, "id64") {
                    all_spaces.push(id as u64);
                } else if let Some(id) = cfdict_get_i64(space_dict, "ManagedSpaceID") {
                    all_spaces.push(id as u64);
                }
            }
        }

        core_foundation::base::CFRelease(displays_ref);
        (all_spaces, display_uuid)
    }
}

/// Switch to a specific space on the main display.
pub fn switch_to_space(space_id: u64, display_uuid: &str) {
    unsafe {
        let cid = CGSMainConnectionID();
        let uuid = CFString::new(display_uuid);
        CGSManagedDisplaySetCurrentSpace(
            cid,
            uuid.as_concrete_TypeRef() as core_foundation::base::CFTypeRef,
            space_id,
        );
    }
}

fn get_bundle_id_for_pid(pid: i64) -> Option<String> {
    use objc2_app_kit::NSRunningApplication;

    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32);
    let app = app?;
    let bundle_id = app.bundleIdentifier()?;
    Some(bundle_id.to_string())
}

/// Extract a number from a CFNumber as f64. Tries float64 then int32.
unsafe fn cfnumber_to_f64(value: core_foundation::base::CFTypeRef) -> Option<f64> {
    let mut result_f64: f64 = 0.0;
    if CFNumberGetValue(
        value,
        13, /* kCFNumberFloat64Type */
        &mut result_f64 as *mut _ as *mut _,
    ) {
        return Some(result_f64);
    }
    let mut result_i32: i32 = 0;
    if CFNumberGetValue(
        value,
        3, /* kCFNumberSInt32Type */
        &mut result_i32 as *mut _ as *mut _,
    ) {
        return Some(result_i32 as f64);
    }
    None
}

/// Look up a key in a CFDictionary and return the raw value ref.
unsafe fn cfdict_get_raw(
    dict: core_foundation::base::CFTypeRef,
    key: &str,
) -> Option<core_foundation::base::CFTypeRef> {
    let cf_key = CFString::new(key);
    let mut value: core_foundation::base::CFTypeRef = std::ptr::null();
    if CFDictionaryGetValueIfPresent(dict, cf_key.as_concrete_TypeRef() as *const _, &mut value)
        == 0
    {
        return None;
    }
    if value.is_null() {
        None
    } else {
        Some(value)
    }
}

unsafe fn cfdict_get_i64(dict: core_foundation::base::CFTypeRef, key: &str) -> Option<i64> {
    let value = cfdict_get_raw(dict, key)?;
    let mut result: i64 = 0;
    if CFNumberGetValue(
        value,
        4, /* kCFNumberSInt64Type */
        &mut result as *mut _ as *mut _,
    ) {
        return Some(result);
    }
    let mut result32: i32 = 0;
    if CFNumberGetValue(
        value,
        3, /* kCFNumberSInt32Type */
        &mut result32 as *mut _ as *mut _,
    ) {
        return Some(result32 as i64);
    }
    None
}

unsafe fn cfdict_get_f64(dict: core_foundation::base::CFTypeRef, key: &str) -> Option<f64> {
    let value = cfdict_get_raw(dict, key)?;
    cfnumber_to_f64(value)
}

unsafe fn cfdict_get_string(dict: core_foundation::base::CFTypeRef, key: &str) -> Option<String> {
    let value = cfdict_get_raw(dict, key)?;
    let cf_str = CFString::wrap_under_get_rule(value as *const _);
    Some(cf_str.to_string())
}

fn parse_bounds(dict: core_foundation::base::CFTypeRef) -> Option<WindowBounds> {
    unsafe {
        let bounds_dict = cfdict_get_raw(dict, "kCGWindowBounds")?;
        let x = cfdict_get_f64(bounds_dict, "X")?;
        let y = cfdict_get_f64(bounds_dict, "Y")?;
        let w = cfdict_get_f64(bounds_dict, "Width")?;
        let h = cfdict_get_f64(bounds_dict, "Height")?;

        Some(WindowBounds {
            x,
            y,
            width: w,
            height: h,
        })
    }
}

/// Check Screen Recording permission without prompting.
///
/// `CGPreflightScreenCaptureAccess` is unreliable for `.app` bundles — it can
/// return `false` even after the user has granted access.  A more dependable
/// test is to actually request the window list and check whether macOS
/// populates `kCGWindowName` for windows owned by other processes: that key is
/// only present when Screen Recording access has been granted.
pub fn check_screen_recording() -> bool {
    // Fast path: the official API agrees we have access.
    if unsafe { CGPreflightScreenCaptureAccess() } {
        return true;
    }

    // Fallback: try to read a window name from another process.
    unsafe {
        let my_pid = std::process::id() as i64;
        let option = kCGWindowListExcludeDesktopElements | kCGWindowListOptionAll;
        let window_list_ref = CGWindowListCopyWindowInfo(option, 0);
        if window_list_ref.is_null() {
            return false;
        }

        let count = CFArrayGetCount(window_list_ref);
        for i in 0..count {
            let dict = CFArrayGetValueAtIndex(window_list_ref, i);
            if dict.is_null() {
                continue;
            }

            // Only check windows from *other* processes.
            if let Some(pid) = cfdict_get_i64(dict, "kCGWindowOwnerPID") {
                if pid == my_pid {
                    continue;
                }
            }

            // Layer 0 = normal windows — more likely to carry a name.
            if cfdict_get_i64(dict, "kCGWindowLayer") != Some(0) {
                continue;
            }

            // Without Screen Recording the key is entirely absent from
            // the dictionary.  Its mere presence (even if the value is an
            // empty string) proves we have the permission.
            if cfdict_get_raw(dict, "kCGWindowName").is_some() {
                core_foundation::base::CFRelease(window_list_ref);
                return true;
            }
        }

        core_foundation::base::CFRelease(window_list_ref);
    }

    false
}

/// Request Screen Recording permission.
/// Calls the system API to register the app, then opens System Settings
/// to the Screen Recording pane so the user can grant access.
pub fn request_screen_recording() {
    unsafe { CGRequestScreenCaptureAccess(); }
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
        .spawn();
}

pub fn enumerate_windows() -> Vec<WindowInfo> {
    let mut windows = Vec::new();
    let mut bundle_cache: HashMap<i64, Option<String>> = HashMap::new();

    unsafe {
        let cid = CGSMainConnectionID();
        let option = kCGWindowListOptionAll | kCGWindowListExcludeDesktopElements;
        let window_list_ref = CGWindowListCopyWindowInfo(option, 0);
        if window_list_ref.is_null() {
            eprintln!("[debug] CGWindowListCopyWindowInfo returned null");
            return windows;
        }

        let count = CFArrayGetCount(window_list_ref);
        eprintln!("[debug] CGWindowList returned {count} entries");

        let mut skipped_layer = 0;
        let mut skipped_system = 0;
        let mut skipped_no_pid = 0;
        let mut skipped_no_id = 0;
        let mut skipped_no_bounds = 0;
        let mut skipped_tiny = 0;
        let mut skipped_no_wid = 0;

        for i in 0..count {
            let dict = CFArrayGetValueAtIndex(window_list_ref, i);
            if dict.is_null() {
                continue;
            }

            // Filter: layer must be 0 (normal windows)
            let layer = match cfdict_get_i64(dict, "kCGWindowLayer") {
                Some(l) => l,
                None => {
                    skipped_layer += 1;
                    continue;
                }
            };
            if layer != 0 {
                skipped_layer += 1;
                continue;
            }

            // Filter: exclude system processes
            let owner_name = cfdict_get_string(dict, "kCGWindowOwnerName").unwrap_or_default();
            if EXCLUDED_OWNERS.iter().any(|&e| e == owner_name) {
                skipped_system += 1;
                continue;
            }

            let pid = match cfdict_get_i64(dict, "kCGWindowOwnerPID") {
                Some(p) => p,
                None => {
                    skipped_no_pid += 1;
                    continue;
                }
            };

            let window_id = match cfdict_get_i64(dict, "kCGWindowNumber") {
                Some(id) => id as u32,
                None => {
                    skipped_no_wid += 1;
                    continue;
                }
            };

            // Use bundle ID if available, fall back to owner name as identifier
            let bundle_id = bundle_cache
                .entry(pid)
                .or_insert_with(|| get_bundle_id_for_pid(pid))
                .clone();

            let app_id = match bundle_id {
                Some(b) => b,
                None => {
                    if owner_name.is_empty() {
                        skipped_no_id += 1;
                        continue;
                    }
                    owner_name.clone()
                }
            };

            let window_title = cfdict_get_string(dict, "kCGWindowName").unwrap_or_default();

            let bounds = match parse_bounds(dict) {
                Some(b) => b,
                None => {
                    skipped_no_bounds += 1;
                    continue;
                }
            };

            if bounds.width <= 64.0 || bounds.height <= 64.0 {
                skipped_tiny += 1;
                continue;
            }

            let space_id = get_space_for_window(cid, window_id);

            windows.push(WindowInfo {
                pid,
                window_id,
                bundle_id: app_id,
                window_title,
                bounds,
                space_id,
            });
        }

        eprintln!(
            "[debug] Filtered: layer={skipped_layer} system={skipped_system} no_pid={skipped_no_pid} \
             no_id={skipped_no_id} no_wid={skipped_no_wid} no_bounds={skipped_no_bounds} tiny={skipped_tiny} \
             => kept {} windows",
            windows.len()
        );

        core_foundation::base::CFRelease(window_list_ref);
    }

    windows
}

/// Set a window's position and size by matching its CGWindowID via the Accessibility API.
/// Sets size first, then position, then position again — this avoids macOS clamping
/// the position based on the old (wrong) size.
pub fn set_window_position(pid: i64, window_id: u32, bounds: &WindowBounds) -> bool {
    unsafe {
        let app_ref = AXUIElementCreateApplication(pid as i32);
        if app_ref.is_null() {
            return false;
        }

        let windows_attr = CFString::new("AXWindows");
        let mut windows_ref: core_foundation::base::CFTypeRef = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(
            app_ref,
            windows_attr.as_concrete_TypeRef() as *const _,
            &mut windows_ref,
        );
        core_foundation::base::CFRelease(app_ref as *const _);

        if err != 0 || windows_ref.is_null() {
            return false;
        }

        let count = CFArrayGetCount(windows_ref);

        for i in 0..count {
            let window_ref = CFArrayGetValueAtIndex(windows_ref, i);
            if window_ref.is_null() {
                continue;
            }

            // Match by CGWindowID using private _AXUIElementGetWindow
            let mut ax_window_id: u32 = 0;
            let err = _AXUIElementGetWindow(window_ref, &mut ax_window_id);
            if err != 0 {
                continue;
            }
            if ax_window_id != window_id {
                continue;
            }

            // Set size first so position isn't clamped by old size
            let mut size = CGSize {
                width: bounds.width,
                height: bounds.height,
            };
            let size_value =
                AXValueCreate(K_AX_VALUE_TYPE_CG_SIZE, &mut size as *mut _ as *const _);
            if !size_value.is_null() {
                let size_attr = CFString::new("AXSize");
                AXUIElementSetAttributeValue(
                    window_ref,
                    size_attr.as_concrete_TypeRef() as *const _,
                    size_value as *const _,
                );
                core_foundation::base::CFRelease(size_value as *const _);
            }

            // Set position
            let mut point = CGPoint {
                x: bounds.x,
                y: bounds.y,
            };
            let position_value =
                AXValueCreate(K_AX_VALUE_TYPE_CG_POINT, &mut point as *mut _ as *const _);
            if !position_value.is_null() {
                let pos_attr = CFString::new("AXPosition");
                AXUIElementSetAttributeValue(
                    window_ref,
                    pos_attr.as_concrete_TypeRef() as *const _,
                    position_value as *const _,
                );
                core_foundation::base::CFRelease(position_value as *const _);
            }

            // Set position again — some apps adjust after the first set
            let mut point2 = CGPoint {
                x: bounds.x,
                y: bounds.y,
            };
            let position_value2 =
                AXValueCreate(K_AX_VALUE_TYPE_CG_POINT, &mut point2 as *mut _ as *const _);
            if !position_value2.is_null() {
                let pos_attr2 = CFString::new("AXPosition");
                AXUIElementSetAttributeValue(
                    window_ref,
                    pos_attr2.as_concrete_TypeRef() as *const _,
                    position_value2 as *const _,
                );
                core_foundation::base::CFRelease(position_value2 as *const _);
            }

            core_foundation::base::CFRelease(windows_ref);
            return true;
        }

        core_foundation::base::CFRelease(windows_ref);
    }

    false
}

/// Check Accessibility permission without prompting.
pub fn check_accessibility() -> bool {
    unsafe {
        let key = CFString::new("AXTrustedCheckOptionPrompt");
        let value = CFBoolean::false_value();
        let options = core_foundation::dictionary::CFDictionary::from_CFType_pairs(&[(
            key,
            value.as_CFType(),
        )]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef() as *const _)
    }
}

/// Request Accessibility permission.
/// Calls the system API to show the prompt, then opens System Settings
/// to the Accessibility pane so the user can grant access.
pub fn request_accessibility() {
    unsafe {
        let key = CFString::new("AXTrustedCheckOptionPrompt");
        let value = CFBoolean::true_value();
        let options = core_foundation::dictionary::CFDictionary::from_CFType_pairs(&[(
            key,
            value.as_CFType(),
        )]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef() as *const _);
    }
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

#[repr(C)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
struct CGSize {
    width: f64,
    height: f64,
}

const K_AX_VALUE_TYPE_CG_POINT: u32 = 1;
const K_AX_VALUE_TYPE_CG_SIZE: u32 = 2;

extern "C" {
    fn CFArrayGetCount(array: core_foundation::base::CFTypeRef) -> i64;
    fn CFArrayGetValueAtIndex(
        array: core_foundation::base::CFTypeRef,
        index: i64,
    ) -> core_foundation::base::CFTypeRef;
    fn CFDictionaryGetValueIfPresent(
        dict: core_foundation::base::CFTypeRef,
        key: *const std::ffi::c_void,
        value: *mut core_foundation::base::CFTypeRef,
    ) -> u8;
    fn CFNumberGetValue(
        number: core_foundation::base::CFTypeRef,
        the_type: i32,
        value_ptr: *mut std::ffi::c_void,
    ) -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> core_foundation::base::CFTypeRef;
    fn AXUIElementCopyAttributeValue(
        element: core_foundation::base::CFTypeRef,
        attribute: *const std::ffi::c_void,
        value: *mut core_foundation::base::CFTypeRef,
    ) -> i32;
    fn AXUIElementSetAttributeValue(
        element: core_foundation::base::CFTypeRef,
        attribute: *const std::ffi::c_void,
        value: *const std::ffi::c_void,
    ) -> i32;
    fn AXValueCreate(
        value_type: u32,
        value: *const std::ffi::c_void,
    ) -> core_foundation::base::CFTypeRef;
    fn AXIsProcessTrustedWithOptions(options: *const std::ffi::c_void) -> bool;
    fn _AXUIElementGetWindow(element: core_foundation::base::CFTypeRef, window_id: *mut u32)
        -> i32;
}
