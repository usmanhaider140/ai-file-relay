# Desktop app (the hub)

A Tauri 2 app that:

- runs the hub (MCP endpoint, one-time file links, browser bridge) on 127.0.0.1
- shows a popup for each transfer while approvals are set to "ask": **Allow once**, **Allow this site for 1 hour**, or **Deny** (no answer in 60 seconds means no)
- lets you choose allowed folders, see the activity log, and change settings
- sets up Chrome (native messaging host) and connects Claude Desktop and Cursor with one click
- lives in the menu bar / system tray, with **Pause all transfers** and **Start when I log in**

It ships `relay-mcp` next to itself (a Tauri "sidecar"). Chrome starts that `relay-mcp` as its native messaging host, and AI apps start it over stdio; both find the running app and forward to it.

## Run and build

```sh
npm install
npm run tauri dev      # development
npm run tauri build    # installers in ../../target/release/bundle/
```

`npm run sidecar` builds `relay-mcp` and copies it to `src-tauri/binaries/` with the target triple in its name; `tauri dev` and `tauri build` run it for you.

## Code

| Path | What |
| --- | --- |
| `src-tauri/src/hub.rs` | starts the hub, retries while the port is busy, applies settings live |
| `src-tauri/src/commands.rs` | commands the windows call |
| `src-tauri/src/connect.rs` | one-click connect for Claude Desktop and Cursor (keeps a backup of their settings) |
| `src-tauri/src/lib.rs` | windows, tray menu, popup that follows the approval queue |
| `src/App.tsx` | main window |
| `src/Approval.tsx` | approval popup |
