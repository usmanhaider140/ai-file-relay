//! A `relay-mcp` started by an AI app finds a running hub and forwards to
//! it, so every AI app shares one browser link and one approval popup.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use relay_core::Config;
use relay_mcp::{Relay, http};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn stdio_relay_forwards_to_the_running_hub() {
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut config = Config {
        port,
        token: "one-hub-token".into(),
        ..Config::default()
    };
    let docs = dir.path().join("docs");
    std::fs::create_dir(&docs).unwrap();
    let docs = config.allow(&docs).unwrap();
    let config_path = dir.path().join("config.toml");
    config.save(&config_path).unwrap();

    // The hub, as the desktop app would run it.
    let relay = Arc::new(Relay::from_config(config).with_hub_role("desktop"));
    let shutdown = CancellationToken::new();
    let app = http::router(relay, shutdown.clone());
    let stop = shutdown.clone();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move { stop.cancelled_owned().await })
            .await
            .unwrap();
    });

    // An AI app starts relay-mcp over stdio.
    let mut child = Command::new(env!("CARGO_BIN_EXE_relay-mcp"))
        .env("AI_FILE_RELAY_CONFIG", &config_path)
        .env("HTTP_PROXY", "http://192.0.2.1:9") // must be ignored for 127.0.0.1
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();

    let mut next_id = 0;
    let mut rpc = async |method: &str, params: Value| -> Value {
        next_id += 1;
        let msg = json!({ "jsonrpc": "2.0", "id": next_id, "method": method, "params": params });
        stdin
            .write_all(format!("{msg}\n").as_bytes())
            .await
            .unwrap();
        loop {
            let line = tokio::time::timeout(Duration::from_secs(10), lines.next_line())
                .await
                .expect("no answer within 10 s")
                .unwrap()
                .unwrap();
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == next_id {
                return v;
            }
        }
    };

    let init = rpc(
        "initialize",
        json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } }),
    )
    .await;
    assert_eq!(init["result"]["serverInfo"]["name"], "ai-file-relay");

    let tools = rpc("tools/list", json!({})).await;
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 5);

    // relay_status answered by the hub proves the call was forwarded.
    let status = rpc(
        "tools/call",
        json!({ "name": "relay_status", "arguments": {} }),
    )
    .await;
    let st = &status["result"]["structuredContent"];
    assert_eq!(st["hub"], "desktop");
    assert_eq!(st["allowed_folders"], json!([docs.display().to_string()]));

    // Errors from the hub reach the AI app as tool errors.
    let bad = rpc(
        "tools/call",
        json!({ "name": "list_files", "arguments": { "folder": "/" } }),
    )
    .await;
    assert_eq!(bad["result"]["isError"], true);

    shutdown.cancel();
    let _ = child.kill().await;
}
