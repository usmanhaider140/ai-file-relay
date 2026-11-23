# AI File Relay

**Let any AI app put files into web pages and get downloads back, safely, without spinning up a temporary server.**

> **Status: early work (M2).** Uploads and downloads work end to end in Chrome, tested in CI with real Chrome. The desktop app with the approval popup (M3) is not built yet, so for now you turn approvals on or off in the settings file. Watch or star the repo to follow along.

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
| **M2 · Extension** ✅ | Uploads and downloads working end to end |
| **M3 · Desktop app** | Tray app, approval popup, installers |
| **M4 · Release v0.1** | Signed builds, Chrome Web Store, MCP registry |

## Project layout

```
crates/
  relay-protocol/   shared message types
  relay-core/       allowed folders, path checks, one-time links, approvals, history, config
  relay-mcp/        the `relay-mcp` command: MCP server, local hub, and the
                    native messaging host that Chrome starts
  relay-nmh/        Chrome native messaging framing
apps/
  desktop/          Tauri 2 desktop app (M3)
  extension/        Chrome MV3 extension
tests/e2e/          end-to-end test with real Chrome
docs/               architecture and protocol notes
```

## Try it

You need Rust 1.88 or newer, Node.js 20 or newer, and Chrome (or Chromium, Edge, Brave).

**1. Install `relay-mcp` and allow a folder**

```sh
cargo install --git https://github.com/usmanhaider140/ai-file-relay relay-mcp
relay-mcp init
relay-mcp allow ~/Documents/ai-files
```

**2. Install the Chrome extension** (until it is in the Chrome Web Store)

```sh
git clone https://github.com/usmanhaider140/ai-file-relay
cd ai-file-relay/apps/extension
npm install && npm run build
```

Open `chrome://extensions`, turn on **Developer mode**, click **Load unpacked** and pick the `apps/extension/dist` folder.

**3. Let the extension start `relay-mcp`**

```sh
relay-mcp install-native-host
```

Then restart Chrome. The extension's icon has no badge when everything is connected, `…` while it waits for your AI app, and `!` if step 3 is missing.

**4. Connect your AI app**

*Claude Desktop:* open Settings → Developer → Edit Config and add:

```json
{
  "mcpServers": {
    "ai-file-relay": {
      "command": "/full/path/to/relay-mcp"
    }
  }
}
```

(`which relay-mcp` prints the full path.) Restart Claude Desktop and ask: *"Use AI File Relay to upload cv.pdf to the job form in my browser."*

*Claude Code:* `claude mcp add ai-file-relay -- relay-mcp`

*Over HTTP instead of stdio:* run `relay-mcp serve`, then connect your MCP client to `http://127.0.0.1:<port>/mcp` with the header `Authorization: Bearer <token>`. `relay-mcp config --show-token` prints both.

**5. Turn on transfers (until the desktop app exists)**

Approvals are set to `"ask"`, but the approval popup comes with the desktop app (M3). Until then, every transfer is refused. To test, set `approval = "allow_all"` in the settings file (`relay-mcp config` shows where it is). Only do this on your own machine, with folders you are happy for the AI to use.

### Tools

| Tool | What it does |
| --- | --- |
| `list_files` | Lists allowed folders and what is inside them |
| `list_tabs` | Lists open Chrome tabs |
| `upload_to_page` | Puts a file into a page's upload box: a normal or hidden file input, or a drag-and-drop area (pass `selector`) |
| `wait_for_download` | Waits for the page's next download and returns its path in the inbox folder |
| `relay_status` | Shows setup, connection and recent transfers |

### Good to know

- **One hub at a time.** The first `relay-mcp` that starts (for example from Claude Desktop) becomes the hub and talks to Chrome. A second AI app running its own `relay-mcp` at the same time can list files but cannot reach the browser. The desktop app (M3) will fix this by being the one hub for all apps.
- **Why the extension asks for access to all sites:** it can only put a file into a page it is allowed to script. It does nothing on a page until the AI asks for an upload there.
- **Downloads** are copied into the inbox folder and never overwrite an older file.

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

# End-to-end test with real Chrome (Chromium or Chrome for Testing)
cargo build -p relay-mcp
cd tests/e2e && npm install && CHROME_PATH=/path/to/chrome npm test
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
