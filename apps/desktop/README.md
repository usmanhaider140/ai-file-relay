# Desktop app (hub)

Coming in **M3**. This will be a Tauri 2 app that:

- runs in the menu bar / system tray
- hosts the MCP endpoint and the local file server
- shows the approval popup for each transfer
- manages allowed folders and shows transfer history
- offers one-click "Connect" for Claude Desktop, Cursor and VS Code
- installs the native messaging host manifest for Chrome

Until then, the logic it will use lives in `crates/`.
