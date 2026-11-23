# Architecture

The desktop hub is the centre. AI apps reach it through MCP, and the browser reaches it through a tiny native messaging host that Chrome starts.

```
AI apps (Claude, Cursor, VS Code)
        │  MCP (HTTP on localhost, or stdio)
        ▼
┌──────────── Desktop hub ────────────┐
│  MCP endpoint                       │
│  Relay core  ◀── WebSocket /bridge ─┼──▶ relay-mcp as native host (started by Chrome)
│  Approvals and tray                 │          │ native messaging
│  Allowed folders ── file bytes ─────┼──┐       ▼
└─────────────────────────────────────┘  └──▶ Chrome extension ──▶ web page
                                      (localhost HTTP + one-time token)
```

## How Chrome reaches the hub

Chrome starts `relay-mcp` with a `chrome-extension://…` argument; that makes it run as the native messaging host. The host reads the port and token from the settings file and connects to the hub's `/bridge` WebSocket, then passes messages both ways unchanged. If the hub is not running, the host keeps trying every 2 seconds and sends the extension a `hub_status` message, so the toolbar badge can show it.

`relay-mcp install-native-host` writes the host manifest for Chrome, Chromium, Edge and Brave (on Windows it also adds the registry keys). The manifest only allows the extension's pinned ID.

## Why file bytes skip native messaging

Chrome limits a message from the native host to the extension to 1 MB. So native messaging only carries small control messages. The extension downloads the file bytes from the hub on `127.0.0.1` with a one-time token.

## Upload flow

1. The AI calls `upload_to_page` with a file path and a tab.
2. The hub checks the path is inside an allowed folder, then asks the user.
3. The user clicks Allow. The hub makes a one-time token.
4. The hub sends the tab id and token to the extension through the messaging host.
5. The extension's background worker fetches the bytes from the hub, injects a small content script into the tab, and sends it the file in 1 MB pieces. The content script builds a `File`, sets it on the page's file input and fires `input` and `change` (or, for a drag-and-drop area, fires `dragenter`, `dragover` and `drop`).
6. The extension reports back; the hub logs the transfer and the tool returns.

## Download flow

1. The AI calls `wait_for_download`.
2. The extension watches `chrome.downloads` and reports the first download that finishes after the request (from the same site, if a tab was given).
3. The hub copies the file into its inbox folder and returns that path.

## Crates

| Crate | Job |
| --- | --- |
| `relay-protocol` | Message types shared by every part |
| `relay-core` | Allowed folders, path checks, one-time file links, approvals, history, settings file |
| `relay-mcp` | MCP tools (built with `rmcp`), the localhost HTTP server and the `relay-mcp` command |
| `relay-nmh` | Chrome native messaging framing (length-prefixed JSON) |

## Message format

Every message is JSON with a `type` field, for example `{"type":"ping"}`. See `crates/relay-protocol/src/lib.rs`. `cargo test` writes the same types as TypeScript to `apps/extension/src/generated/protocol.ts`.

## HTTP endpoints (`relay-mcp serve`)

| Path | Auth | What |
| --- | --- | --- |
| `/mcp` | `Authorization: Bearer <token>` | MCP over Streamable HTTP |
| `/bridge` | `Authorization: Bearer <token>` | WebSocket for the native host (link to the extension) |
| `/files/<token>` | the one-time token itself | Bytes of one approved file; works once, expires after 60 seconds |
| `/health` | none | Returns `ok` |

The server listens on 127.0.0.1 only, and requests whose `Host` header is not localhost are refused (protection against DNS rebinding). When an upload fails, its file link is cancelled at once.
