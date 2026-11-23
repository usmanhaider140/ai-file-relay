//! Shared state for the MCP server and the HTTP server.

use std::path::{Path, PathBuf};
use std::time::Duration;

use relay_core::{
    Action, AllowedFolders, ApprovalRequest, Approver, Config, Decision, Event, History, Outcome,
    PolicyApprover, TokenStore,
};

use crate::browser::{BrowserBridge, NotConnected, UploadJob};
use crate::extension::ExtensionBridge;

/// How long a one-time file link works.
pub const FILE_LINK_TTL: Duration = Duration::from_secs(60);

/// Everything the tools and the HTTP server share.
pub struct Relay {
    pub config: Config,
    pub folders: AllowedFolders,
    /// Folders in the config that no longer exist.
    pub missing_folders: Vec<PathBuf>,
    pub tokens: TokenStore,
    pub approver: Box<dyn Approver>,
    pub history: History,
    pub browser: Box<dyn BrowserBridge>,
    /// The extension link, when this process is the hub. The HTTP server
    /// connects the native host's WebSocket to it.
    pub extension: Option<ExtensionBridge>,
    /// Base URL of the file server, for example `http://127.0.0.1:41234`.
    /// `None` when no file server is running.
    pub file_server: Option<String>,
}

impl Relay {
    /// Build the state from a config, with the default approver (from the
    /// `approval` setting) and no browser connection.
    pub fn from_config(config: Config) -> Self {
        let (folders, missing_folders) = config.allowed();
        let history = match &config.history_file {
            Some(file) => History::with_file(file),
            None => History::in_memory(),
        };
        let approver = Box::new(PolicyApprover {
            mode: config.approval,
        });
        Self {
            config,
            folders,
            missing_folders,
            tokens: TokenStore::new(),
            approver,
            history,
            browser: Box::new(NotConnected),
            extension: None,
            file_server: None,
        }
    }

    /// Use the real extension link as the browser.
    pub fn with_extension(mut self, bridge: ExtensionBridge) -> Self {
        self.browser = Box::new(bridge.clone());
        self.extension = Some(bridge);
        self
    }

    /// Use another browser bridge.
    pub fn with_browser(mut self, browser: impl BrowserBridge + 'static) -> Self {
        self.browser = Box::new(browser);
        self
    }

    /// Use another approver.
    pub fn with_approver(mut self, approver: impl Approver + 'static) -> Self {
        self.approver = Box::new(approver);
        self
    }

    /// Set the file server base URL.
    pub fn with_file_server(mut self, base_url: impl Into<String>) -> Self {
        self.file_server = Some(base_url.into());
        self
    }

    fn log(&self, action: Action, path: &Path, site: Option<String>, outcome: Outcome) {
        if let Err(e) = self.history.record(Event::now(action, path, site, outcome)) {
            tracing::warn!("could not write the transfer log: {e}");
        }
    }

    /// The site (host name) of a tab, if the browser can tell us.
    async fn tab_site(&self, tab_id: i64) -> Option<String> {
        let tabs = self.browser.list_tabs().await.ok()?;
        let tab = tabs.into_iter().find(|t| t.id == tab_id)?;
        site_of(&tab.url)
    }

    /// Run the whole upload flow: check the path, check the size, ask for
    /// approval, make a one-time link, hand the job to the browser, and log
    /// the result. Errors are written for the AI to read.
    pub async fn upload(
        &self,
        path: &str,
        tab_id: i64,
        selector: Option<String>,
    ) -> Result<UploadDone, String> {
        let real = self.folders.check(path).map_err(|e| e.to_string())?;
        let meta = std::fs::metadata(&real).map_err(|e| format!("cannot read {path}: {e}"))?;
        if !meta.is_file() {
            return Err(format!("not a file: {path}"));
        }
        let max = self.config.max_file_bytes();
        if meta.len() > max {
            return Err(format!(
                "file is {} bytes, over the {} MB limit (max_file_mb in the config)",
                meta.len(),
                self.config.max_file_mb
            ));
        }

        let site = self.tab_site(tab_id).await;
        let request = ApprovalRequest {
            action: Action::Upload,
            path: real.clone(),
            site: site.clone(),
        };
        if let Decision::Deny(why) = self.approver.decide(&request) {
            self.log(Action::Upload, &real, site, Outcome::Denied(why.clone()));
            return Err(format!("upload not approved: {why}"));
        }

        let Some(base) = &self.file_server else {
            let why = "the file server is not running (start the hub with `relay-mcp serve`)";
            self.log(Action::Upload, &real, site, Outcome::Failed(why.into()));
            return Err(why.into());
        };

        let file_name = real
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into());
        let token = self.tokens.issue(&real, FILE_LINK_TTL);
        let job = UploadJob {
            request_id: relay_core::random_hex(8),
            tab_id,
            file_url: format!("{base}/files/{token}"),
            token: token.clone(),
            mime_type: guess_mime(&file_name).into(),
            file_name: file_name.clone(),
            size: meta.len(),
            selector,
        };

