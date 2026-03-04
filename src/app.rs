use std::sync::mpsc;
use std::time::Instant;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSImage, NSMenu, NSMenuItem,
    NSSquareStatusItemLength, NSStatusBar, NSStatusItem,
};
use objc2_foundation::{NSData, NSObject, NSSize, NSString};

use crate::config::AppConfig;
use crate::display::{register_display_callback, DisplayEvent};
use crate::layout::{self, LayoutStore};
use crate::storage;
use crate::update::UpdateInfo;
use crate::window;

// Global mutable state accessed from ObjC callbacks
static mut APP_STATE: Option<*mut AppState> = None;

struct AppState {
    config: AppConfig,
    layout_store: LayoutStore,
    display_rx: mpsc::Receiver<DisplayEvent>,
    last_display_change: Option<Instant>,
    status_item: Retained<NSStatusItem>,
    update_info: Option<UpdateInfo>,
    update_rx: mpsc::Receiver<Option<UpdateInfo>>,
    update_tx: mpsc::Sender<Option<UpdateInfo>>,
    mtm: MainThreadMarker,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "StandGroundHandler"]
    struct StandGroundHandler;

    impl StandGroundHandler {
        #[unsafe(method(saveLayout:))]
        fn save_layout(&self, _sender: &AnyObject) {
            unsafe {
                if let Some(state_ptr) = APP_STATE {
                    let state = &mut *state_ptr;
                    match layout::save_current_layout(&mut state.layout_store) {
                        Ok(count) => {
                            if let Err(e) = storage::save_layouts(&state.layout_store) {
                                eprintln!("Error saving layouts: {e}");
                            }
                            eprintln!("Saved layout with {count} windows");
                            if crate::DEBUG {
                                if let Ok(json) =
                                    serde_json::to_string_pretty(&state.layout_store)
                                {
                                    eprintln!("[debug] Current layout store:\n{json}");
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Error saving layout: {e}");
                        }
                    }
                }
            }
        }

        #[unsafe(method(restoreLayout:))]
        fn restore_layout(&self, _sender: &AnyObject) {
            unsafe {
                if let Some(state_ptr) = APP_STATE {
                    let state = &*state_ptr;
                    match layout::restore_layout(&state.layout_store) {
                        Ok((restored, total)) => {
                            println!("Restored {restored}/{total} windows");
                        }
                        Err(e) => {
                            eprintln!("Error restoring layout: {e}");
                        }
                    }
                }
            }
        }

        #[unsafe(method(toggleAutoRestore:))]
        fn toggle_auto_restore(&self, _sender: &AnyObject) {
            unsafe {
                if let Some(state_ptr) = APP_STATE {
                    let state = &mut *state_ptr;
                    state.config.auto_restore = !state.config.auto_restore;
                    if let Err(e) = storage::save_config(&state.config) {
                        eprintln!("Error saving config: {e}");
                    }
                    rebuild_menu(state);
                    println!(
                        "Auto-restore: {}",
                        if state.config.auto_restore {
                            "On"
                        } else {
                            "Off"
                        }
                    );
                }
            }
        }

        #[unsafe(method(toggleLaunchAtLogin:))]
        fn toggle_launch_at_login(&self, _sender: &AnyObject) {
            unsafe {
                if let Some(state_ptr) = APP_STATE {
                    let state = &mut *state_ptr;
                    state.config.launch_at_login = !state.config.launch_at_login;
                    if let Err(e) = storage::save_config(&state.config) {
                        eprintln!("Error saving config: {e}");
                    }
                    if let Err(e) = storage::set_launch_at_login(state.config.launch_at_login) {
                        eprintln!("Error updating launch agent: {e}");
                    }
                    rebuild_menu(state);
                    eprintln!(
                        "Launch at Login: {}",
                        if state.config.launch_at_login {
                            "On"
                        } else {
                            "Off"
                        }
                    );
                }
            }
        }

        #[unsafe(method(toggleAutoUpdate:))]
        fn toggle_auto_update(&self, _sender: &AnyObject) {
            unsafe {
                if let Some(state_ptr) = APP_STATE {
                    let state = &mut *state_ptr;
                    state.config.auto_update = !state.config.auto_update;
                    if let Err(e) = storage::save_config(&state.config) {
                        eprintln!("Error saving config: {e}");
                    }
                    rebuild_menu(state);
                    eprintln!(
                        "Auto-update: {}",
                        if state.config.auto_update {
                            "On"
                        } else {
                            "Off"
                        }
                    );
                }
            }
        }

        #[unsafe(method(checkForUpdates:))]
        fn check_for_updates(&self, _sender: &AnyObject) {
            unsafe {
                if let Some(state_ptr) = APP_STATE {
                    let state = &mut *state_ptr;
                    let tx = state.update_tx.clone();
                    let current_version = crate::VERSION.to_string();
                    std::thread::spawn(move || {
                        match crate::update::check_for_update(&current_version) {
                            Ok(Some(info)) => {
                                eprintln!("Update available: v{}", info.version);
                                let _ = tx.send(Some(info));
                            }
                            Ok(None) => {
                                eprintln!("Already up to date");
                                let _ = tx.send(None);
                            }
                            Err(e) => {
                                eprintln!("Update check failed: {e}");
                                let _ = tx.send(None);
                            }
                        }
                    });
                }
            }
        }

        #[unsafe(method(installUpdate:))]
        fn install_update(&self, _sender: &AnyObject) {
            unsafe {
                if let Some(state_ptr) = APP_STATE {
                    let state = &*state_ptr;
                    if let Some(info) = &state.update_info {
                        let download_url = info.download_url.clone();
                        eprintln!("Installing update v{}...", info.version);
                        std::thread::spawn(move || {
                            match crate::update::apply_update(&download_url) {
                                Ok(()) => {
                                    eprintln!("Update installed, restarting...");
                                    crate::update::restart_app();
                                }
                                Err(e) => {
                                    eprintln!("Failed to install update: {e}");
                                }
                            }
                        });
                    }
                }
            }
        }

        #[unsafe(method(quitApp:))]
        fn quit_app(&self, _sender: &AnyObject) {
            unsafe {
                let mtm = MainThreadMarker::new_unchecked();
                let app = NSApplication::sharedApplication(mtm);
                app.terminate(None);
            }
        }

        #[unsafe(method(showLicense:))]
        fn show_license(&self, _sender: &AnyObject) {
            let _ = std::process::Command::new("open")
                .arg("https://github.com/JCQuintas/standground/blob/main/LICENSE")
                .spawn();
        }

        #[unsafe(method(timerFired:))]
        fn timer_fired(&self, _sender: &AnyObject) {
            unsafe {
                if let Some(state_ptr) = APP_STATE {
                    let state = &mut *state_ptr;

                    // Check for display change events
                    while let Ok(_event) = state.display_rx.try_recv() {
                        state.last_display_change = Some(Instant::now());
                    }

                    // Debounce: wait 2 seconds after last display change before restoring
                    if let Some(last_change) = state.last_display_change {
                        if last_change.elapsed().as_secs() >= 2 && state.config.auto_restore {
                            state.last_display_change = None;
                            println!(
                                "Display configuration changed, auto-restoring layout..."
                            );
                            match layout::restore_layout(&state.layout_store) {
                                Ok((restored, total)) => {
                                    println!("Auto-restored {restored}/{total} windows");
                                }
                                Err(e) => {
                                    eprintln!("No layout to restore: {e}");
                                }
                            }
                        }
                    }

                    // Poll for update check results
                    if let Ok(Some(update_info)) = state.update_rx.try_recv() {
                        if state.config.auto_update {
                            // Auto-update: apply immediately
                            let download_url = update_info.download_url.clone();
                            eprintln!("Auto-updating to v{}...", update_info.version);
                            std::thread::spawn(move || {
                                match crate::update::apply_update(&download_url) {
                                    Ok(()) => {
                                        eprintln!("Update installed, restarting...");
                                        crate::update::restart_app();
                                    }
                                    Err(e) => {
                                        eprintln!("Auto-update failed: {e}");
                                    }
                                }
                            });
                        } else {
                            // Manual mode: show update in menu
                            state.update_info = Some(update_info);
                            rebuild_menu(state);
                        }
                    }
                }
            }
        }
    }
);

pub fn run() {
    let mtm = MainThreadMarker::new().expect("must be called from the main thread");

    unsafe {
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

        // Check permissions
        if !window::check_accessibility() {
            eprintln!("Accessibility access not yet granted. Please grant access in System Settings > Privacy & Security > Accessibility.");
        }
        if !window::check_screen_recording() {
            eprintln!("Screen Recording access not yet granted. Please grant access in System Settings > Privacy & Security > Screen Recording.");
        }

        // Load persisted state
        let mut config = storage::load_config();
        let layout_store = storage::load_layouts();

        // Sync launch agent with config (in case plist was removed externally)
        let agent_installed = storage::is_launch_agent_installed();
        if config.launch_at_login != agent_installed {
            config.launch_at_login = agent_installed;
            let _ = storage::save_config(&config);
        }

        // Set up display change detection
        let (display_tx, display_rx) = mpsc::channel();
        if let Err(e) = register_display_callback(display_tx) {
            eprintln!("Warning: could not register display callback: {e}");
        }

        // Create status bar item with brick icon
        let status_bar = NSStatusBar::systemStatusBar();
        let status_item = status_bar.statusItemWithLength(NSSquareStatusItemLength);

        let icon_bytes = include_bytes!("../assets/icon.svg");
        let ns_data = NSData::with_bytes(icon_bytes);
        let icon_image =
            NSImage::initWithData(NSImage::alloc(), &ns_data).expect("Failed to create icon image");
        icon_image.setTemplate(true);
        icon_image.setSize(NSSize::new(18.0, 18.0));

        if let Some(button) = status_item.button(mtm) {
            button.setImage(Some(&icon_image));
        }

        // Build menu
        let menu = build_menu(
            config.auto_restore,
            config.launch_at_login,
            config.auto_update,
            &None,
            mtm,
        );
        status_item.setMenu(Some(&menu));

        // Set up update checking
        let (update_tx, update_rx) = mpsc::channel();
        let startup_tx = update_tx.clone();
        let current_version = crate::VERSION.to_string();
        std::thread::spawn(
            move || match crate::update::check_for_update(&current_version) {
                Ok(info) => {
                    let _ = startup_tx.send(info);
                }
                Err(e) => {
                    eprintln!("Update check failed: {e}");
                    let _ = startup_tx.send(None);
                }
            },
        );

        // Store app state
        let state = Box::new(AppState {
            config,
            layout_store,
            display_rx,
            last_display_change: None,
            status_item,
            update_info: None,
            update_rx,
            update_tx,
            mtm,
        });
        APP_STATE = Some(Box::into_raw(state));

        // Set up a repeating timer to poll for display changes
        let handler: Retained<StandGroundHandler> = msg_send![StandGroundHandler::alloc(), init];

        let _timer = objc2_foundation::NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
            1.0,
            &handler,
            sel!(timerFired:),
            None,
            true,
        );

        app.run();
    }
}

