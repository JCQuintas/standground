use std::fs;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub version: String,
    pub download_url: String,
}

pub fn check_for_update(current_version: &str) -> Result<Option<UpdateInfo>, String> {
    let url = "https://api.github.com/repos/jcquintas/standground/releases/latest";
    let user_agent = format!("standground/{current_version}");

    let mut response = ureq::get(url)
        .header("User-Agent", &user_agent)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Failed to check for updates: {e}"))?;

    let body: serde_json::Value = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("Failed to parse response: {e}"))?;

    let tag_name = body["tag_name"]
        .as_str()
        .ok_or("Missing tag_name in response")?;

    let latest_version = tag_name.strip_prefix('v').unwrap_or(tag_name);

    if !is_newer(latest_version, current_version) {
        return Ok(None);
    }

    let arch_suffix = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        other => return Err(format!("Unsupported architecture: {other}")),
    };

    let assets = body["assets"]
        .as_array()
        .ok_or("Missing assets in response")?;

    let expected_name = format!("libstandground_core-darwin-{arch_suffix}.dylib");

    let download_url = assets
        .iter()
        .find(|a| a["name"].as_str() == Some(&expected_name))
        .and_then(|a| a["browser_download_url"].as_str())
        .ok_or_else(|| format!("No asset found matching {expected_name}"))?
        .to_string();

    Ok(Some(UpdateInfo {
        version: latest_version.to_string(),
        download_url,
    }))
}

/// Compare semver strings, returns true if `latest` is newer than `current`.
fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> { s.split('.').filter_map(|p| p.parse().ok()).collect() };
    let l = parse(latest);
    let c = parse(current);
    l > c
}

/// Download the new dylib to the app data directory.
pub fn apply_update(download_url: &str) -> Result<(), String> {
    let data_dir = crate::storage::data_dir()
        .map_err(|e| format!("Failed to determine data directory: {e}"))?;

    let dylib_name = "libstandground_core.dylib";
    let final_path = data_dir.join(dylib_name);
    let temp_path = data_dir.join(format!("{dylib_name}.tmp"));

    let mut response = ureq::get(download_url)
        .header("User-Agent", &format!("standground/{}", crate::VERSION))
        .call()
        .map_err(|e| format!("Failed to download update: {e}"))?;

    let mut file =
        fs::File::create(&temp_path).map_err(|e| format!("Failed to create temp file: {e}"))?;
    std::io::copy(&mut response.body_mut().as_reader(), &mut file)
        .map_err(|e| format!("Failed to write dylib: {e}"))?;
    drop(file);

    // Atomic rename
    fs::rename(&temp_path, &final_path)
        .map_err(|e| format!("Failed to install dylib: {e}"))?;

    Ok(())
}

pub fn restart_app() -> ! {
    let exe = std::env::current_exe().expect("Failed to get current exe for restart");
    let mut args: Vec<String> = std::env::args().collect();
    if !args.is_empty() {
        args.remove(0);
    }

    Command::new(&exe)
        .args(&args)
        .spawn()
        .expect("Failed to restart app");

    unsafe {
        use objc2::MainThreadMarker;
        use objc2_app_kit::NSApplication;

        let mtm = MainThreadMarker::new_unchecked();
        let app = NSApplication::sharedApplication(mtm);
        app.terminate(None);
    }

    std::process::exit(0);
}
