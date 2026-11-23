//! The whole chain except Chrome itself:
//!
//! test (pretends to be the extension) ⇄ relay-mcp started as a native host
//! ⇄ WebSocket /bridge ⇄ hub ⇄ MCP upload / download flow.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use relay_core::{ApprovalMode, Config};
use relay_mcp::{ExtensionBridge, Relay, http};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio_util::sync::CancellationToken;

async fn send(stdin: &mut ChildStdin, msg: Value) {
    let body = serde_json::to_vec(&msg).unwrap();
    stdin
        .write_all(&(body.len() as u32).to_ne_bytes())
        .await
        .unwrap();
    stdin.write_all(&body).await.unwrap();
    stdin.flush().await.unwrap();
}

async fn recv(stdout: &mut ChildStdout) -> Value {
    let read = async {
        let mut len = [0u8; 4];
        stdout.read_exact(&mut len).await.unwrap();
        let mut buf = vec![0u8; u32::from_ne_bytes(len) as usize];
        stdout.read_exact(&mut buf).await.unwrap();
        serde_json::from_slice(&buf).unwrap()
    };
    tokio::time::timeout(Duration::from_secs(10), read)
        .await
        .expect("no message from the host within 10 s")
}

/// Read messages until one has the given type.
async fn recv_type(stdout: &mut ChildStdout, ty: &str) -> Value {
    loop {
        let msg = recv(stdout).await;
        if msg["type"] == ty {
            return msg;
        }
    }
}

/// Minimal HTTP GET, so the test needs no HTTP client.
async fn http_get(url: &str) -> (u16, Vec<u8>) {
    let rest = url.strip_prefix("http://").unwrap();
    let (host, path) = rest.split_once('/').unwrap();
    let mut s = tokio::net::TcpStream::connect(host).await.unwrap();
    s.write_all(
        format!("GET /{path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes(),
    )
    .await
    .unwrap();
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await.unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let status = head[9..12].parse().unwrap();
    (status, raw[split + 4..].to_vec())
}

struct Chain {
    _dir: tempfile::TempDir,
    relay: Arc<Relay>,
    allowed: std::path::PathBuf,
    host: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    shutdown: CancellationToken,
}

async fn start_chain() -> Chain {
    let dir = tempfile::tempdir().unwrap();
    let allowed = dir.path().join("docs");
    std::fs::create_dir(&allowed).unwrap();
    std::fs::write(allowed.join("cv.pdf"), b"%PDF-1.7 my cv").unwrap();

    // Hub on a free port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut config = Config {
        port,
        token: "chain-test-token".into(),
        approval: ApprovalMode::AllowAll,
        inbox_folder: Some(dir.path().join("inbox")),
        ..Config::default()
    };
    let allowed = config.allow(&allowed).unwrap();
    let config_path = dir.path().join("config.toml");
    config.save(&config_path).unwrap();

    let relay = Arc::new(
        Relay::from_config(config)
            .with_file_server(format!("http://127.0.0.1:{port}"))
            .with_extension(ExtensionBridge::new()),
    );
    let shutdown = CancellationToken::new();
    let app = http::router(relay.clone(), shutdown.clone());
    let stop = shutdown.clone();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move { stop.cancelled_owned().await })
            .await
            .unwrap();
    });

    // The real binary, started the way Chrome starts it.
    let mut host = Command::new(env!("CARGO_BIN_EXE_relay-mcp"))
        .arg("chrome-extension://test-extension/")
        .env("AI_FILE_RELAY_CONFIG", &config_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stdin = host.stdin.take().unwrap();
    let stdout = host.stdout.take().unwrap();
    Chain {
        _dir: dir,
        relay,
        allowed,
        host,
        stdin,
        stdout,
        shutdown,
    }
}

async fn say_hello(c: &mut Chain) {
    send(
        &mut c.stdin,
        json!({ "type": "hello", "extension_version": "0.2.0-test", "protocol_version": relay_protocol::PROTOCOL_VERSION }),
    )
    .await;
    let welcome = recv_type(&mut c.stdout, "welcome").await;
    assert_eq!(
        welcome["protocol_version"],
        relay_protocol::PROTOCOL_VERSION
    );
}

