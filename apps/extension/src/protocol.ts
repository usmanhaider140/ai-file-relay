// Message types shared with the Rust side (crates/relay-protocol).
// Written by hand for M0. From M1 this file is generated with ts-rs,
// so keep it in step with crates/relay-protocol/src/lib.rs until then.

export const NATIVE_HOST_NAME = "io.github.usmanhaider140.ai_file_relay";
export const PROTOCOL_VERSION = 1;

export type ExtensionToHub =
  | { type: "hello"; extension_version: string; protocol_version: number }
  | { type: "ping" }
  | { type: "upload_result"; request_id: string; ok: boolean; error?: string }
  | { type: "download_finished"; tab_id: number | null; url: string; path: string };

export type HubToExtension =
  | { type: "welcome"; hub_version: string; protocol_version: number }
  | { type: "pong" }
  | {
      type: "upload_to_page";
      request_id: string;
      tab_id: number;
      token: string;
      file_name: string;
      mime_type: string;
      size: number;
      selector?: string;
    }
  | { type: "error"; message: string };
