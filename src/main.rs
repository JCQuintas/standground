mod app;
mod config;
mod display;
mod layout;
mod storage;
mod update;
mod window;

pub static mut DEBUG: bool = false;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("StandGround {VERSION}");
        return;
    }

    let foreground = args.iter().any(|a| a == "--foreground" || a == "-f");
    let debug = args.iter().any(|a| a == "--debug" || a == "-d");
    let is_app_bundle = is_running_from_app_bundle();

    unsafe {
        DEBUG = debug;
    }

    if foreground || is_app_bundle {
        app::run();
    } else {
        daemonize(debug);
    }
}

/// Detect if we're running inside a .app bundle (Contents/MacOS/standground).
fn is_running_from_app_bundle() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .map(|parent| parent.ends_with("Contents/MacOS"))
        .unwrap_or(false)
}

fn daemonize(debug: bool) {
    use std::process::Command;

    let exe = std::env::current_exe().expect("Failed to get current executable path");

    let mut cmd = Command::new(&exe);
    cmd.arg("--foreground");
    if debug {
        cmd.arg("--debug");
    }

    let child = if debug {
        cmd.spawn()
    } else {
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    };

    let mut child = child.expect("Failed to spawn background process");
    println!("StandGround started (pid {})", child.id());
    // Detach: the parent exits immediately after spawning the daemon.
    // We won't wait for the child — it runs independently.
    std::mem::drop(child.stdout.take());
    std::mem::drop(child.stderr.take());
    std::mem::drop(child.stdin.take());
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}
