# AI File Relay

**Let any AI app put files into web pages and get downloads back, safely, without spinning up a temporary server.**

> **Status: early work (M0).** The project setup is done. Nothing is ready for daily use yet. Watch or star the repo to follow along.

## The problem

AI agents are good at using the browser, but moving files is hard. To upload a file to a website, or hand a downloaded file back to the AI, agents often start a throwaway web server or push file bytes through the chat. That is slow, fragile and not very safe.

## The idea

AI File Relay is a small free tool with three parts that work as one:

| Part | What it does | Written in |
| --- | --- | --- |
| **Desktop app (the hub)** | Runs in the menu bar. Keeps the list of allowed folders, asks you to approve each transfer, and keeps a history. | Rust (Tauri 2) |
| **MCP server** | Gives any MCP client (Claude, Cursor, VS Code and others) simple tools like `upload_to_page` and `wait_for_download`. | Rust (`rmcp`) |
| **Chrome extension** | Puts files into upload boxes on web pages and catches finished downloads. | TypeScript |

```
AI app ──MCP──▶ Desktop hub ◀──native messaging──▶ Chrome extension ──▶ web page
                    │                                     ▲
                    └──────── file bytes (localhost) ─────┘
```

## Safe by default

- The hub listens on `127.0.0.1` only.
- The AI can only reach folders you allow. `..` and symlinks cannot escape them, and folders like `~/.ssh` are always blocked.
- You approve every upload.
- File links work once and expire quickly.
- No cloud, no account, no telemetry.

## Roadmap

| Milestone | What you get |
| --- | --- |
| **M0 · Project setup** ✅ | Workspace, CI, licenses, first safety rules |
| **M1 · Hub + MCP** | MCP tools working in Claude Desktop |
| **M2 · Extension** | Uploads and downloads working end to end |
| **M3 · Desktop app** | Tray app, approval popup, installers |
| **M4 · Release v0.1** | Signed builds, Chrome Web Store, MCP registry |

## Project layout

```
crates/
  relay-protocol/   shared message types
  relay-core/       allowed folders, path checks (later: approvals, tokens, history)
  relay-mcp/        MCP server (M1)
  relay-nmh/        native messaging host that Chrome starts
apps/
  desktop/          Tauri 2 desktop app (M3)
  extension/        Chrome MV3 extension
docs/               architecture and protocol notes
```

## Build from source

You need Rust 1.85 or newer, and Node.js 20 or newer for the extension.

```sh
# Rust workspace
cargo test --workspace

# Chrome extension
cd apps/extension
npm install
npm run build      # output in apps/extension/dist
```

## Contributing

Help is welcome, especially from people who know Rust, Tauri or Chrome extensions. Read [CONTRIBUTING.md](CONTRIBUTING.md) to get started, and look for issues labelled **good first issue**.

Found a security problem? Please don't open a public issue. See [SECURITY.md](SECURITY.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution you submit for inclusion in this project, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