#[tokio::test]
async fn upload_travels_the_whole_chain() {
    let mut c = start_chain().await;
    let status = recv_type(&mut c.stdout, "hub_status").await;
    assert_eq!(status["connected"], true);
    say_hello(&mut c).await;

    // The AI asks to upload; the "extension" answers like the real one.
    let relay = c.relay.clone();
    let file = c.allowed.join("cv.pdf").display().to_string();
    let upload = tokio::spawn(async move { relay.upload(&file, 42, None).await });

    // First the hub asks for the tab list (to learn the site name).
    let list = recv_type(&mut c.stdout, "list_tabs").await;
    send(
        &mut c.stdin,
        json!({ "type": "tabs", "request_id": list["request_id"], "tabs": [
            { "id": 42, "title": "Jobs", "url": "https://jobs.example.com/apply" }
        ]}),
    )
    .await;

    let job = recv_type(&mut c.stdout, "upload_to_page").await;
    assert_eq!(job["tab_id"], 42);
    assert_eq!(job["file_name"], "cv.pdf");
    assert_eq!(job["mime_type"], "application/pdf");

    // The extension fetches the bytes once...
    let url = job["file_url"].as_str().unwrap();
    let (status, body) = http_get(url).await;
    assert_eq!(status, 200);
    assert_eq!(body, b"%PDF-1.7 my cv");
    // ...and the link does not work a second time.
    assert_eq!(http_get(url).await.0, 404);

    send(
        &mut c.stdin,
        json!({ "type": "upload_result", "request_id": job["request_id"], "ok": true }),
    )
    .await;
    let done = upload.await.unwrap().expect("upload should succeed");
    assert_eq!(done.file_name, "cv.pdf");

    let last = &c.relay.history.recent(1)[0];
    assert_eq!(last.site.as_deref(), Some("jobs.example.com"));
    c.shutdown.cancel();
    let _ = c.host.kill().await;
}

#[tokio::test]
async fn download_travels_the_whole_chain() {
    let mut c = start_chain().await;
    recv_type(&mut c.stdout, "hub_status").await;
    say_hello(&mut c).await;

    let chrome_file = c._dir.path().join("Downloads/report.csv");
    std::fs::create_dir_all(chrome_file.parent().unwrap()).unwrap();
    std::fs::write(&chrome_file, "a,b\n1,2\n").unwrap();

    let relay = c.relay.clone();
    let wait =
        tokio::spawn(async move { relay.wait_for_download(None, Duration::from_secs(5)).await });
    let watch = recv_type(&mut c.stdout, "watch_download").await;
    assert_eq!(watch["timeout_ms"], 5000);
    send(
        &mut c.stdin,
        json!({ "type": "download_finished", "request_id": watch["request_id"],
                "url": "https://data.example.com/report.csv",
                "path": chrome_file.display().to_string() }),
    )
    .await;
    let path = wait.await.unwrap().expect("download should succeed");
    assert_eq!(path, c._dir.path().join("inbox").join("report.csv"));
    assert_eq!(std::fs::read_to_string(path).unwrap(), "a,b\n1,2\n");
    assert_eq!(
        c.relay.history.recent(1)[0].site.as_deref(),
        Some("data.example.com")
    );
    c.shutdown.cancel();
    let _ = c.host.kill().await;
}

#[tokio::test]
async fn host_reports_when_the_hub_goes_away() {
    let mut c = start_chain().await;
    assert_eq!(
        recv_type(&mut c.stdout, "hub_status").await["connected"],
        true
    );
    say_hello(&mut c).await;

    c.shutdown.cancel();
    let status = recv_type(&mut c.stdout, "hub_status").await;
    assert_eq!(status["connected"], false);

    // While the hub is away, the host still answers pings itself.
    send(&mut c.stdin, json!({ "type": "ping" })).await;
    recv_type(&mut c.stdout, "pong").await;
    let _ = c.host.kill().await;
}
