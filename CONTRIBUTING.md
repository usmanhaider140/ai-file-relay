# Contributing to AI File Relay

Thanks for helping! This guide keeps things simple.

## Before you start

- For a small fix (typo, small bug), just open a pull request.
- For a new feature or a bigger change, open an issue first so we can agree on the approach.
- Look for issues labelled **good first issue** if you are new to the project.

## Set up

You need:

- Rust 1.85 or newer (`rustup update stable`)
- Node.js 20 or newer (for the extension)

```sh
git clone https://github.com/usmanhaider140/ai-file-relay.git
cd ai-file-relay
cargo test --workspace
cd apps/extension && npm install && npm run build
```

## Before you open a pull request

Run the same checks CI runs:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd apps/extension && npm run typecheck && npm run build
```

## Pull request tips

- Keep each pull request focused on one change.
- Add tests for new logic, especially anything about file paths, tokens or approvals.
- Use [conventional commit](https://www.conventionalcommits.org/) messages, for example `feat(core): add one-time tokens` or `fix(nmh): handle closed pipe`. The changelog is built from them.
- If you change a message in `crates/relay-protocol`, update `apps/extension/src/protocol.ts` too (until it is generated in M1).

## Code of conduct

Be kind and respectful. See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## License

By contributing, you agree that your work is dual licensed under MIT and Apache-2.0, like the rest of the project.
