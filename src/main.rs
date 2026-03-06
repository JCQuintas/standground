//! Thin shim that loads the real app logic from a dynamic library.
//!
//! The .app bundle contains this shim as its signed executable. Since the shim
//! never changes, macOS TCC permissions (Accessibility) survive across updates.
//! Updates only replace the dylib in the data directory.

use std::ffi::{CStr, CString};
use std::os::raw::c_void;
use std::path::PathBuf;

const RTLD_NOW: i32 = 0x2;
const DYLIB_NAME: &str = "libstandground_core.dylib";
const DATA_DIR_NAME: &str = "com.standground.standground";

extern "C" {
    fn dlopen(filename: *const i8, flags: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const i8) -> *mut c_void;
    fn dlerror() -> *const i8;
}

fn main() {
    let dylib_path = find_dylib().unwrap_or_else(|| {
        eprintln!("Could not find StandGround core library ({DYLIB_NAME})");
        std::process::exit(1);
    });

    unsafe {
        let path_cstr =
            CString::new(dylib_path.to_string_lossy().as_bytes()).expect("invalid dylib path");
        let handle = dlopen(path_cstr.as_ptr(), RTLD_NOW);
        if handle.is_null() {
            let err = CStr::from_ptr(dlerror());
            eprintln!(
                "Failed to load {}: {}",
                dylib_path.display(),
                err.to_string_lossy()
            );
            std::process::exit(1);
        }

        let sym = CString::new("standground_main").unwrap();
        let func_ptr = dlsym(handle, sym.as_ptr());
        if func_ptr.is_null() {
            eprintln!("Failed to find standground_main in {}", dylib_path.display());
            std::process::exit(1);
        }

        let entry: extern "C" fn() = std::mem::transmute(func_ptr);
        entry();
    }
}

fn find_dylib() -> Option<PathBuf> {
    // 1. Updated version in the app data directory
    if let Ok(home) = std::env::var("HOME") {
        let path = PathBuf::from(&home)
            .join("Library/Application Support")
            .join(DATA_DIR_NAME)
            .join(DYLIB_NAME);
        if path.exists() {
            return Some(path);
        }
    }

    // 2. Bundled in .app/Contents/Resources (first run after install)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(macos_dir) = exe.parent() {
            if macos_dir.ends_with("Contents/MacOS") {
                let resources = macos_dir
                    .parent()
                    .unwrap()
                    .join("Resources")
                    .join(DYLIB_NAME);
                if resources.exists() {
                    return Some(resources);
                }
            }
        }
    }

    // 3. Same directory as the executable (standalone usage)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let path = dir.join(DYLIB_NAME);
            if path.exists() {
                return Some(path);
            }
        }
    }

    None
}
