//! One hub for every AI app.
//!
//! The desktop app (or `relay-mcp serve`) is the hub: it owns the browser
//! link, the approval popup and the history. When an AI app starts
//! `relay-mcp` over stdio and a hub is already running, this module turns
//! that `relay-mcp` into a thin proxy: every MCP request is forwarded to the
//! hub's `/mcp` endpoint. So Claude Desktop, Claude Code and Cursor can all
//! reach the browser at the same time.

use std::time::Duration;

use relay_core::Config;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, ErrorData, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerConfig,
};
use rmcp::service::{RequestContext, RoleClient, RoleServer, RunningService, ServiceError};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::{ServerHandler, ServiceExt};
use serde::{Deserialize, Serialize};

/// What `GET /health` returns, so a client can tell our hub from any other
/// program that happens to use the port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubIdentity {
    pub app: String,
    pub version: String,
    /// "desktop" for the desktop app, "serve" for `relay-mcp serve`,
    /// "stdio" for a `relay-mcp` started by an AI app.
    pub role: String,
}

pub const APP_NAME: &str = "ai-file-relay";

impl HubIdentity {
    pub fn new(role: &str) -> Self {
        Self {
            app: APP_NAME.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            role: role.into(),
        }
    }
}

/// An HTTP client for talking to the hub on 127.0.0.1. It never uses a
/// system proxy: requests to our own machine must not leave it.
pub fn local_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .pool_max_idle_per_host(0)
        .build()
        .expect("could not build the local HTTP client")
}

/// Is our hub running on this port? Returns who it is.
pub async fn probe_hub(port: u16) -> Option<HubIdentity> {
    let url = format!("http://127.0.0.1:{port}/health");
    let res = local_client()
        .get(url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .ok()?;
    let id: HubIdentity = res.json().await.ok()?;
    (id.app == APP_NAME).then_some(id)
}

/// Forwards MCP requests to the hub.
pub struct HubProxy {
    hub: RunningService<RoleClient, rmcp::model::ClientConfig>,
}

impl std::fmt::Debug for HubProxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubProxy").finish_non_exhaustive()
    }
}

fn to_error(e: ServiceError) -> ErrorData {
    match e {
        ServiceError::McpError(e) => e,
        other => ErrorData::internal_error(
            format!("lost contact with the AI File Relay hub: {other}"),
            None,
        ),
    }
}

impl HubProxy {
    /// Connect to the hub's MCP endpoint.
    pub async fn connect(config: &Config) -> anyhow::Result<Self> {
        let url = format!("http://127.0.0.1:{}/mcp", config.port);
        let transport = StreamableHttpClientTransport::with_client(
            local_client(),
            StreamableHttpClientTransportConfig::with_uri(url).auth_header(config.token.clone()),
        );
        let hub = rmcp::model::ClientConfig::default()
            .serve(transport)
            .await?;
        Ok(Self { hub })
    }
}

impl ServerHandler for HubProxy {
    fn get_info(&self) -> ServerConfig {
        // Show the hub's own name and instructions to the AI app.
        let mut config = ServerConfig::new(ServerCapabilities::builder().enable_tools().build());
        if let Some(info) = self.hub.peer_info() {
            if let Some(server) = info.server_info.clone() {
                config = config.with_server_info(server);
            }
            if let Some(text) = info.instructions.clone() {
                config = config.with_instructions(text);
            }
        }
        config
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        self.hub.list_tools(request).await.map_err(to_error)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.hub.call_tool_once(request).await.map_err(to_error)
    }
}
