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
