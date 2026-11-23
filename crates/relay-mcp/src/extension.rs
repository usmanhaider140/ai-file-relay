//! The real browser bridge: talks to the Chrome extension.
//!
//! The native messaging host (started by Chrome) connects to the hub's
//! `/bridge` WebSocket and passes messages through. This module keeps that
//! connection and matches each answer to the request that asked for it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use relay_protocol::{ExtensionToHub, HubToExtension, PROTOCOL_VERSION};
use tokio::sync::{mpsc, oneshot};

use crate::browser::{
    BoxFuture, BridgeError, BridgeStatus, BrowserBridge, FinishedDownload, Tab, UploadJob,
};

const LIST_TABS_TIMEOUT: Duration = Duration::from_secs(10);
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);
/// Extra time on top of the download timeout, for the answer to travel back.
const DOWNLOAD_GRACE: Duration = Duration::from_secs(10);

const NOT_CONNECTED: &str = "the Chrome extension is not connected. Check that Chrome is open, \
the AI File Relay extension is installed, and `relay-mcp install-native-host` was run";

/// Connection to the extension, shared by the MCP tools and the `/bridge`
/// WebSocket. Cheap to clone.
#[derive(Clone, Default)]
pub struct ExtensionBridge {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    conn: Mutex<Option<Conn>>,
    pending: Mutex<HashMap<String, oneshot::Sender<ExtensionToHub>>>,
    next_id: AtomicU64,
}

struct Conn {
    id: u64,
    tx: mpsc::UnboundedSender<HubToExtension>,
    /// Set after a good `hello`.
    extension_version: Option<String>,
    problem: Option<String>,
}

impl std::fmt::Debug for ExtensionBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionBridge")
            .field("status", &self.current_status())
            .finish()
    }
}

impl ExtensionBridge {
    pub fn new() -> Self {
        Self::default()
    }

    /// A new connection arrived. It replaces any older one. Returns its id
    /// and the channel of messages to send to it.
    pub fn attach(&self) -> (u64, mpsc::UnboundedReceiver<HubToExtension>) {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::unbounded_channel();
        let old = self.lock_conn().replace(Conn {
            id,
            tx,
            extension_version: None,
            problem: None,
        });
        if old.is_some() {
            tracing::info!("a new extension connection replaced the old one");
        }
        (id, rx)
    }

    /// A connection closed.
    pub fn detach(&self, id: u64) {
        let mut conn = self.lock_conn();
        if conn.as_ref().is_some_and(|c| c.id == id) {
            *conn = None;
            drop(conn);
            // Requests still waiting can never be answered now; dropping
            // their senders wakes them with an error.
            self.inner
                .pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
            tracing::info!("extension disconnected");
        }
    }

    /// Handle one message from the extension. Returns a reply to send back,
    /// if any.
    pub fn handle_incoming(&self, id: u64, msg: ExtensionToHub) -> Option<HubToExtension> {
        match msg {
            ExtensionToHub::Hello {
                extension_version,
                protocol_version,
            } => {
                let mut conn = self.lock_conn();
                let conn = conn.as_mut().filter(|c| c.id == id)?;
                if protocol_version != PROTOCOL_VERSION {
                    let problem = format!(
                        "the extension speaks protocol {protocol_version} but this hub speaks \
                         {PROTOCOL_VERSION}; update both to the same version"
                    );
                    tracing::warn!("{problem}");
                    conn.problem = Some(problem.clone());
                    return Some(HubToExtension::Error { message: problem });
                }
                tracing::info!("extension {extension_version} connected");
                conn.extension_version = Some(extension_version);
                conn.problem = None;
                Some(HubToExtension::Welcome {
                    hub_version: env!("CARGO_PKG_VERSION").into(),
                    protocol_version: PROTOCOL_VERSION,
                })
            }
            ExtensionToHub::Ping => Some(HubToExtension::Pong),
            answer => {
                let request_id = answer.request_id().unwrap_or_default().to_owned();
                let waiter = self
                    .inner
                    .pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&request_id);
                match waiter {
                    Some(tx) => {
                        let _ = tx.send(answer);
                    }
                    None => tracing::debug!("answer for unknown request {request_id}"),
                }
                None
            }
        }
    }

    fn lock_conn(&self) -> std::sync::MutexGuard<'_, Option<Conn>> {
        self.inner.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn current_status(&self) -> BridgeStatus {
        match self.lock_conn().as_ref() {
            Some(Conn {
                extension_version: Some(v),
                ..
            }) => BridgeStatus::Connected {
                extension_version: v.clone(),
            },
            Some(Conn {
                problem: Some(p), ..
            }) => BridgeStatus::NotConnected { reason: p.clone() },
            _ => BridgeStatus::NotConnected {
                reason: NOT_CONNECTED.into(),
            },
        }
    }

    /// Send a request and wait for its answer.
    async fn request(
        &self,
        build: impl FnOnce(String) -> HubToExtension,
        timeout: Duration,
    ) -> Result<ExtensionToHub, BridgeError> {
        if let BridgeStatus::NotConnected { reason } = self.current_status() {
            return Err(BridgeError::NotConnected(reason));
        }
        let request_id = relay_core::random_hex(8);
        let (tx, rx) = oneshot::channel();
        self.inner
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(request_id.clone(), tx);

        let sent = self
            .lock_conn()
            .as_ref()
            .map(|c| c.tx.send(build(request_id.clone())).is_ok())
            .unwrap_or(false);
        if !sent {
            self.forget(&request_id);
            return Err(BridgeError::NotConnected(NOT_CONNECTED.into()));
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(answer)) => Ok(answer),
            Ok(Err(_)) => Err(BridgeError::NotConnected(
                "the extension disconnected before answering".into(),
            )),
            Err(_) => {
                self.forget(&request_id);
                Err(BridgeError::Timeout)
            }
        }
    }

    fn forget(&self, request_id: &str) {
        self.inner
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(request_id);
    }
}

