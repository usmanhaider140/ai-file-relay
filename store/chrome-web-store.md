# Chrome Web Store listing

Everything to paste into the Chrome Web Store developer dashboard.

## Package

Build the zip with `cd apps/extension && npm ci && npm run build && cd dist && zip -r ../ai-file-relay-extension.zip .`, or download the extension zip from a GitHub release.

## Store listing

**Name:** AI File Relay

**Summary** (max 132 characters):
Lets AI apps like Claude put files into web pages and get downloads back, from folders you allow and only after you approve.

**Category:** Developer Tools

**Language:** English

**Description:**

AI File Relay lets your AI apps (Claude Desktop, Claude Code, Cursor and others that use MCP) move files between your computer and web pages, safely.

Ask your AI to "upload my CV to this job form" or "download the report from this page", and AI File Relay does the file part:

• Puts a file into a page's upload box: normal and hidden file inputs, and drag-and-drop areas
• Catches the next file a page downloads and hands it to your AI
• Lists your open tabs so the AI can pick the right page

You stay in control:

• The AI can only use files inside folders you choose
• A popup asks you before every transfer: Allow once, Allow this site for 1 hour, or Deny
• Nothing is sent to any server: everything runs on your computer
• Free and open source: https://github.com/usmanhaider140/ai-file-relay

This extension needs the free AI File Relay desktop app. Get it, and the setup guide, at https://github.com/usmanhaider140/ai-file-relay

**Graphics:**
- Icon: `apps/extension/icons/icon-128.png`
- Screenshots (1280 × 800): `store/screenshot-1-approve.png`, `store/screenshot-2-done.png`, `store/screenshot-3-app.png`
- Small promo tile (440 × 280): `store/promo-tile-440x280.png`

## Privacy practices

**Single purpose:**
Move files between the user's computer and web pages for the user's AI apps, through the AI File Relay desktop app, with the user's approval for each transfer.

**Permission justifications:**

| Permission | Why |
| --- | --- |
| `nativeMessaging` | Talks to the AI File Relay program on the user's own computer, which the AI apps connect to. This is the only way the extension receives requests. |
| `tabs` | Lists open tabs (title and URL) so the AI can choose which page to upload to, and finds a tab's site for the approval popup. |
| `scripting` | Injects a small script into the one tab the user approved, to put the file into its upload box. Nothing is injected otherwise. |
| `downloads` | Reports the next finished download (file name and location) when the AI asks for it. |
| Host permission `<all_urls>` | The file can be uploaded to whatever site the user is on, so the script must be allowed on that page. It is only injected after the user approves an upload there. Also used to fetch the approved file from the local program on 127.0.0.1. |

**Remote code:** No. All code is in the package.

**Data usage:** Check "Website content" (file contents put into pages) and "Web history" (tab URLs when listing tabs). Then certify:
- I do not sell or transfer user data to third parties
- I do not use or transfer user data for purposes unrelated to the item's single purpose
- I do not use or transfer user data to determine creditworthiness or for lending purposes

**Privacy policy URL:** https://github.com/usmanhaider140/ai-file-relay/blob/main/docs/privacy.md

## After it is approved

The store gives the extension a new ID. Add it so the native host accepts it:

1. In `crates/relay-protocol/src/lib.rs`, add the store ID next to `DEV_EXTENSION_ID` and include it in the IDs the installer allows (see `crates/relay-mcp/src/install.rs`, `extension_ids`).
2. Release a new version; users click **Set up Chrome** again in the app.
