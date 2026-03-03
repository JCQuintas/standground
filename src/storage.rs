use std::fs;
use std::io;
use std::path::PathBuf;

use directories::ProjectDirs;

use crate::config::AppConfig;
use crate::layout::LayoutStore;

fn data_dir() -> io::Result<PathBuf> {
    let proj = ProjectDirs::from("com", "standground", "standground")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Could not determine data directory"))?;
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
    let json = serde_json::to_string_pretty(store)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
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
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    atomic_write(&path, json.as_bytes())
}

const LAUNCH_AGENT_LABEL: &str = "com.standground.standground";

fn launch_agent_path() -> io::Result<PathBuf> {
    let home = std::env::var("HOME")
        .map_err(|e| io::Error::new(io::ErrorKind::NotFound, e))?;
    let dir = PathBuf::from(home).join("Library/LaunchAgents");
    fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("{LAUNCH_AGENT_LABEL}.plist")))
}

pub fn set_launch_at_login(enabled: bool) -> io::Result<()> {
    let plist_path = launch_agent_path()?;

    if enabled {
        let exe = std::env::current_exe()?;
        let exe_str = exe.to_string_lossy();
        let plist = format!(
r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LAUNCH_AGENT_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe_str}</string>
        <string>--foreground</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
</dict>
</plist>
"#);
        atomic_write(&plist_path, plist.as_bytes())?;
    } else if plist_path.exists() {
        fs::remove_file(&plist_path)?;
    }

    Ok(())
}

pub fn is_launch_agent_installed() -> bool {
    launch_agent_path().map(|p| p.exists()).unwrap_or(false)
}
