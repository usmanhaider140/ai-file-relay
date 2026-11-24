//! The hub inside the desktop app.
//!
//! Runs the same HTTP server as `relay-mcp serve` (MCP endpoint, file
//! links, browser bridge), but with the approval queue, so each transfer
//! shows the popup. If the port is taken, it keeps retrying and says why.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use relay_core::Config;
use relay_mcp::approvals::DEFAULT_TIMEOUT;
use relay_mcp::proxy::probe_hub;
use relay_mcp::{ApprovalQueue, ExtensionBridge, Relay, http};
use serde::Serialize;
use tokio_util::sync::CancellationToken;

const RETRY_EVERY: Duration = Duration::from_secs(3);

/// What the hub is doing, for the app's status screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum HubState {
    Starting,
    Running { port: u16 },
    PortBusy { port: u16, detail: String },
}

/// Everything the app shares about the hub.
pub struct Hub {
    pub config_path: PathBuf,
    pub config: Mutex<Config>,
    pub relay: Arc<Relay>,
    pub queue: Arc<ApprovalQueue>,
    pub bridge: ExtensionBridge,
    pub state: Mutex<HubState>,
    pub shutdown: CancellationToken,
}

impl Hub {
    /// Load the settings and build the hub (not started yet).
    pub fn new(config_path: PathBuf) -> anyhow::Result<Arc<Self>> {
        let (config, _) = Config::load_or_create(&config_path)?;
        let queue = Arc::new(ApprovalQueue::new(config.approval, DEFAULT_TIMEOUT));
        let bridge = ExtensionBridge::new();
        let relay = Arc::new(
            Relay::from_config(config.clone())
                .with_approver(queue.clone())
                .with_extension(bridge.clone())
                .with_file_server(format!("http://127.0.0.1:{}", config.port))
                .with_hub_role("desktop"),
        );
        Ok(Arc::new(Self {
            config_path,
            config: Mutex::new(config),
            relay,
            queue,
            bridge,
            state: Mutex::new(HubState::Starting),
            shutdown: CancellationToken::new(),
        }))
    }

    pub fn state(&self) -> HubState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set_state(&self, state: HubState) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = state;
    }

    /// A copy of the current settings.
    pub fn config(&self) -> Config {
        self.config
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Change the settings, save them, and apply what can change live.
    pub fn update_config(&self, change: impl FnOnce(&mut Config)) -> Result<Config, String> {
        let mut config = self.config.lock().unwrap_or_else(|e| e.into_inner());
        let mut next = config.clone();
        change(&mut next);
        next.save(&self.config_path).map_err(|e| e.to_string())?;
        self.relay.set_allowed_folders(&next.allowed_folders);
        self.queue.set_mode(next.approval);
        *config = next.clone();
        Ok(next)
    }

    /// Run the hub until the app quits. Retries while the port is busy.
    pub async fn run(self: Arc<Self>) {
        let port = self.config().port;
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        loop {
            match tokio::net::TcpListener::bind(addr).await {
                Ok(listener) => {
                    tracing::info!("hub running on {addr}");
                    self.set_state(HubState::Running { port });
                    let app = http::router(self.relay.clone(), self.shutdown.clone());
                    let stop = self.shutdown.clone();
                    let served = axum::serve(listener, app)
                        .with_graceful_shutdown(async move { stop.cancelled_owned().await })
                        .await;
                    if let Err(e) = served {
                        tracing::warn!("hub stopped: {e}");
                    }
                    if self.shutdown.is_cancelled() {
                        return;
                    }
                }
                Err(e) => {
                    self.set_state(HubState::PortBusy {
                        port,
                        detail: format!("port {port} is in use ({e})"),
                    });
                    // Find out who has it, to give a useful hint.
                    let detail = match probe_hub(port).await {
                        Some(other) => format!(
                            "another AI File Relay hub (started by {}) is using port {port}. \
                             Quit that AI app or `relay-mcp serve`, and this app takes over.",
                            match other.role.as_str() {
                                "stdio" => "an AI app",
                                "serve" => "`relay-mcp serve`",
                                _ => "another copy of this app",
                            }
                        ),
                        None => format!("port {port} is in use by another program ({e})"),
                    };
                    self.set_state(HubState::PortBusy { port, detail });
                }
            }
            tokio::select! {
                _ = self.shutdown.cancelled() => return,
                _ = tokio::time::sleep(RETRY_EVERY) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relay_core::ApprovalMode;

    #[tokio::test]
    async fn settings_changes_apply_live_and_are_saved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let hub = Hub::new(path.clone()).unwrap();
        assert_eq!(hub.queue.mode(), ApprovalMode::Ask);

        let docs = dir.path().join("docs");
        std::fs::create_dir(&docs).unwrap();
        hub.update_config(|c| {
            c.allow(&docs).unwrap();
            c.approval = ApprovalMode::DenyAll;
        })
        .unwrap();

        assert_eq!(hub.queue.mode(), ApprovalMode::DenyAll);
        assert_eq!(hub.relay.allowed_roots().len(), 1);
        let saved = Config::load(&path).unwrap();
        assert_eq!(saved.approval, ApprovalMode::DenyAll);
        assert_eq!(saved.allowed_folders.len(), 1);
    }

    #[tokio::test]
    async fn runs_and_reports_a_busy_port() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        // Take a port first, then point the hub at it.
        let blocker = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = blocker.local_addr().unwrap().port();
        let mut config = Config::generate();
        config.port = port;
        config.save(&path).unwrap();

        let hub = Hub::new(path).unwrap();
        let running = tokio::spawn(hub.clone().run());
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(matches!(hub.state(), HubState::PortBusy { .. }));

        // Free the port: the hub takes it on its next try.
        drop(blocker);
        tokio::time::sleep(RETRY_EVERY + Duration::from_millis(500)).await;
        assert_eq!(hub.state(), HubState::Running { port });

        hub.shutdown.cancel();
        running.await.unwrap();
    }
}
