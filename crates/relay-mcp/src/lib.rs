//! MCP server for AI File Relay.
//!
//! - [`relay`]: the shared state (settings, allowed folders, tokens, history)
//! - [`browser`]: the link to the Chrome extension (connected in M2)
//! - [`server`]: the MCP tools
//! - [`http`]: the localhost HTTP server (MCP endpoint + one-time file links)

pub mod browser;
pub mod http;
pub mod relay;
pub mod server;

pub use relay::Relay;
pub use server::RelayServer;
