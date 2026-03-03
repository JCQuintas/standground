use std::fs;
use std::os::unix::fs::PermissionsExt;
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

    // Determine the right asset for the current architecture
    let arch_suffix = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        other => return Err(format!("Unsupported architecture: {other}")),
    };
    let expected_name = format!("standground-darwin-{arch_suffix}.tar.gz");

    let assets = body["assets"]
        .as_array()
        .ok_or("Missing assets in response")?;

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
    let parse = |s: &str| -> Vec<u64> {
        s.split('.')
            .filter_map(|p| p.parse().ok())
            .collect()
    };
    let l = parse(latest);
    let c = parse(current);
    l > c
}

pub fn apply_update(download_url: &str) -> Result<(), String> {
    let temp_dir = std::env::temp_dir().join("standground-update");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).map_err(|e| format!("Failed to create temp dir: {e}"))?;

    let archive_path = temp_dir.join("update.tar.gz");

    // Download the archive
    let mut response = ureq::get(download_url)
        .header("User-Agent", &format!("standground/{}", crate::VERSION))
        .call()
        .map_err(|e| format!("Failed to download update: {e}"))?;

    let mut file =
        fs::File::create(&archive_path).map_err(|e| format!("Failed to create archive: {e}"))?;
    std::io::copy(&mut response.body_mut().as_reader(), &mut file)
        .map_err(|e| format!("Failed to write archive: {e}"))?;

    // Extract the binary
    let status = Command::new("tar")
        .args(["xzf", archive_path.to_str().unwrap()])
        .current_dir(&temp_dir)
        .status()
        .map_err(|e| format!("Failed to extract archive: {e}"))?;

    if !status.success() {
        return Err("tar extraction failed".to_string());
    }

    let new_binary = temp_dir.join("standground");
    if !new_binary.exists() {
        return Err("Extracted binary not found".to_string());
    }

    // Determine where to place the new binary
    let current_exe =
        std::env::current_exe().map_err(|e| format!("Failed to get current exe: {e}"))?;

    let target = if is_app_bundle(&current_exe) {
        // Running from .app bundle — replace the binary inside Contents/MacOS/
        current_exe.clone()
    } else {
        current_exe.clone()
    };

    // Replace: rename old binary to .bak, move new one in
    let backup = target.with_extension("bak");
    let _ = fs::remove_file(&backup); // remove old backup if it exists
    fs::rename(&target, &backup).map_err(|e| format!("Failed to backup current binary: {e}"))?;
    fs::copy(&new_binary, &target).map_err(|e| format!("Failed to install new binary: {e}"))?;

    // Ensure executable permissions
    let perms = fs::Permissions::from_mode(0o755);
    fs::set_permissions(&target, perms)
        .map_err(|e| format!("Failed to set permissions: {e}"))?;

    // Clean up
    let _ = fs::remove_dir_all(&temp_dir);
    let _ = fs::remove_file(&backup);

    Ok(())
}

fn is_app_bundle(exe_path: &std::path::Path) -> bool {
    exe_path
        .parent()
        .map(|p| p.ends_with("Contents/MacOS"))
        .unwrap_or(false)
}

pub fn restart_app() -> ! {
    let exe = std::env::current_exe().expect("Failed to get current exe for restart");
    let mut args: Vec<String> = std::env::args().collect();
    // Remove the program name (first arg)
    if !args.is_empty() {
        args.remove(0);
    }

    Command::new(&exe)
        .args(&args)
        .spawn()
        .expect("Failed to restart app");

    // Terminate current process
    unsafe {
        use objc2::MainThreadMarker;
        use objc2_app_kit::NSApplication;

        let mtm = MainThreadMarker::new_unchecked();
        let app = NSApplication::sharedApplication(mtm);
        app.terminate(None);
    }

    // Fallback if NSApp terminate doesn't exit
    std::process::exit(0);
}
