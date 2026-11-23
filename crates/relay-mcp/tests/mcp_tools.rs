//! Talks to the MCP server the way an AI app does, over an in-memory pipe.

use std::sync::Arc;

use relay_core::{ApprovalMode, Config};
use relay_mcp::{Relay, RelayServer};
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult, ClientConfig};
use rmcp::service::{RoleClient, RunningService};
use serde_json::{Value, json};

struct Setup {
    _dir: tempfile::TempDir,
    allowed: std::path::PathBuf,
    client: RunningService<RoleClient, ClientConfig>,
}

async fn start(approval: ApprovalMode) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let allowed = dir.path().join("docs");
    std::fs::create_dir(&allowed).unwrap();
    std::fs::write(allowed.join("notes.txt"), "hello").unwrap();
    std::fs::create_dir(allowed.join("photos")).unwrap();

    let mut config = Config {
        approval,
        ..Config::default()
    };
    let allowed = config.allow(&allowed).unwrap();
    let relay = Arc::new(Relay::from_config(config).with_file_server("http://127.0.0.1:1"));

    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let server = RelayServer::new(relay).serve(server_io).await.unwrap();
        let _ = server.waiting().await;
    });
    let client = ClientConfig::default().serve(client_io).await.unwrap();
    Setup {
        _dir: dir,
        allowed,
        client,
    }
}

async fn call(s: &Setup, name: &'static str, args: Value) -> CallToolResult {
    let args = args.as_object().cloned().unwrap_or_default();
    s.client
        .call_tool(CallToolRequestParams::new(name).with_arguments(args))
        .await
        .unwrap()
}

fn text(r: &CallToolResult) -> String {
    serde_json::to_string(&r.content).unwrap()
}

#[tokio::test]
async fn server_lists_the_five_tools() {
    let s = start(ApprovalMode::Ask).await;
    let mut names: Vec<String> = s
        .client
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "list_files",
            "list_tabs",
            "relay_status",
            "upload_to_page",
            "wait_for_download"
        ]
    );
    let info = s.client.peer_info().unwrap();
    assert_eq!(info.server_info.as_ref().unwrap().name, "ai-file-relay");
    assert!(
        info.instructions
            .as_deref()
            .unwrap_or("")
            .contains("list_files")
    );
}

#[tokio::test]
async fn list_files_shows_allowed_folders_then_contents() {
    let s = start(ApprovalMode::Ask).await;

    let roots = call(&s, "list_files", json!({})).await;
    assert_ne!(roots.is_error, Some(true), "{}", text(&roots));
    let roots = roots.structured_content.unwrap();
    assert_eq!(
        roots["items"][0]["path"],
        json!(s.allowed.display().to_string())
    );

    let inside = call(
        &s,
        "list_files",
        json!({ "folder": s.allowed.display().to_string() }),
    )
    .await;
    let inside = inside.structured_content.unwrap();
    let names: Vec<&str> = inside["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["photos", "notes.txt"]);
    assert_eq!(inside["items"][1]["size"], json!(5));
}

#[tokio::test]
async fn list_files_outside_is_an_error_the_ai_can_read() {
    let s = start(ApprovalMode::Ask).await;
    let parent = s.allowed.parent().unwrap().display().to_string();
    let r = call(&s, "list_files", json!({ "folder": parent })).await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("outside the allowed folders"));
}

#[tokio::test]
async fn upload_is_refused_until_approved() {
    let s = start(ApprovalMode::Ask).await;
    let file = s.allowed.join("notes.txt").display().to_string();
    let r = call(&s, "upload_to_page", json!({ "path": file, "tab_id": 1 })).await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("not approved"));
}

#[tokio::test]
async fn approved_upload_reaches_the_browser_step() {
    let s = start(ApprovalMode::AllowAll).await;
    let file = s.allowed.join("notes.txt").display().to_string();
    let r = call(&s, "upload_to_page", json!({ "path": file, "tab_id": 1 })).await;
    // No extension yet (M2), so it stops at the browser with a clear message.
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).contains("Chrome extension"), "{}", text(&r));
}

#[tokio::test]
async fn bad_arguments_are_rejected() {
    let s = start(ApprovalMode::AllowAll).await;
    let r = s
        .client
        .call_tool(
            CallToolRequestParams::new("upload_to_page")
                .with_arguments(json!({ "tab_id": 1 }).as_object().cloned().unwrap()),
        )
        .await;
    // Missing `path`: either a protocol error or an error result.
    match r {
        Err(_) => {}
        Ok(r) => assert_eq!(r.is_error, Some(true)),
    }
}

#[tokio::test]
async fn status_reports_setup() {
    let s = start(ApprovalMode::Ask).await;
    let r = call(&s, "relay_status", json!({})).await;
    let st = r.structured_content.unwrap();
    assert_eq!(st["approval"], json!("ask"));
    assert_eq!(st["browser_connected"], json!(false));
    assert_eq!(
        st["allowed_folders"],
        json!([s.allowed.display().to_string()])
    );
    assert_eq!(st["file_server"], json!("http://127.0.0.1:1"));
}
