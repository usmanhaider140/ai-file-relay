//! AI File Relay desktop app.
//!
//! - runs the hub (see [`hub`]) so every AI app shares one browser link
//! - shows a popup for each transfer while approvals are set to "ask"
//! - lives in the menu bar / system tray; closing the window keeps it running

pub mod commands;
pub mod connect;
pub mod hub;

use relay_core::{ApprovalMode, Config};
use relay_mcp::Answer;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tracing_subscriber::EnvFilter;

use crate::hub::Hub;

/// Argument added when the app starts at login, so it starts hidden.
const STARTED_AT_LOGIN: &str = "--at-login";

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Show or hide the popup to match the queue.
fn sync_popup(app: &AppHandle, hub: &Hub) {
    let Some(w) = app.get_webview_window("approval") else {
        return;
    };
    let _ = app.emit_to("approval", "approvals-changed", ());
    if hub.queue.pending().is_empty() {
        let _ = w.hide();
    } else {
        let _ = w.center();
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        let _ = w.request_user_attention(Some(tauri::UserAttentionType::Critical));
    }
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config_path = match std::env::var_os("AI_FILE_RELAY_CONFIG") {
        Some(p) => p.into(),
        None => Config::default_path().expect("could not find a config folder for this system"),
    };
    let hub = Hub::new(config_path).expect("could not load the AI File Relay settings");
    let window_hub = hub.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // A second launch just brings the running app to the front.
            show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![STARTED_AT_LOGIN]),
        ))
        .manage(hub.clone())
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::add_folder,
            commands::remove_folder,
            commands::set_approval_mode,
            commands::history,
            commands::pending_approvals,
            commands::answer_approval,
            commands::clear_site_grants,
            commands::install_native_host,
            commands::connect_app,
            commands::disconnect_app,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            // The hub itself.
            tauri::async_runtime::spawn(hub.clone().run());

            // Popup follows the approval queue.
            let mut events = hub.queue.subscribe();
            let popup_hub = hub.clone();
            let popup_app = handle.clone();
            tauri::async_runtime::spawn(async move {
                // Every event (or a missed one) means: look at the queue again.
                while !matches!(
                    events.recv().await,
                    Err(tokio::sync::broadcast::error::RecvError::Closed)
                ) {
                    sync_popup(&popup_app, &popup_hub);
                }
            });

            // Tray menu.
            let open = MenuItem::with_id(app, "open", "Open AI File Relay", true, None::<&str>)?;
            let paused = hub.queue.mode() == ApprovalMode::DenyAll;
            let pause = CheckMenuItem::with_id(
                app,
                "pause",
                "Pause all transfers",
                true,
                paused,
                None::<&str>,
            )?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&open, &pause, &PredefinedMenuItem::separator(app)?, &quit],
            )?;
            let tray_hub = hub.clone();
            let pause_item = pause.clone();
            let tray_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
            TrayIconBuilder::with_id("main")
                .icon(tray_icon)
                .tooltip("AI File Relay")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "open" => show_main(app),
                    "pause" => {
                        let pause_now = tray_hub.queue.mode() != ApprovalMode::DenyAll;
                        let mode = if pause_now {
                            ApprovalMode::DenyAll
                        } else {
                            ApprovalMode::Ask
                        };
                        if let Err(e) = tray_hub.update_config(|c| c.approval = mode) {
                            tracing::warn!("could not change the approval mode: {e}");
                        }
                        let _ = pause_item.set_checked(pause_now);
                    }
                    "quit" => {
                        tray_hub.shutdown.cancel();
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;

            // Show the main window, unless we were started at login.
            let at_login = std::env::args().any(|a| a == STARTED_AT_LOGIN);
            if !at_login {
                show_main(&handle);
            }
            Ok(())
        })
        .on_window_event({
            let hub = window_hub;
            move |window, event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    // Keep running in the tray; the hub must stay up.
                    api.prevent_close();
                    let _ = window.hide();
                    if window.label() == "approval" {
                        // Closing the popup means "no" to what it showed.
                        for p in hub.queue.pending() {
                            hub.queue.answer(&p.id, Answer::Deny);
                        }
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running AI File Relay");
}
