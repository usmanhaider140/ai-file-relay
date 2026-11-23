//! Shared message types for AI File Relay.
//!
//! These types travel between the Chrome extension, the native messaging
//! host and the hub. Every message is JSON with a `type` field.
//! `cargo test` also writes these types as TypeScript to
//! `apps/extension/src/generated/` (with ts-rs), so the extension always
//! matches. Do not edit the generated files by hand.
//!
//! ```text
//! extension ──native messaging──▶ host (relay-mcp) ──WebSocket /bridge──▶ hub
//! ```
//!
//! The host passes messages through unchanged. It only sends one message of
//! its own: [`HubToExtension::HubStatus`], when the hub connects or goes away.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Name Chrome uses to find our native messaging host.
pub const NATIVE_HOST_NAME: &str = "io.github.usmanhaider140.ai_file_relay";

/// ID of the extension when loaded from `apps/extension` (pinned by the
/// `key` in its manifest). The native host only accepts this extension.
pub const DEV_EXTENSION_ID: &str = "cccoeijnpfnpkdehiobanmiphloageco";

/// Version of this message format. Bump it when a message changes shape.
pub const PROTOCOL_VERSION: u32 = 2;

/// An open browser tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "protocol.ts")]
pub struct TabInfo {
    #[ts(type = "number")]
    pub id: i64,
    pub title: String,
    pub url: String,
}

/// Messages the extension sends to the hub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export, export_to = "protocol.ts")]
pub enum ExtensionToHub {
    /// First message after connecting.
    Hello {
        extension_version: String,
        protocol_version: u32,
    },
    /// Health check.
    Ping,
    /// Reply to `ListTabs`.
    Tabs {
        request_id: String,
        tabs: Vec<TabInfo>,
    },
    /// Reply to `UploadToPage`.
    UploadResult {
        request_id: String,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        error: Option<String>,
    },
    /// Reply to `WatchDownload`: a download finished.
    DownloadFinished {
        request_id: String,
        url: String,
        /// Where Chrome saved the file.
        path: String,
    },
    /// Reply to `WatchDownload`: no download, or it failed.
    DownloadFailed { request_id: String, error: String },
}

impl ExtensionToHub {
    /// The request this message answers, if it is an answer.
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::Tabs { request_id, .. }
            | Self::UploadResult { request_id, .. }
            | Self::DownloadFinished { request_id, .. }
            | Self::DownloadFailed { request_id, .. } => Some(request_id),
            Self::Hello { .. } | Self::Ping => None,
        }
    }
}

/// Messages the hub (or the host) sends to the extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export, export_to = "protocol.ts")]
pub enum HubToExtension {
    /// Reply to `Hello`.
    Welcome {
        hub_version: String,
        protocol_version: u32,
    },
    /// Reply to `Ping`.
    Pong,
    /// Sent by the native host when the hub connects or goes away.
    HubStatus {
        connected: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        detail: Option<String>,
    },
    /// List the open tabs.
    ListTabs { request_id: String },
    /// Put a file into a page's upload box.
    ///
    /// The extension downloads the bytes from `file_url` (a one-time link on
    /// the hub); bytes never travel through native messaging, because Chrome
    /// limits those messages to 1 MB.
    UploadToPage {
        request_id: String,
        #[ts(type = "number")]
        tab_id: i64,
        token: String,
        /// Full URL of the bytes on the hub, for example
        /// `http://127.0.0.1:41234/files/<token>`.
        file_url: String,
        file_name: String,
        mime_type: String,
        #[ts(type = "number")]
        size: u64,
        /// CSS selector of the file input or drop zone, if the AI knows it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        selector: Option<String>,
    },
    /// Report the next download that finishes, within `timeout_ms`.
    WatchDownload {
        request_id: String,
        /// Only count downloads started from this tab's site.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional, type = "number")]
        tab_id: Option<i64>,
        #[ts(type = "number")]
        timeout_ms: u64,
    },
    /// Something went wrong on the hub side.
    Error { message: String },
}

/// The constants above as TypeScript, for the extension.
pub fn constants_ts() -> String {
    format!(
        "// This file was generated from crates/relay-protocol by `cargo test`. Do not edit it.\n\n\
         export const NATIVE_HOST_NAME = \"{NATIVE_HOST_NAME}\";\n\
         export const DEV_EXTENSION_ID = \"{DEV_EXTENSION_ID}\";\n\
         export const PROTOCOL_VERSION = {PROTOCOL_VERSION};\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes `constants.ts` next to the ts-rs output, so the extension
    /// always uses the same name, ID and version as the Rust side.
    #[test]
    fn export_constants() {
        let Some(dir) = std::env::var_os("TS_RS_EXPORT_DIR") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("constants.ts"), constants_ts()).unwrap();
    }

    #[test]
    fn ping_uses_type_tag() {
        let json = serde_json::to_string(&ExtensionToHub::Ping).unwrap();
        assert_eq!(json, r#"{"type":"ping"}"#);
    }

    #[test]
    fn upload_round_trip() {
        let msg = HubToExtension::UploadToPage {
            request_id: "r1".into(),
            tab_id: 7,
            token: "abc".into(),
            file_url: "http://127.0.0.1:1/files/abc".into(),
            file_name: "report.pdf".into(),
            mime_type: "application/pdf".into(),
            size: 1024,
            selector: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(!json.contains("selector"));
        let back: HubToExtension = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);
    }

    #[test]
    fn upload_result_error_is_optional() {
        let msg: ExtensionToHub =
            serde_json::from_str(r#"{"type":"upload_result","request_id":"r1","ok":true}"#)
                .unwrap();
        assert_eq!(
            msg,
            ExtensionToHub::UploadResult {
                request_id: "r1".into(),
                ok: true,
                error: None
            }
        );
    }

    #[test]
    fn answers_know_their_request() {
        let tabs: ExtensionToHub = serde_json::from_str(
            r#"{"type":"tabs","request_id":"r9","tabs":[{"id":3,"title":"T","url":"https://a.b/"}]}"#,
        )
        .unwrap();
        assert_eq!(tabs.request_id(), Some("r9"));
        assert_eq!(ExtensionToHub::Ping.request_id(), None);
    }

    #[test]
    fn watch_download_without_tab() {
        let json = serde_json::to_string(&HubToExtension::WatchDownload {
            request_id: "r1".into(),
            tab_id: None,
            timeout_ms: 5000,
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"type":"watch_download","request_id":"r1","timeout_ms":5000}"#
        );
    }

    #[test]
    fn dev_extension_id_looks_right() {
        assert_eq!(DEV_EXTENSION_ID.len(), 32);
        assert!(DEV_EXTENSION_ID.chars().all(|c| ('a'..='p').contains(&c)));
    }
}
