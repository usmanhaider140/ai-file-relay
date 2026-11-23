//! Shared message types for AI File Relay.
//!
//! These types travel between the Chrome extension, the native messaging
//! host and the desktop hub. Every message is JSON with a `type` field.
//! In M1 these types will also be exported to TypeScript for the extension.

use serde::{Deserialize, Serialize};

/// Name Chrome uses to find our native messaging host.
pub const NATIVE_HOST_NAME: &str = "io.github.usmanhaider140.ai_file_relay";

/// Version of this message format. Bump it when a message changes shape.
pub const PROTOCOL_VERSION: u32 = 1;

/// Messages the extension sends to the hub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExtensionToHub {
    /// First message after connecting.
    Hello {
        extension_version: String,
        protocol_version: u32,
    },
    /// Health check.
    Ping,
    /// Result of an `UploadToPage` request.
    UploadResult {
        request_id: String,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Chrome finished a download.
    DownloadFinished {
        tab_id: Option<i64>,
        url: String,
        path: String,
    },
}

/// Messages the hub sends to the extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HubToExtension {
    /// Reply to `Hello`.
    Welcome {
        hub_version: String,
        protocol_version: u32,
    },
    /// Reply to `Ping`.
    Pong,
    /// Put a file into a page's upload box.
    ///
    /// The extension fetches the bytes from the hub with `token`; bytes never
    /// travel through native messaging (Chrome limits those messages to 1 MB).
    UploadToPage {
        request_id: String,
        tab_id: i64,
        token: String,
        file_name: String,
        mime_type: String,
        size: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
    },
    /// Something went wrong on the hub side.
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
