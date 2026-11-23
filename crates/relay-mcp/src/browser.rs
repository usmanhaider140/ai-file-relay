//! The link to the browser.
//!
//! The MCP tools talk to the browser through [`BrowserBridge`]. In M1 the
//! only bridge is [`NotConnected`], so browser tools explain that the
//! Chrome extension is not built yet. M2 adds the real bridge, which talks
//! to the extension through the native messaging host.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

/// A boxed future, so the trait can be used as `dyn BrowserBridge`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// An open browser tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    pub id: i64,
    pub title: String,
    pub url: String,
}

/// Everything the extension needs to put one file into a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadJob {
    pub request_id: String,
    pub tab_id: i64,
    /// One-time token; the extension downloads the bytes with it.
    pub token: String,
    /// Full URL of the bytes, for example `http://127.0.0.1:41234/files/<token>`.
    pub file_url: String,
    pub file_name: String,
    pub mime_type: String,
    pub size: u64,
    /// CSS selector of the file input, when the AI knows it.
    pub selector: Option<String>,
}

/// A download Chrome finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishedDownload {
    /// Where Chrome saved it.
    pub path: PathBuf,
    /// Where it came from.
    pub url: String,
}

/// Is the extension there?
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeStatus {
    Connected { extension_version: String },
    NotConnected { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BridgeError {
    #[error("the browser is not connected: {0}")]
    NotConnected(String),
    #[error("timed out waiting for the browser")]
    Timeout,
    #[error("the browser reported an error: {0}")]
    Failed(String),
}

/// What the MCP tools need from the browser.
pub trait BrowserBridge: Send + Sync {
    fn status(&self) -> BridgeStatus;
    fn list_tabs(&self) -> BoxFuture<'_, Result<Vec<Tab>, BridgeError>>;
    fn upload(&self, job: UploadJob) -> BoxFuture<'_, Result<(), BridgeError>>;
    fn wait_for_download(
        &self,
        tab_id: Option<i64>,
        timeout: Duration,
    ) -> BoxFuture<'_, Result<FinishedDownload, BridgeError>>;
}

/// The bridge used until the extension exists (M2).
#[derive(Debug, Default, Clone, Copy)]
pub struct NotConnected;

const NOT_BUILT: &str = "the Chrome extension link is coming in M2";

impl BrowserBridge for NotConnected {
    fn status(&self) -> BridgeStatus {
        BridgeStatus::NotConnected {
            reason: NOT_BUILT.into(),
        }
    }

    fn list_tabs(&self) -> BoxFuture<'_, Result<Vec<Tab>, BridgeError>> {
        Box::pin(async { Err(BridgeError::NotConnected(NOT_BUILT.into())) })
    }

    fn upload(&self, _job: UploadJob) -> BoxFuture<'_, Result<(), BridgeError>> {
        Box::pin(async { Err(BridgeError::NotConnected(NOT_BUILT.into())) })
    }

    fn wait_for_download(
        &self,
        _tab_id: Option<i64>,
        _timeout: Duration,
    ) -> BoxFuture<'_, Result<FinishedDownload, BridgeError>> {
        Box::pin(async { Err(BridgeError::NotConnected(NOT_BUILT.into())) })
    }
}
