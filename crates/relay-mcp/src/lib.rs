//! MCP server for AI File Relay.
//!
//! - [`relay`]: the shared state (settings, allowed folders, tokens, history)
//! - [`approvals`]: approvals a person answers (used by the desktop app)
//! - [`browser`]: what the tools need from the browser
//! - [`extension`]: the real link to the Chrome extension
//! - [`native_host`]: the program Chrome starts (native messaging host)
//! - [`install`]: registers the native host with Chrome
//! - [`proxy`]: forwards a stdio `relay-mcp` to a running hub
//! - [`server`]: the MCP tools
//! - [`http`]: the localhost HTTP server (MCP endpoint + one-time file links)

pub mod approvals;
pub mod browser;
pub mod extension;
pub mod http;
pub mod install;
pub mod native_host;
pub mod proxy;
pub mod relay;
pub mod server;

pub use approvals::{Answer, ApprovalQueue, PendingApproval, QueueEvent};
pub use extension::ExtensionBridge;
pub use relay::Relay;
pub use server::RelayServer;
