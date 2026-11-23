//! Commands the app's windows call (with `invoke`).

use std::path::PathBuf;
use std::sync::Arc;

use relay_core::{ApprovalMode, Event};
use relay_mcp::browser::{BridgeStatus, BrowserBridge};
use relay_mcp::{Answer, PendingApproval, install};
use serde::Serialize;
use tauri::State;

use crate::connect::{self, AiApp};
use crate::hub::{Hub, HubState};

type HubRef<'a> = State<'a, Arc<Hub>>;

#[derive(Debug, Serialize)]
pub struct AppConnection {
    pub id: AiApp,
    pub name: &'static str,
    pub installed: bool,
    pub connected: bool,
    pub config_file: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SiteGrant {
    pub site: String,
    pub action: relay_core::Action,
    pub until_ms: u64,
}

/// Everything the main window shows.
#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub version: &'static str,
    pub hub: HubState,
    pub extension_connected: bool,
    pub extension_detail: String,
    pub approval_mode: ApprovalMode,
    pub allowed_folders: Vec<String>,
    pub missing_folders: Vec<String>,
    pub native_host_path: Option<String>,
    pub relay_mcp_path: Option<String>,
    pub apps: Vec<AppConnection>,
    pub claude_code_command: Option<String>,
    pub pending: usize,
    pub site_grants: Vec<SiteGrant>,
    pub config_path: String,
}

/// Where the bundled `relay-mcp` lives: next to this app's executable
/// (Tauri puts sidecars there), or on the PATH as a fallback.
pub fn relay_mcp_path() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "relay-mcp.exe"
    } else {
        "relay-mcp"
    };
    let beside = std::env::current_exe().ok()?.parent()?.join(name);
    if beside.is_file() {
        return Some(beside);
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|p| p.join(name))
            .find(|p| p.is_file())
    })
}

fn show(p: &std::path::Path) -> String {
    p.display().to_string()
}

#[tauri::command]
pub fn get_state(hub: HubRef<'_>) -> Snapshot {
    let config = hub.config();
    let (extension_connected, extension_detail) = match hub.bridge.status() {
        BridgeStatus::Connected { extension_version } => {
            (true, format!("extension {extension_version}"))
        }
        BridgeStatus::NotConnected { reason } => (false, reason),
    };
    let relay_mcp = relay_mcp_path();
    let apps = AiApp::ALL
        .into_iter()
        .map(|app| {
            let file = app.config_file();
            AppConnection {
                id: app,
                name: app.name(),
                installed: app.looks_installed(),
                connected: file.as_deref().is_some_and(connect::is_connected),
                config_file: file.as_deref().map(show),
            }
        })
        .collect();
    Snapshot {
        version: env!("CARGO_PKG_VERSION"),
        hub: hub.state(),
        extension_connected,
        extension_detail,
        approval_mode: hub.queue.mode(),
        allowed_folders: config.allowed_folders.iter().map(|p| show(p)).collect(),
        missing_folders: hub
            .relay
            .missing_folders()
            .iter()
            .map(|p| show(p))
            .collect(),
        native_host_path: install::installed_path().as_deref().map(show),
        relay_mcp_path: relay_mcp.as_deref().map(show),
        apps,
        claude_code_command: relay_mcp.as_deref().map(connect::claude_code_command),
        pending: hub.queue.pending().len(),
        site_grants: hub
            .queue
            .site_grants()
            .into_iter()
            .map(|(site, action, until_ms)| SiteGrant {
                site,
                action,
                until_ms,
            })
            .collect(),
        config_path: show(&hub.config_path),
    }
}

#[tauri::command]
pub fn add_folder(hub: HubRef<'_>, path: String) -> Result<(), String> {
    let mut result = Ok(());
    hub.update_config(|c| {
        if let Err(e) = c.allow(&path) {
            result = Err(e.to_string());
        }
    })?;
    result
}

#[tauri::command]
pub fn remove_folder(hub: HubRef<'_>, path: String) -> Result<(), String> {
    hub.update_config(|c| {
        c.disallow(&path);
    })?;
    Ok(())
}

#[tauri::command]
pub fn set_approval_mode(hub: HubRef<'_>, mode: ApprovalMode) -> Result<(), String> {
    hub.update_config(|c| c.approval = mode)?;
    Ok(())
}

#[tauri::command]
pub fn history(hub: HubRef<'_>, limit: usize) -> Vec<Event> {
    hub.relay.history.recent(limit.min(500))
}

#[tauri::command]
pub fn pending_approvals(hub: HubRef<'_>) -> Vec<PendingApproval> {
    hub.queue.pending()
}

#[tauri::command]
pub fn answer_approval(hub: HubRef<'_>, id: String, answer: Answer) -> bool {
    hub.queue.answer(&id, answer)
}

#[tauri::command]
pub fn clear_site_grants(hub: HubRef<'_>) {
    hub.queue.clear_site_grants();
}

#[tauri::command]
pub fn install_native_host() -> Result<Vec<String>, String> {
    let exe = relay_mcp_path().ok_or(
        "could not find relay-mcp next to the app. Reinstall AI File Relay, or install \
         relay-mcp with cargo.",
    )?;
    let done = install::install(&exe, &install::extension_ids(&[]))?;
    Ok(done
        .into_iter()
        .map(|d| format!("{}: {}", d.browser, d.location))
        .collect())
}

#[tauri::command]
pub fn connect_app(app: AiApp) -> Result<String, String> {
    let exe = relay_mcp_path().ok_or("could not find relay-mcp next to the app")?;
    let file = app
        .config_file()
        .ok_or("could not find this app's settings folder")?;
    connect::connect(&file, &exe)?;
    Ok(format!(
        "Added AI File Relay to {}. Restart {} to use it.",
        file.display(),
        app.name()
    ))
}

#[tauri::command]
pub fn disconnect_app(app: AiApp) -> Result<(), String> {
    let file = app
        .config_file()
        .ok_or("could not find this app's settings folder")?;
    connect::disconnect(&file)
}
