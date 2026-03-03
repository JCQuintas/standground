mod app;
mod config;
mod display;
mod layout;
mod storage;
mod window;

pub static mut DEBUG: bool = false;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let foreground = args.iter().any(|a| a == "--foreground" || a == "-f");
    let debug = args.iter().any(|a| a == "--debug" || a == "-d");

    unsafe { DEBUG = debug; }

    if foreground {
        app::run();
    } else {
        daemonize(debug);
    }
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

    let child = child.expect("Failed to spawn background process");
    println!("StandGround started (pid {})", child.id());
}
