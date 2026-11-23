//! The native messaging host: the program Chrome starts for the extension.
//!
//! Chrome starts `relay-mcp` with the extension's origin as an argument and
//! talks to it over stdin/stdout. This host connects to the hub's `/bridge`
//! WebSocket (port and token come from the config file) and passes messages
//! both ways. When the hub is not running it keeps trying, tells the
//! extension with a `hub_status` message, and answers `ping` itself.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use relay_core::Config;
use relay_nmh::{read_message, write_message};
use relay_protocol::HubToExtension;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};

/// Wait between attempts to reach the hub.
const RETRY_EVERY: Duration = Duration::from_secs(2);

/// True if Chrome started this program as a native messaging host.
/// Chrome passes the caller's origin, for example
/// `chrome-extension://<id>/` (and on Windows also `--parent-window=…`).
pub fn started_by_chrome(args: &[String]) -> bool {
    args.iter().any(|a| a.starts_with("chrome-extension://"))
}

/// Run the host until Chrome closes stdin.
pub fn run(config_path: PathBuf) -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("relay-mcp host: cannot start runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    // stdin: one blocking thread reads framed messages from Chrome.
    let (from_ext_tx, from_ext) = mpsc::unbounded_channel::<Value>();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        loop {
            match read_message::<Value>(&mut stdin) {
                Ok(Some(msg)) => {
                    if from_ext_tx.send(msg).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    eprintln!("relay-mcp host: bad message from Chrome: {e}");
                    break;
                }
            }
        }
    });

    // stdout: one blocking thread writes framed messages to Chrome.
    let (to_ext, mut to_ext_rx) = mpsc::unbounded_channel::<Value>();
    let writer = std::thread::spawn(move || {
        let mut stdout = std::io::stdout().lock();
        while let Some(msg) = to_ext_rx.blocking_recv() {
            if let Err(e) = write_message(&mut stdout, &msg) {
                eprintln!("relay-mcp host: cannot write to Chrome: {e}");
                break;
            }
        }
    });

    runtime.block_on(pump(config_path, from_ext, to_ext));
    drop(runtime);
    let _ = writer.join();
    ExitCode::SUCCESS
}

fn status(connected: bool, detail: Option<String>) -> Value {
    serde_json::to_value(HubToExtension::HubStatus { connected, detail }).unwrap_or(Value::Null)
}

fn is_type(msg: &Value, ty: &str) -> bool {
    msg.get("type").and_then(Value::as_str) == Some(ty)
}

/// Main loop: keep a hub connection and pass messages through.
async fn pump(
    config_path: PathBuf,
    mut from_ext: mpsc::UnboundedReceiver<Value>,
    to_ext: mpsc::UnboundedSender<Value>,
) {
    // The last `hello` from the extension. It is sent again to every new
    // hub connection, so the hub knows the extension is there.
    let mut hello: Option<Value> = None;
    let mut last_detail = String::new();

    loop {
        // ----- not connected: try to reach the hub -----
        let socket = match connect(&config_path).await {
            Ok(s) => s,
            Err(detail) => {
                if detail != last_detail {
                    let _ = to_ext.send(status(false, Some(detail.clone())));
                    last_detail = detail;
                }
                // Keep serving the extension while we wait.
                let wait = tokio::time::sleep(RETRY_EVERY);
                tokio::pin!(wait);
                loop {
                    tokio::select! {
                        _ = &mut wait => break,
                        msg = from_ext.recv() => {
                            let Some(msg) = msg else { return };
                            if is_type(&msg, "hello") {
                                hello = Some(msg);
                                let _ = to_ext.send(status(false, Some(last_detail.clone())));
                            } else if is_type(&msg, "ping") {
                                let _ = to_ext.send(serde_json::json!({ "type": "pong" }));
                            }
                        }
                    }
                }
                continue;
            }
        };

        // ----- connected -----
        last_detail.clear();
        let _ = to_ext.send(status(true, None));
        let (mut sink, mut stream) = socket.split();
        if let Some(h) = &hello
            && sink
                .send(tungstenite::Message::Text(h.to_string().into()))
                .await
                .is_err()
        {
            continue;
        }
        loop {
            tokio::select! {
                msg = from_ext.recv() => {
                    let Some(msg) = msg else {
                        let _ = sink.close().await;
                        return;
                    };
                    if is_type(&msg, "hello") {
                        hello = Some(msg.clone());
                    }
                    if sink.send(tungstenite::Message::Text(msg.to_string().into())).await.is_err() {
                        break;
                    }
                }
                incoming = stream.next() => {
                    match incoming {
                        Some(Ok(tungstenite::Message::Text(text))) => {
                            match serde_json::from_str::<Value>(&text) {
                                Ok(v) => { let _ = to_ext.send(v); }
                                Err(e) => eprintln!("relay-mcp host: bad message from hub: {e}"),
                            }
                        }
                        Some(Ok(tungstenite::Message::Close(_))) | None | Some(Err(_)) => break,
                        Some(Ok(_)) => {}
                    }
                }
            }
        }
        let _ = to_ext.send(status(
            false,
            Some("the hub went away; reconnecting".into()),
        ));
        last_detail = "the hub went away; reconnecting".into();
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Read the config and open the WebSocket to the hub.
async fn connect(config_path: &std::path::Path) -> Result<Socket, String> {
    let config = Config::load(config_path).map_err(|e| {
        format!("no settings file yet ({e}). Run `relay-mcp init` and `relay-mcp allow <folder>`")
    })?;
    if config.port == 0 || config.token.is_empty() {
        return Err("the settings file has no port or token yet. Run `relay-mcp init`".into());
    }
    let url = format!("ws://127.0.0.1:{}/bridge", config.port);
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|e| format!("bad hub address {url}: {e}"))?;
    let auth = format!("Bearer {}", config.token)
        .parse()
        .map_err(|_| "the token in the settings file is not valid".to_string())?;
    request.headers_mut().insert("authorization", auth);

    match tokio::time::timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(request),
    )
    .await
    {
        Ok(Ok((socket, _))) => Ok(socket),
        Ok(Err(tungstenite::Error::Http(res))) if res.status() == 401 => {
            Err("the hub refused the token; is the hub using a different settings file?".into())
        }
        Ok(Err(_)) | Err(_) => Err(format!(
            "the hub is not running on port {}. Start Claude Desktop (or your AI app) \
             with AI File Relay, or run `relay-mcp serve`",
            config.port
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_chrome_start() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(started_by_chrome(&args(&[
            "relay-mcp",
            "chrome-extension://abc/"
        ])));
        assert!(started_by_chrome(&args(&[
            "relay-mcp",
            "chrome-extension://abc/",
            "--parent-window=0"
        ])));
        assert!(!started_by_chrome(&args(&["relay-mcp", "serve"])));
    }
}
