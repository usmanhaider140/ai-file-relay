//! The MCP tools.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{Json, ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::browser::BridgeStatus;
use crate::relay::Relay;

/// Longest a tool will wait for a download.
const MAX_WAIT_SECONDS: u64 = 600;
const DEFAULT_WAIT_SECONDS: u64 = 60;

const INSTRUCTIONS: &str = "AI File Relay moves files between this computer and web pages in the \
user's Chrome browser. Use list_files to see what you may use (only folders the user allowed). \
Use list_tabs to find a tab, then upload_to_page to put a file into that page's upload box. \
Use wait_for_download to get a file the page downloads. The user may be asked to approve \
transfers. Call relay_status if something does not work.";

// ---------- tool inputs ----------

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ListFilesParams {
    /// Folder to list. Leave empty to see the allowed folders.
    #[serde(default)]
    pub folder: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UploadParams {
    /// Full path of the file to upload. It must be inside an allowed folder.
    pub path: String,
    /// Id of the browser tab, from list_tabs.
    pub tab_id: i64,
    /// Optional CSS selector of the file input, if the page has more than one.
    #[serde(default)]
    pub selector: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct WaitForDownloadParams {
    /// Only count downloads started from this tab. Leave empty for any tab.
    #[serde(default)]
    pub tab_id: Option<i64>,
    /// How long to wait, in seconds (default 60, max 600).
    #[serde(default)]
    pub timeout_seconds: Option<u64>,
}

// ---------- tool outputs ----------

#[derive(Debug, Serialize, JsonSchema)]
pub struct FileItem {
    pub name: String,
    pub path: String,
    /// "folder" or "file".
    pub kind: String,
    /// Size in bytes, for files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListFilesResult {
    /// The folder that was listed, or empty when showing the allowed folders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    pub items: Vec<FileItem>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TabItem {
    pub id: i64,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListTabsResult {
    pub tabs: Vec<TabItem>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct UploadResult {
    pub file_name: String,
    pub size: u64,
    pub tab_id: i64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DownloadResult {
    /// Where the downloaded file now is.
    pub path: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TransferItem {
    pub time_ms: u64,
    pub action: String,
    pub path: String,
    /// "done", "denied" or "failed".
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct StatusResult {
    pub version: String,
    /// Who runs the hub: "desktop", "serve" or "stdio".
    pub hub: String,
    pub allowed_folders: Vec<String>,
    /// Folders in the config that no longer exist.
    pub missing_folders: Vec<String>,
    /// "ask", "allow_all" or "deny_all".
    pub approval: String,
    pub browser_connected: bool,
    pub browser_detail: String,
    /// Base URL of the local file server, if it is running.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_server: Option<String>,
    pub recent_transfers: Vec<TransferItem>,
}

// ---------- server ----------

/// The MCP server. Cheap to clone; all clones share one [`Relay`].
#[derive(Clone)]
pub struct RelayServer {
    relay: Arc<Relay>,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for RelayServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelayServer").finish_non_exhaustive()
    }
}

fn show(path: &std::path::Path) -> String {
    path.display().to_string()
}

#[tool_router]
impl RelayServer {
    pub fn new(relay: Arc<Relay>) -> Self {
        Self {
            relay,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "List files you are allowed to use. With no folder, shows the allowed folders. \
                       With a folder, lists what is inside it (not subfolders)."
    )]
    async fn list_files(
        &self,
        Parameters(p): Parameters<ListFilesParams>,
    ) -> Result<Json<ListFilesResult>, String> {
        let Some(folder) = p.folder.filter(|f| !f.trim().is_empty()) else {
            let roots = self.relay.allowed_roots();
            if roots.is_empty() {
                return Err(
                    "no folders are allowed yet. The user can add one in the AI File \
                            Relay app, or with `relay-mcp allow <folder>`."
                        .into(),
                );
            }
            let items = roots
                .iter()
                .map(|root| FileItem {
                    name: root
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| show(root)),
                    path: show(root),
                    kind: "folder".into(),
                    size: None,
                })
                .collect();
            return Ok(Json(ListFilesResult {
                folder: None,
                items,
            }));
        };

        let entries = self.relay.list_folder(&folder).map_err(|e| e.to_string())?;
        let items = entries
            .into_iter()
            .map(|e| FileItem {
                name: e.name,
                path: show(&e.path),
                kind: match e.kind {
                    relay_core::EntryKind::Folder => "folder".into(),
                    relay_core::EntryKind::File => "file".into(),
                },
                size: e.size,
            })
            .collect();
        Ok(Json(ListFilesResult {
            folder: Some(folder),
            items,
        }))
    }

    #[tool(description = "List the open tabs in the user's Chrome browser (id, title, URL).")]
    async fn list_tabs(&self) -> Result<Json<ListTabsResult>, String> {
        let tabs = self
            .relay
            .browser
            .list_tabs()
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ListTabsResult {
            tabs: tabs
                .into_iter()
                .map(|t| TabItem {
                    id: t.id,
                    title: t.title,
                    url: t.url,
                })
                .collect(),
        }))
    }

    #[tool(
        description = "Put a local file into the upload box (file input) of a page in the user's \
                       browser. The file must be inside an allowed folder. The user may need to \
                       approve it."
    )]
    async fn upload_to_page(
        &self,
        Parameters(p): Parameters<UploadParams>,
    ) -> Result<Json<UploadResult>, String> {
        let done = self.relay.upload(&p.path, p.tab_id, p.selector).await?;
        Ok(Json(UploadResult {
            file_name: done.file_name,
            size: done.size,
            tab_id: done.tab_id,
        }))
    }

    #[tool(
        description = "Wait for the next file the browser downloads and return its path on this \
                       computer. Start the download first (or right after calling this)."
    )]
    async fn wait_for_download(
        &self,
        Parameters(p): Parameters<WaitForDownloadParams>,
    ) -> Result<Json<DownloadResult>, String> {
        let secs = p
            .timeout_seconds
            .unwrap_or(DEFAULT_WAIT_SECONDS)
            .clamp(1, MAX_WAIT_SECONDS);
        let path: PathBuf = self
            .relay
            .wait_for_download(p.tab_id, Duration::from_secs(secs))
            .await?;
        Ok(Json(DownloadResult { path: show(&path) }))
    }

    #[tool(
        description = "Show whether AI File Relay is set up and connected, and recent transfers."
    )]
    async fn relay_status(&self) -> Json<StatusResult> {
        let relay = &self.relay;
        let (browser_connected, browser_detail) = match relay.browser.status() {
            BridgeStatus::Connected { extension_version } => {
                (true, format!("extension {extension_version} connected"))
            }
            BridgeStatus::NotConnected { reason } => (false, reason),
        };
        let approval = serde_json::to_value(relay.approver.mode())
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let recent_transfers = relay
            .history
            .recent(5)
            .into_iter()
            .map(|e| {
                let (result, reason) = match e.outcome {
                    relay_core::Outcome::Done => ("done", None),
                    relay_core::Outcome::Denied(r) => ("denied", Some(r)),
                    relay_core::Outcome::Failed(r) => ("failed", Some(r)),
                };
                TransferItem {
                    time_ms: e.time_ms,
                    action: format!("{:?}", e.action).to_lowercase(),
                    path: show(&e.path),
                    result: result.into(),
                    reason,
                }
            })
            .collect();
        Json(StatusResult {
            version: env!("CARGO_PKG_VERSION").into(),
            hub: relay.hub_role.clone(),
            allowed_folders: relay.allowed_roots().iter().map(|p| show(p)).collect(),
            missing_folders: relay.missing_folders().iter().map(|p| show(p)).collect(),
            approval,
            browser_connected,
            browser_detail,
            file_server: relay.file_server.clone(),
            recent_transfers,
        })
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for RelayServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("ai-file-relay", env!("CARGO_PKG_VERSION"))
                    .with_title("AI File Relay"),
            )
            .with_instructions(INSTRUCTIONS)
    }
}
