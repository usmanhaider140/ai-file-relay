# Privacy policy

_Last updated: 24 November 2026_

AI File Relay (the desktop app, the `relay-mcp` program and the Chrome extension) runs entirely on your own computer. It is open source: you can read every line at <https://github.com/usmanhaider140/ai-file-relay>.

## What it handles

- **Files you allow.** An AI app can only reach files inside folders you choose. Files are read from your disk and put into the web page you approve, or downloaded files are copied into the app's inbox folder.
- **Tab titles and addresses.** When an AI app asks for the list of open tabs, the extension reads their titles and URLs so the AI can pick the right page.
- **Download details.** When an AI app asks to receive a download, the extension reads the file name, address and where Chrome saved it.
- **A local transfer log.** Each transfer (time, file, site, result) is written to a log file on your computer so you can see what happened.

## What it does not do

- It sends nothing to us or to any server we run. There is no account, no analytics and no tracking.
- The extension only talks to the AI File Relay program on your own computer (through Chrome's native messaging and `127.0.0.1`).
- It does not read or change web pages unless an AI app asks for an upload to that page and you approve it.
- It does not sell, share or transfer any data to third parties.

The only network access outside your computer is the desktop app checking GitHub for a new version, which sends no personal data.

Files you upload go to the web site you chose, under that site's own privacy policy.

## Your control

- Choose and remove allowed folders at any time in the desktop app.
- Approve or deny each transfer, or pause all transfers.
- Uninstall the app and the extension, and delete the app's data folder, to remove everything (the log included).

## Contact

Questions: open an issue at <https://github.com/usmanhaider140/ai-file-relay/issues>.
