// Background service worker.
// M0: connect to the native messaging host and say hello.
// M2: fill upload boxes and report finished downloads.

import {
  NATIVE_HOST_NAME,
  PROTOCOL_VERSION,
  type ExtensionToHub,
  type HubToExtension,
} from "./protocol.js";

let port: chrome.runtime.Port | null = null;

function send(msg: ExtensionToHub): void {
  port?.postMessage(msg);
}

function handle(msg: HubToExtension): void {
  switch (msg.type) {
    case "welcome":
      console.log(`AI File Relay: connected to hub ${msg.hub_version}`);
      break;
    case "pong":
      console.log("AI File Relay: pong");
      break;
    case "upload_to_page":
      // Coming in M2.
      send({
        type: "upload_result",
        request_id: msg.request_id,
        ok: false,
        error: "uploads are not built yet",
      });
      break;
    case "error":
      console.warn(`AI File Relay: hub error: ${msg.message}`);
      break;
  }
}

function connect(): void {
  port = chrome.runtime.connectNative(NATIVE_HOST_NAME);
  port.onMessage.addListener((msg: HubToExtension) => handle(msg));
  port.onDisconnect.addListener(() => {
    const reason = chrome.runtime.lastError?.message ?? "closed";
    console.warn(`AI File Relay: host disconnected (${reason})`);
    port = null;
  });
  send({
    type: "hello",
    extension_version: chrome.runtime.getManifest().version,
    protocol_version: PROTOCOL_VERSION,
  });
}

chrome.runtime.onStartup.addListener(connect);
chrome.runtime.onInstalled.addListener(connect);