fn unexpected(answer: &ExtensionToHub) -> BridgeError {
    BridgeError::Failed(format!("unexpected answer from the extension: {answer:?}"))
}

impl BrowserBridge for ExtensionBridge {
    fn status(&self) -> BridgeStatus {
        self.current_status()
    }

    fn list_tabs(&self) -> BoxFuture<'_, Result<Vec<Tab>, BridgeError>> {
        Box::pin(async move {
            let answer = self
                .request(
                    |request_id| HubToExtension::ListTabs { request_id },
                    LIST_TABS_TIMEOUT,
                )
                .await?;
            match answer {
                ExtensionToHub::Tabs { tabs, .. } => Ok(tabs
                    .into_iter()
                    .map(|t| Tab {
                        id: t.id,
                        title: t.title,
                        url: t.url,
                    })
                    .collect()),
                other => Err(unexpected(&other)),
            }
        })
    }

    fn upload(&self, job: UploadJob) -> BoxFuture<'_, Result<(), BridgeError>> {
        Box::pin(async move {
            let answer = self
                .request(
                    |request_id| HubToExtension::UploadToPage {
                        request_id,
                        tab_id: job.tab_id,
                        token: job.token,
                        file_url: job.file_url,
                        file_name: job.file_name,
                        mime_type: job.mime_type,
                        size: job.size,
                        selector: job.selector,
                    },
                    UPLOAD_TIMEOUT,
                )
                .await?;
            match answer {
                ExtensionToHub::UploadResult { ok: true, .. } => Ok(()),
                ExtensionToHub::UploadResult { error, .. } => Err(BridgeError::Failed(
                    error.unwrap_or_else(|| "upload failed".into()),
                )),
                other => Err(unexpected(&other)),
            }
        })
    }

    fn wait_for_download(
        &self,
        tab_id: Option<i64>,
        timeout: Duration,
    ) -> BoxFuture<'_, Result<FinishedDownload, BridgeError>> {
        Box::pin(async move {
            let timeout_ms = u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX);
            let answer = self
                .request(
                    |request_id| HubToExtension::WatchDownload {
                        request_id,
                        tab_id,
                        timeout_ms,
                    },
                    timeout + DOWNLOAD_GRACE,
                )
                .await?;
            match answer {
                ExtensionToHub::DownloadFinished { url, path, .. } => Ok(FinishedDownload {
                    path: PathBuf::from(path),
                    url,
                }),
                ExtensionToHub::DownloadFailed { error, .. } => Err(BridgeError::Failed(error)),
                other => Err(unexpected(&other)),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relay_protocol::TabInfo;

    /// Pretend to be the extension on the other end of a connection.
    fn fake_extension(bridge: ExtensionBridge) -> u64 {
        let (id, mut rx) = bridge.attach();
        let hello = bridge.handle_incoming(
            id,
            ExtensionToHub::Hello {
                extension_version: "9.9.9".into(),
                protocol_version: PROTOCOL_VERSION,
            },
        );
        assert!(matches!(hello, Some(HubToExtension::Welcome { .. })));
        let b = bridge.clone();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let answer = match msg {
                    HubToExtension::ListTabs { request_id } => ExtensionToHub::Tabs {
                        request_id,
                        tabs: vec![TabInfo {
                            id: 5,
                            title: "Upload test".into(),
                            url: "https://example.com/upload".into(),
                        }],
                    },
                    HubToExtension::UploadToPage {
                        request_id,
                        file_name,
                        ..
                    } => ExtensionToHub::UploadResult {
                        request_id,
                        ok: file_name != "bad.txt",
                        error: (file_name == "bad.txt").then(|| "no file input".into()),
                    },
                    HubToExtension::WatchDownload { request_id, .. } => {
                        ExtensionToHub::DownloadFinished {
                            request_id,
                            url: "https://example.com/a.csv".into(),
                            path: "/tmp/a.csv".into(),
                        }
                    }
                    _ => continue,
                };
                b.handle_incoming(id, answer);
            }
        });
        id
    }

    fn job(name: &str) -> UploadJob {
        UploadJob {
            request_id: "x".into(),
            tab_id: 5,
            token: "t".into(),
            file_url: "http://127.0.0.1:1/files/t".into(),
            file_name: name.into(),
            mime_type: "text/plain".into(),
            size: 1,
            selector: None,
        }
    }

    #[tokio::test]
    async fn not_connected_until_hello() {
        let bridge = ExtensionBridge::new();
        assert!(matches!(bridge.status(), BridgeStatus::NotConnected { .. }));
        let (_id, _rx) = bridge.attach();
        assert!(matches!(bridge.status(), BridgeStatus::NotConnected { .. }));
        assert!(matches!(
            bridge.list_tabs().await,
            Err(BridgeError::NotConnected(_))
        ));
    }

    #[tokio::test]
    async fn requests_get_their_answers() {
        let bridge = ExtensionBridge::new();
        fake_extension(bridge.clone());
        assert_eq!(
            bridge.status(),
            BridgeStatus::Connected {
                extension_version: "9.9.9".into()
            }
        );
        let tabs = bridge.list_tabs().await.unwrap();
        assert_eq!(tabs[0].id, 5);
        bridge.upload(job("ok.txt")).await.unwrap();
        let err = bridge.upload(job("bad.txt")).await.unwrap_err();
        assert_eq!(err, BridgeError::Failed("no file input".into()));
        let dl = bridge
            .wait_for_download(None, Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(dl.path, PathBuf::from("/tmp/a.csv"));
    }

    #[tokio::test]
    async fn wrong_protocol_version_is_reported() {
        let bridge = ExtensionBridge::new();
        let (id, _rx) = bridge.attach();
        let reply = bridge.handle_incoming(
            id,
            ExtensionToHub::Hello {
                extension_version: "0.0.1".into(),
                protocol_version: PROTOCOL_VERSION + 1,
            },
        );
        assert!(matches!(reply, Some(HubToExtension::Error { .. })));
        match bridge.status() {
            BridgeStatus::NotConnected { reason } => assert!(reason.contains("protocol")),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn disconnect_wakes_waiting_requests() {
        let bridge = ExtensionBridge::new();
        let (id, _rx) = bridge.attach();
        bridge.handle_incoming(
            id,
            ExtensionToHub::Hello {
                extension_version: "1".into(),
                protocol_version: PROTOCOL_VERSION,
            },
        );
        let b = bridge.clone();
        let waiting = tokio::spawn(async move { b.list_tabs().await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        bridge.detach(id);
        let result = waiting.await.unwrap();
        assert!(matches!(result, Err(BridgeError::NotConnected(_))));
    }

    #[tokio::test]
    async fn no_answer_times_out() {
        let bridge = ExtensionBridge::new();
        let (id, _rx) = bridge.attach();
        bridge.handle_incoming(
            id,
            ExtensionToHub::Hello {
                extension_version: "1".into(),
                protocol_version: PROTOCOL_VERSION,
            },
        );
        let result = bridge
            .request(
                |request_id| HubToExtension::ListTabs { request_id },
                Duration::from_millis(30),
            )
            .await;
        assert_eq!(result, Err(BridgeError::Timeout));
        assert!(bridge.inner.pending.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn newer_connection_wins() {
        let bridge = ExtensionBridge::new();
        let (old, _rx1) = bridge.attach();
        let (new, _rx2) = bridge.attach();
        bridge.detach(old); // must not drop the newer connection
        let reply = bridge.handle_incoming(
            new,
            ExtensionToHub::Hello {
                extension_version: "2".into(),
                protocol_version: PROTOCOL_VERSION,
            },
        );
        assert!(reply.is_some());
        assert!(matches!(bridge.status(), BridgeStatus::Connected { .. }));
    }
}
