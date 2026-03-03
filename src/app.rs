use std::sync::mpsc;
use std::time::Instant;

use cocoa::appkit::{
    NSApp, NSApplication, NSApplicationActivationPolicyAccessory, NSMenu, NSMenuItem, NSStatusBar,
    NSStatusItem, NSSquareStatusItemLength,
};
use cocoa::base::{id, nil, YES};
use cocoa::foundation::{NSAutoreleasePool, NSString};
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel};
use objc::{class, msg_send, sel, sel_impl};

use crate::config::AppConfig;
use crate::display::{DisplayEvent, register_display_callback};
use crate::layout::{self, LayoutStore};
use crate::storage;
use crate::window;

// Global mutable state accessed from ObjC callbacks
static mut APP_STATE: Option<*mut AppState> = None;

struct AppState {
    config: AppConfig,
    layout_store: LayoutStore,
    display_rx: mpsc::Receiver<DisplayEvent>,
    last_display_change: Option<Instant>,
    status_item: id,
}

pub fn run() {
    unsafe {
        let _pool = NSAutoreleasePool::new(nil);

        let app = NSApp();
        app.setActivationPolicy_(NSApplicationActivationPolicyAccessory);

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
        let status_bar = NSStatusBar::systemStatusBar(nil);
        let status_item = status_bar.statusItemWithLength_(NSSquareStatusItemLength);

        let icon_bytes = include_bytes!("../assets/icon.svg");
        let ns_data: id = msg_send![class!(NSData), dataWithBytes:icon_bytes.as_ptr()
                                                     length:icon_bytes.len()];
        let icon_image: id = msg_send![class!(NSImage), alloc];
        let icon_image: id = msg_send![icon_image, initWithData: ns_data];
        // Set as template so macOS handles light/dark mode automatically
        let _: () = msg_send![icon_image, setTemplate: YES];
        // Size to fit the menu bar
        let icon_size = cocoa::foundation::NSSize::new(18.0, 18.0);
        let _: () = msg_send![icon_image, setSize: icon_size];

        let button: id = msg_send![status_item, button];
        let _: () = msg_send![button, setImage: icon_image];
        let _: () = msg_send![status_item, setHighlightMode: YES];

        // Register custom ObjC class for menu actions
        register_menu_handler_class();

        // Build menu
        let menu = build_menu(config.auto_restore, config.launch_at_login);
        status_item.setMenu_(menu);

        // Store app state
        let state = Box::new(AppState {
            config,
            layout_store,
            display_rx,
            last_display_change: None,
            status_item,
        });
        APP_STATE = Some(Box::into_raw(state));

        // Set up a repeating timer to poll for display changes
        let timer_interval = 1.0; // seconds
        let handler_class = class!(StandGroundHandler);
        let handler: id = msg_send![handler_class, new];

        let timer_cls = class!(NSTimer);
        let _timer: id = msg_send![timer_cls,
            scheduledTimerWithTimeInterval: timer_interval
            target: handler
            selector: sel!(timerFired:)
            userInfo: nil
            repeats: YES
        ];

        app.run();
    }
}

