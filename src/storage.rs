use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::config::AppConfig;
use crate::layout::LayoutStore;

pub(crate) fn data_dir() -> io::Result<PathBuf> {
    let proj = ProjectDirs::from("com", "standground", "standground").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Could not determine data directory",
        )
    })?;
    let dir = proj.data_dir().to_path_buf();
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn atomic_write(path: &PathBuf, contents: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, contents)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn load_layouts() -> LayoutStore {
    let path = match data_dir() {
        Ok(d) => d.join("layouts.json"),
        Err(e) => {
            eprintln!("Warning: could not determine data dir: {e}");
            return LayoutStore::default();
        }
    };
    match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => LayoutStore::default(),
    }
}

pub fn save_layouts(store: &LayoutStore) -> io::Result<()> {
    let path = data_dir()?.join("layouts.json");
    let json = serde_json::to_string_pretty(store).map_err(io::Error::other)?;
    atomic_write(&path, json.as_bytes())
}

pub fn load_config() -> AppConfig {
    let path = match data_dir() {
        Ok(d) => d.join("config.json"),
        Err(_) => return AppConfig::default(),
    };
    match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => AppConfig::default(),
    }
}

pub fn save_config(config: &AppConfig) -> io::Result<()> {
    let path = data_dir()?.join("config.json");
    let json = serde_json::to_string_pretty(config).map_err(io::Error::other)?;
    atomic_write(&path, json.as_bytes())
}

const LAUNCH_AGENT_LABEL: &str = "com.standground.standground";

fn launch_agent_path() -> io::Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|e| io::Error::new(io::ErrorKind::NotFound, e))?;
    let dir = PathBuf::from(home).join("Library/LaunchAgents");
    fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("{LAUNCH_AGENT_LABEL}.plist")))
}

pub fn set_launch_at_login(enabled: bool) -> io::Result<()> {
    let plist_path = launch_agent_path()?;

    if enabled {
        let exe = std::env::current_exe()?;
        let program_args = if let Some(app_path) = get_app_bundle_path(&exe) {
            // Running from .app bundle — use `open` so macOS handles it properly
            let app_str = app_path.to_string_lossy();
            format!(
                "        <string>/usr/bin/open</string>\n        <string>-a</string>\n        <string>{app_str}</string>"
            )
        } else {
            // Running as standalone binary
            let exe_str = exe.to_string_lossy();
            format!("        <string>{exe_str}</string>\n        <string>--foreground</string>")
        };

        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LAUNCH_AGENT_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
{program_args}
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
</dict>
</plist>
"#
        );
        atomic_write(&plist_path, plist.as_bytes())?;
    } else if plist_path.exists() {
        fs::remove_file(&plist_path)?;
    }

    Ok(())
}

/// If the binary is inside a .app bundle, return the .app directory path.
fn get_app_bundle_path(exe: &Path) -> Option<PathBuf> {
    // Binary is at Something.app/Contents/MacOS/standground
    let macos_dir = exe.parent()?;
    let contents_dir = macos_dir.parent()?;
    let app_dir = contents_dir.parent()?;
    if macos_dir.ends_with("MacOS")
        && contents_dir.ends_with("Contents")
        && app_dir.extension().is_some_and(|ext| ext == "app")
    {
        Some(app_dir.to_path_buf())
    } else {
        None
    }
}

pub fn is_launch_agent_installed() -> bool {
    launch_agent_path().map(|p| p.exists()).unwrap_or(false)
}