unsafe fn build_menu(
    auto_restore: bool,
    launch_at_login: bool,
    auto_update: bool,
    update_info: &Option<UpdateInfo>,
    mtm: MainThreadMarker,
) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    let handler: Retained<StandGroundHandler> = msg_send![StandGroundHandler::alloc(), init];
    let empty_str = NSString::from_str("");

    // Version header (disabled, just for display)
    let version_text = format!("StandGround v{}", crate::VERSION);
    let version_title = NSString::from_str(&version_text);
    let version_item = NSMenuItem::initWithTitle_action_keyEquivalent(
        NSMenuItem::alloc(mtm),
        &version_title,
        None,
        &empty_str,
    );
    version_item.setEnabled(false);
    menu.addItem(&version_item);

    // Separator
    menu.addItem(&NSMenuItem::separatorItem(mtm));

    // Save Current Layout
    let save_title = NSString::from_str("Save Current Layout");
    let save_item = NSMenuItem::initWithTitle_action_keyEquivalent(
        NSMenuItem::alloc(mtm),
        &save_title,
        Some(sel!(saveLayout:)),
        &empty_str,
    );
    save_item.setTarget(Some(&*handler));
    menu.addItem(&save_item);

    // Restore Layout
    let restore_title = NSString::from_str("Restore Layout");
    let restore_item = NSMenuItem::initWithTitle_action_keyEquivalent(
        NSMenuItem::alloc(mtm),
        &restore_title,
        Some(sel!(restoreLayout:)),
        &empty_str,
    );
    restore_item.setTarget(Some(&*handler));
    menu.addItem(&restore_item);

    // Separator
    menu.addItem(&NSMenuItem::separatorItem(mtm));

    // Auto-restore toggle
    let auto_text = if auto_restore {
        "Auto-restore: On"
    } else {
        "Auto-restore: Off"
    };
    let auto_title = NSString::from_str(auto_text);
    let auto_item = NSMenuItem::initWithTitle_action_keyEquivalent(
        NSMenuItem::alloc(mtm),
        &auto_title,
        Some(sel!(toggleAutoRestore:)),
        &empty_str,
    );
    auto_item.setTarget(Some(&*handler));
    menu.addItem(&auto_item);

    // Launch at Login toggle
    let login_text = if launch_at_login {
        "Launch at Login: On"
    } else {
        "Launch at Login: Off"
    };
    let login_title = NSString::from_str(login_text);
    let login_item = NSMenuItem::initWithTitle_action_keyEquivalent(
        NSMenuItem::alloc(mtm),
        &login_title,
        Some(sel!(toggleLaunchAtLogin:)),
        &empty_str,
    );
    login_item.setTarget(Some(&*handler));
    menu.addItem(&login_item);

    // Auto-update toggle
    let update_toggle_text = if auto_update {
        "Auto-update: On"
    } else {
        "Auto-update: Off"
    };
    let update_toggle_title = NSString::from_str(update_toggle_text);
    let update_toggle_item = NSMenuItem::initWithTitle_action_keyEquivalent(
        NSMenuItem::alloc(mtm),
        &update_toggle_title,
        Some(sel!(toggleAutoUpdate:)),
        &empty_str,
    );
    update_toggle_item.setTarget(Some(&*handler));
    menu.addItem(&update_toggle_item);

    // Separator
    menu.addItem(&NSMenuItem::separatorItem(mtm));

    // Update / Check for Updates item
    if let Some(info) = update_info {
        if !auto_update {
            let update_text = format!("Update to v{}", info.version);
            let update_title = NSString::from_str(&update_text);
            let update_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &update_title,
                Some(sel!(installUpdate:)),
                &empty_str,
            );
            update_item.setTarget(Some(&*handler));
            menu.addItem(&update_item);
        } else {
            let check_title = NSString::from_str("Check for Updates");
            let check_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &check_title,
                Some(sel!(checkForUpdates:)),
                &empty_str,
            );
            check_item.setTarget(Some(&*handler));
            menu.addItem(&check_item);
        }
    } else {
        let check_title = NSString::from_str("Check for Updates");
        let check_item = NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &check_title,
            Some(sel!(checkForUpdates:)),
            &empty_str,
        );
        check_item.setTarget(Some(&*handler));
        menu.addItem(&check_item);
    }

    // License
    let license_title = NSString::from_str("License");
    let license_item = NSMenuItem::initWithTitle_action_keyEquivalent(
        NSMenuItem::alloc(mtm),
        &license_title,
        Some(sel!(showLicense:)),
        &empty_str,
    );
    license_item.setTarget(Some(&*handler));
    menu.addItem(&license_item);

    // Separator
    menu.addItem(&NSMenuItem::separatorItem(mtm));

    // Quit
    let quit_title = NSString::from_str("Quit");
    let quit_item = NSMenuItem::initWithTitle_action_keyEquivalent(
        NSMenuItem::alloc(mtm),
        &quit_title,
        Some(sel!(quitApp:)),
        &empty_str,
    );
    quit_item.setTarget(Some(&*handler));
    menu.addItem(&quit_item);

    // Leak the handler to keep it alive — NSMenuItem.target is an unretained reference,
    // so the handler must outlive the menu items.
    std::mem::forget(handler);

    menu
}

unsafe fn rebuild_menu(state: &AppState) {
    let menu = build_menu(
        state.config.auto_restore,
        state.config.launch_at_login,
        state.config.auto_update,
        &state.update_info,
        state.mtm,
    );
    state.status_item.setMenu(Some(&menu));
}
