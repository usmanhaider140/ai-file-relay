# Changelog

All notable changes. Versions follow [Semantic Versioning](https://semver.org/).

## 0.1.0 (unreleased)

First public release.

### Desktop app
- Runs the hub: every AI app shares one browser link, one approval popup and one history
- Approval popup: Allow once, Allow this site for 1 hour, or Deny (no answer in 60 seconds means no)
- Pages for setup steps, allowed folders, activity and settings
- One-click connect for Claude Desktop and Cursor; command to copy for Claude Code
- "Set up Chrome" button; tray menu with Pause all transfers; start at login
- Checks for updates and installs them (signed updates)

### `relay-mcp` (crate `ai-file-relay`)
- MCP tools: `list_files`, `list_tabs`, `upload_to_page`, `wait_for_download`, `relay_status`
- Runs as an MCP server over stdio, as a hub over HTTP (`relay-mcp serve`), as a proxy to a running hub, and as Chrome's native messaging host
- Commands: `init`, `allow`, `disallow`, `config`, `install-native-host`, `uninstall-native-host`

### Chrome extension
- Puts files into normal and hidden file inputs, inputs inside shadow DOM, and drag-and-drop areas
- Reports finished downloads; toolbar badge shows the connection

### Safety
- Hub listens on 127.0.0.1 only; bearer token for MCP and the browser link; localhost `Host` check
- Only allowed folders are reachable; `..` and symlinks cannot escape; folders like `~/.ssh` always blocked
- One-time file links that expire after 60 seconds and are cancelled if an upload fails
- Native host only accepts our extension's ID