        match self.browser.upload(job).await {
            Ok(()) => {
                self.log(Action::Upload, &real, site, Outcome::Done);
                Ok(UploadDone {
                    file_name,
                    size: meta.len(),
                    tab_id,
                })
            }
            Err(e) => {
                // The link was never used; make sure it cannot be used later.
                self.tokens.revoke(&token);
                self.log(Action::Upload, &real, site, Outcome::Failed(e.to_string()));
                Err(format!("upload failed: {e}"))
            }
        }
    }

    /// Wait for a download, ask for approval, copy it into the inbox folder
    /// and return the new path.
    pub async fn wait_for_download(
        &self,
        tab_id: Option<i64>,
        timeout: Duration,
    ) -> Result<PathBuf, String> {
        let inbox = self
            .config
            .inbox_folder
            .clone()
            .ok_or("no inbox_folder is set in the config")?;

        let site = match tab_id {
            Some(id) => self.tab_site(id).await,
            None => None,
        };
        let request = ApprovalRequest {
            action: Action::Download,
            path: inbox.clone(),
            site: site.clone(),
        };
        if let Decision::Deny(why) = self.approver.decide(&request) {
            self.log(Action::Download, &inbox, site, Outcome::Denied(why.clone()));
            return Err(format!("download not approved: {why}"));
        }

        let done = self
            .browser
            .wait_for_download(tab_id, timeout)
            .await
            .map_err(|e| {
                self.log(
                    Action::Download,
                    &inbox,
                    site.clone(),
                    Outcome::Failed(e.to_string()),
                );
                format!("no download: {e}")
            })?;
        let site = site.or_else(|| site_of(&done.url));

        let target = copy_into(&done.path, &inbox).map_err(|e| {
            let why = format!("could not copy the download into the inbox: {e}");
            self.log(
                Action::Download,
                &done.path,
                site.clone(),
                Outcome::Failed(why.clone()),
            );
            why
        })?;
        self.log(Action::Download, &target, site, Outcome::Done);
        Ok(target)
    }
}

/// Result of a finished upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadDone {
    pub file_name: String,
    pub size: u64,
    pub tab_id: i64,
}

