# ai-file-relay

The `relay-mcp` command from [AI File Relay](https://github.com/usmanhaider140/ai-file-relay): it lets any AI app (Claude, Cursor and others) put files into web pages in your browser and get downloads back, safely, without spinning up a temporary server.

```sh
cargo install ai-file-relay     # installs the `relay-mcp` command
relay-mcp init
relay-mcp allow ~/Documents/ai-files
relay-mcp install-native-host
```

One program does several jobs:

- **MCP server** over stdio (what AI apps start) with the tools `list_files`, `list_tabs`, `upload_to_page`, `wait_for_download` and `relay_status`
- **Hub** (`relay-mcp serve`): MCP over HTTP, one-time file links and the browser link, on 127.0.0.1 only
- **Proxy**: when a hub (usually the desktop app) is already running, a stdio `relay-mcp` forwards to it
- **Native messaging host** for the Chrome extension (Chrome starts it)

You also need the Chrome extension, and for approval popups, the desktop app. See the [project README](https://github.com/usmanhaider140/ai-file-relay#readme) for the full setup.

License: MIT OR Apache-2.0
