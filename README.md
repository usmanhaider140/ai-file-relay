# AI File Relay

**Let any AI app put files into web pages and get downloads back, safely, without spinning up a temporary server.**

> **Status: early work (M1).** The MCP server works and you can try it with Claude Desktop, but the Chrome extension link (M2) is not built yet, so uploads and downloads stop at the browser step. Watch or star the repo to follow along.

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
| **M1 · Hub + MCP** ✅ | MCP tools working in Claude Desktop |
| **M2 · Extension** | Uploads and downloads working end to end |
| **M3 · Desktop app** | Tray app, approval popup, installers |
| **M4 · Release v0.1** | Signed builds, Chrome Web Store, MCP registry |

## Project layout

```
crates/
  relay-protocol/   shared message types
  relay-core/       allowed folders, path checks, one-time links, approvals, history, config
  relay-mcp/        MCP server and local hub (the `relay-mcp` command)
  relay-nmh/        native messaging host that Chrome starts
apps/
  desktop/          Tauri 2 desktop app (M3)
  extension/        Chrome MV3 extension
docs/               architecture and protocol notes
```

## Try it (M1)

You need Rust 1.88 or newer.

```sh
# 1. Install the relay-mcp command
cargo install --git https://github.com/usmanhaider140/ai-file-relay relay-mcp

# 2. Create the settings file and allow a folder
relay-mcp init
relay-mcp allow ~/Documents/ai-files
```

**Claude Desktop:** open Settings → Developer → Edit Config and add:

```json
{
  "mcpServers": {
    "ai-file-relay": {
      "command": "/full/path/to/relay-mcp"
    }
  }
}
```

(`which relay-mcp` prints the full path.) Restart Claude Desktop and ask: *"Use AI File Relay to list my files."*

**Claude Code:** `claude mcp add ai-file-relay -- relay-mcp`

**Over HTTP instead of stdio:** run `relay-mcp serve`, then connect your MCP client to `http://127.0.0.1:<port>/mcp` with the header `Authorization: Bearer <token>`. `relay-mcp config --show-token` prints both.

### Tools

| Tool | What it does | Works in M1? |
| --- | --- | --- |
| `list_files` | Lists allowed folders and what is inside them | Yes |
| `relay_status` | Shows setup, connection and recent transfers | Yes |
| `list_tabs` | Lists open Chrome tabs | Needs the extension (M2) |
| `upload_to_page` | Puts a file into a page's upload box | Checks path, size and approval; then needs the extension (M2) |
| `wait_for_download` | Returns a file the page downloaded | Needs the extension (M2) |

### Settings

`relay-mcp config` shows where the settings file is. The main settings:

| Setting | Default | Meaning |
| --- | --- | --- |
| `allowed_folders` | none | Folders the AI may use (`relay-mcp allow` / `disallow`) |
| `approval` | `"ask"` | `"ask"` needs the desktop app (M3), so for now transfers are refused. `"allow_all"` allows every transfer inside allowed folders: only for testing. `"deny_all"` turns transfers off. |
| `max_file_mb` | `100` | Largest file that can be uploaded |
| `port` | random | Port of the local hub on 127.0.0.1 |

## Build from source

You need Rust 1.88 or newer, and Node.js 20 or newer for the extension.

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
