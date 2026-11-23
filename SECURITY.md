# Security policy

AI File Relay moves files between AI apps, your computer and the browser, so security matters a lot.

## Reporting a problem

**Please do not open a public issue for security problems.**

Use GitHub's private reporting instead: go to the **Security** tab of this repo and click **Report a vulnerability**.

Please include:

- what the problem is and what an attacker could do with it
- steps to reproduce it
- the version or commit you tested

You should get a reply within 7 days. Once a fix is ready, we will credit you in the release notes unless you ask us not to.

## Supported versions

The project is in early development. Only the latest commit on `main` gets security fixes for now.

## Security goals

- The hub listens on `127.0.0.1` only and every request needs a token.
- An AI can only reach folders the user allowed; `..` and symlinks cannot escape them.
- Sensitive folders such as `~/.ssh` are always blocked.
- The user approves each upload.
- File links work once and expire quickly.
- The native messaging host only accepts our own extension's ID.
