// Message types and constants shared with the Rust side.
// Both files in ./generated are written from crates/relay-protocol by
// `cargo test`. Do not edit them by hand; CI checks they are up to date.

export type { ExtensionToHub, HubToExtension, TabInfo } from "./generated/protocol.js";
export { NATIVE_HOST_NAME, DEV_EXTENSION_ID, PROTOCOL_VERSION } from "./generated/constants.js";