unsafe fn build_menu(auto_restore: bool, launch_at_login: bool) -> id {
    let menu = NSMenu::new(nil).autorelease();
    let handler_class = class!(StandGroundHandler);
    let handler: id = msg_send![handler_class, new];

    // Save Current Layout
    let save_title = NSString::alloc(nil).init_str("Save Current Layout");
    let save_item = NSMenuItem::alloc(nil).initWithTitle_action_keyEquivalent_(
        save_title,
        sel!(saveLayout:),
        NSString::alloc(nil).init_str(""),
    );
    let _: () = msg_send![save_item, setTarget: handler];
    menu.addItem_(save_item);

    // Restore Layout
    let restore_title = NSString::alloc(nil).init_str("Restore Layout");
    let restore_item = NSMenuItem::alloc(nil).initWithTitle_action_keyEquivalent_(
        restore_title,
        sel!(restoreLayout:),
        NSString::alloc(nil).init_str(""),
    );
    let _: () = msg_send![restore_item, setTarget: handler];
    menu.addItem_(restore_item);

    // Separator
    let sep: id = msg_send![class!(NSMenuItem), separatorItem];
    menu.addItem_(sep);

    // Auto-restore toggle
    let auto_text = if auto_restore {
        "Auto-restore: On"
    } else {
        "Auto-restore: Off"
    };
    let auto_title = NSString::alloc(nil).init_str(auto_text);
    let auto_item = NSMenuItem::alloc(nil).initWithTitle_action_keyEquivalent_(
        auto_title,
        sel!(toggleAutoRestore:),
        NSString::alloc(nil).init_str(""),
    );
    let _: () = msg_send![auto_item, setTarget: handler];
    menu.addItem_(auto_item);

    // Launch at Login toggle
    let login_text = if launch_at_login {
        "Launch at Login: On"
    } else {
        "Launch at Login: Off"
    };
    let login_title = NSString::alloc(nil).init_str(login_text);
    let login_item = NSMenuItem::alloc(nil).initWithTitle_action_keyEquivalent_(
        login_title,
        sel!(toggleLaunchAtLogin:),
        NSString::alloc(nil).init_str(""),
    );
    let _: () = msg_send![login_item, setTarget: handler];
    menu.addItem_(login_item);

    // Separator
    let sep2: id = msg_send![class!(NSMenuItem), separatorItem];
    menu.addItem_(sep2);

    // Quit
    let quit_title = NSString::alloc(nil).init_str("Quit");
    let quit_item = NSMenuItem::alloc(nil).initWithTitle_action_keyEquivalent_(
        quit_title,
        sel!(quitApp:),
        NSString::alloc(nil).init_str(""),
    );
    let _: () = msg_send![quit_item, setTarget: handler];
    menu.addItem_(quit_item);

    menu
}

fn register_menu_handler_class() {
    let superclass = class!(NSObject);
    if Class::get("StandGroundHandler").is_some() {
        return;
    }
    let mut decl = ClassDecl::new("StandGroundHandler", superclass).unwrap();

    unsafe {
        decl.add_method(
            sel!(saveLayout:),
            save_layout_action as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(restoreLayout:),
            restore_layout_action as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(toggleAutoRestore:),
            toggle_auto_restore_action as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(toggleLaunchAtLogin:),
            toggle_launch_at_login_action as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(quitApp:),
            quit_app_action as extern "C" fn(&Object, Sel, id),
        );
        decl.add_method(
            sel!(timerFired:),
            timer_fired_action as extern "C" fn(&Object, Sel, id),
        );
    }

    decl.register();
}

extern "C" fn save_layout_action(_this: &Object, _cmd: Sel, _sender: id) {
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
                        if let Ok(json) = serde_json::to_string_pretty(&state.layout_store) {
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

extern "C" fn restore_layout_action(_this: &Object, _cmd: Sel, _sender: id) {
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

extern "C" fn toggle_auto_restore_action(_this: &Object, _cmd: Sel, _sender: id) {
    unsafe {
        if let Some(state_ptr) = APP_STATE {
            let state = &mut *state_ptr;
            state.config.auto_restore = !state.config.auto_restore;
            if let Err(e) = storage::save_config(&state.config) {
                eprintln!("Error saving config: {e}");
            }
            // Rebuild menu to reflect new state
            let menu = build_menu(state.config.auto_restore, state.config.launch_at_login);
            state.status_item.setMenu_(menu);
            println!(
                "Auto-restore: {}",
                if state.config.auto_restore { "On" } else { "Off" }
            );
        }
    }
}

extern "C" fn toggle_launch_at_login_action(_this: &Object, _cmd: Sel, _sender: id) {
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
            let menu = build_menu(state.config.auto_restore, state.config.launch_at_login);
            state.status_item.setMenu_(menu);
            eprintln!(
                "Launch at Login: {}",
                if state.config.launch_at_login { "On" } else { "Off" }
            );
        }
    }
}

extern "C" fn quit_app_action(_this: &Object, _cmd: Sel, _sender: id) {
    unsafe {
        let app = NSApp();
        let _: () = msg_send![app, terminate: nil];
    }
}

extern "C" fn timer_fired_action(_this: &Object, _cmd: Sel, _sender: id) {
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
                    println!("Display configuration changed, auto-restoring layout...");
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
        }
    }
}