/// Copy `file` into `folder`, adding " (1)", " (2)" ... if the name is taken.
fn copy_into(file: &Path, folder: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(folder)?;
    let name = file
        .file_name()
        .ok_or_else(|| std::io::Error::other("download has no file name"))?;
    let mut target = folder.join(name);
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = file
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let mut n = 1;
    while target.exists() {
        target = folder.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    std::fs::copy(file, &target)?;
    Ok(target)
}

/// Host name of a URL, for example `example.com` for
/// `https://example.com/upload`. `None` for URLs without a host.
pub fn site_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host_port = rest.split(['/', '?', '#']).next()?;
    let host = host_port.rsplit_once('@').map_or(host_port, |(_, h)| h);
    let host = if host.starts_with('[') {
        host.split_once(']').map(|(h, _)| &h[1..])?
    } else {
        host.split(':').next()?
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// Best guess of a file's type from its extension.
pub fn guess_mime(file_name: &str) -> &'static str {
    let ext = file_name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "txt" | "log" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "xml" => "application/xml",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::{BoxFuture, BridgeError, BridgeStatus, FinishedDownload, Tab};
    use relay_core::ApprovalMode;
    use std::sync::{Arc, Mutex};

    /// A pretend browser that records upload jobs and returns a fixed download.
    #[derive(Default, Clone)]
    struct FakeBrowser {
        jobs: Arc<Mutex<Vec<UploadJob>>>,
        download: Option<PathBuf>,
    }

    impl BrowserBridge for FakeBrowser {
        fn status(&self) -> BridgeStatus {
            BridgeStatus::Connected {
                extension_version: "test".into(),
            }
        }
        fn list_tabs(&self) -> BoxFuture<'_, Result<Vec<Tab>, BridgeError>> {
            Box::pin(async { Ok(vec![]) })
        }
        fn upload(&self, job: UploadJob) -> BoxFuture<'_, Result<(), BridgeError>> {
            self.jobs.lock().unwrap().push(job);
            Box::pin(async { Ok(()) })
        }
        fn wait_for_download(
            &self,
            _tab_id: Option<i64>,
            _timeout: Duration,
        ) -> BoxFuture<'_, Result<FinishedDownload, BridgeError>> {
            let path = self.download.clone();
            Box::pin(async move {
                path.map(|path| FinishedDownload {
                    path,
                    url: "https://example.com/x".into(),
                })
                .ok_or(BridgeError::Timeout)
            })
        }
    }

    fn setup(mode: ApprovalMode) -> (tempfile::TempDir, Config) {
        let dir = tempfile::tempdir().unwrap();
        let allowed = dir.path().join("allowed");
        std::fs::create_dir(&allowed).unwrap();
        std::fs::write(allowed.join("report.pdf"), b"%PDF-1.7 hello").unwrap();
        let mut config = Config {
            approval: mode,
            inbox_folder: Some(dir.path().join("inbox")),
            ..Config::default()
        };
        config.allow(&allowed).unwrap();
        (dir, config)
    }

    #[tokio::test]
    async fn upload_happy_path_issues_a_working_token() {
        let (dir, config) = setup(ApprovalMode::AllowAll);
        let browser = FakeBrowser::default();
        let relay = Relay::from_config(config)
            .with_browser(browser.clone())
            .with_file_server("http://127.0.0.1:1");
        let path = dir.path().join("allowed/report.pdf");
        let done = relay.upload(path.to_str().unwrap(), 7, None).await.unwrap();
        assert_eq!(done.file_name, "report.pdf");

        let job = browser.jobs.lock().unwrap().pop().unwrap();
        assert_eq!(job.tab_id, 7);
        assert_eq!(job.mime_type, "application/pdf");
        assert_eq!(
            job.file_url,
            format!("http://127.0.0.1:1/files/{}", job.token)
        );
        assert!(relay.tokens.redeem(&job.token).is_some());
        assert_eq!(relay.history.recent(1)[0].outcome, Outcome::Done);
    }

    #[tokio::test]
    async fn upload_outside_allowed_is_refused() {
        let (dir, config) = setup(ApprovalMode::AllowAll);
        std::fs::write(dir.path().join("secret.txt"), "no").unwrap();
        let relay = Relay::from_config(config).with_file_server("http://127.0.0.1:1");
        let err = relay
            .upload(dir.path().join("secret.txt").to_str().unwrap(), 1, None)
            .await
            .unwrap_err();
        assert!(err.contains("outside the allowed folders"), "{err}");
        assert_eq!(relay.tokens.pending(), 0);
    }

    #[tokio::test]
    async fn upload_needs_approval_by_default() {
        let (dir, config) = setup(ApprovalMode::Ask);
        let relay = Relay::from_config(config).with_file_server("http://127.0.0.1:1");
        let err = relay
            .upload(
                dir.path().join("allowed/report.pdf").to_str().unwrap(),
                1,
                None,
            )
            .await
            .unwrap_err();
        assert!(err.contains("not approved"), "{err}");
        assert_eq!(relay.tokens.pending(), 0);
        assert!(matches!(
            relay.history.recent(1)[0].outcome,
            Outcome::Denied(_)
        ));
    }

    #[tokio::test]
    async fn upload_over_size_limit_is_refused() {
        let (dir, mut config) = setup(ApprovalMode::AllowAll);
        config.max_file_mb = 0;
        let relay = Relay::from_config(config).with_file_server("http://127.0.0.1:1");
        let err = relay
            .upload(
                dir.path().join("allowed/report.pdf").to_str().unwrap(),
                1,
                None,
            )
            .await
            .unwrap_err();
        assert!(err.contains("MB limit"), "{err}");
    }

    #[tokio::test]
    async fn upload_without_extension_says_so() {
        let (dir, config) = setup(ApprovalMode::AllowAll);
        let relay = Relay::from_config(config).with_file_server("http://127.0.0.1:1");
        let err = relay
            .upload(
                dir.path().join("allowed/report.pdf").to_str().unwrap(),
                1,
                None,
            )
            .await
            .unwrap_err();
        assert!(err.contains("not connected"), "{err}");
        assert!(matches!(
            relay.history.recent(1)[0].outcome,
            Outcome::Failed(_)
        ));
    }

    #[tokio::test]
    async fn download_is_copied_into_inbox_without_overwriting() {
        let (dir, config) = setup(ApprovalMode::AllowAll);
        let chrome_file = dir.path().join("Downloads/invoice.pdf");
        std::fs::create_dir_all(chrome_file.parent().unwrap()).unwrap();
        std::fs::write(&chrome_file, "pdf").unwrap();
        let browser = FakeBrowser {
            download: Some(chrome_file),
            ..Default::default()
        };
        let relay = Relay::from_config(config).with_browser(browser);
        let first = relay
            .wait_for_download(None, Duration::from_secs(1))
            .await
            .unwrap();
        let second = relay
            .wait_for_download(None, Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(first, dir.path().join("inbox/invoice.pdf"));
        assert_eq!(second, dir.path().join("inbox/invoice (1).pdf"));
        assert_eq!(std::fs::read_to_string(second).unwrap(), "pdf");
    }

    #[test]
    fn site_of_urls() {
        assert_eq!(
            site_of("https://Example.com/upload?x=1").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            site_of("http://user:pw@host.dev:8080/a").as_deref(),
            Some("host.dev")
        );
        assert_eq!(site_of("http://[::1]:3000/").as_deref(), Some("::1"));
        assert_eq!(site_of("file:///tmp/a.html"), None);
        assert_eq!(site_of("chrome://newtab/"), Some("newtab".into()));
        assert_eq!(site_of("not a url"), None);
    }

    #[test]
    fn mime_guess() {
        assert_eq!(guess_mime("A.PNG"), "image/png");
        assert_eq!(guess_mime("noext"), "application/octet-stream");
    }

    #[test]
    fn relay_is_shareable() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Relay>();
        let _ = Arc::new(Relay::from_config(Config::default()));
    }
}
