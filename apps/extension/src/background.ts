// Background service worker.
//
// Connects to the native messaging host (relay-mcp, started by Chrome),
// which connects to the hub. Answers the hub's requests:
//   list_tabs       → the open tabs
//   upload_to_page  → fetch the file from the hub, put it into the page
//   watch_download  → report the next finished download
//
// The toolbar badge shows the state: no badge = ready, "…" = waiting for
// the hub, "!" = the native host is not installed.

import {
  NATIVE_HOST_NAME,
  PROTOCOL_VERSION,
  type ExtensionToHub,
  type HubToExtension,
} from "./protocol.js";

/** Size of each piece sent to the page. Base64 makes it 4/3 bigger. */
const CHUNK_BYTES = 1024 * 1024;

let port: chrome.runtime.Port | null = null;
let retryMs = 1000;

// ---------- connection ----------

function send(msg: ExtensionToHub): void {
  try {
    port?.postMessage(msg);
  } catch (e) {
    console.warn("AI File Relay: could not send", e);
  }
}

type BadgeState = "ready" | "waiting" | "no-host";

function setBadge(state: BadgeState, detail: string): void {
  const badge: Record<BadgeState, [string, string]> = {
    ready: ["", "#1d9e75"],
    waiting: ["…", "#ba7517"],
    "no-host": ["!", "#a32d2d"],
  };
  const [text, color] = badge[state];
  void chrome.action.setBadgeText({ text });
  void chrome.action.setBadgeBackgroundColor({ color });
  void chrome.action.setTitle({ title: `AI File Relay: ${detail}` });
}

function connect(): void {
  if (port) return;
  try {
    port = chrome.runtime.connectNative(NATIVE_HOST_NAME);
  } catch (e) {
    console.warn("AI File Relay: connectNative failed", e);
    scheduleReconnect("could not start the native host");
    return;
  }
  port.onMessage.addListener((msg: HubToExtension) => {
    void handle(msg);
  });
  port.onDisconnect.addListener(() => {
    const reason = chrome.runtime.lastError?.message ?? "the native host stopped";
    port = null;
    scheduleReconnect(reason);
  });
  setBadge("waiting", "starting…");
  send({
    type: "hello",
    extension_version: chrome.runtime.getManifest().version,
    protocol_version: PROTOCOL_VERSION,
  });
}

function scheduleReconnect(reason: string): void {
  const missing = /not found|forbidden/i.test(reason);
  setBadge(
    "no-host",
    missing
      ? "the native host is not installed. Run `relay-mcp install-native-host`, then restart Chrome."
      : reason,
  );
  setTimeout(connect, retryMs);
  retryMs = Math.min(retryMs * 2, 60_000);
}

// ---------- requests from the hub ----------

async function handle(msg: HubToExtension): Promise<void> {
  switch (msg.type) {
    case "hub_status":
      if (msg.connected) {
        retryMs = 1000;
        setBadge("ready", "connected");
      } else {
        setBadge("waiting", msg.detail ?? "waiting for the hub");
      }
      break;
    case "welcome":
      retryMs = 1000;
      setBadge("ready", `connected to hub ${msg.hub_version}`);
      break;
    case "pong":
      break;
    case "error":
      console.warn(`AI File Relay: ${msg.message}`);
      setBadge("waiting", msg.message);
      break;
    case "list_tabs": {
      const tabs = await chrome.tabs.query({});
      send({
        type: "tabs",
        request_id: msg.request_id,
        tabs: tabs
          .filter((t) => t.id !== undefined)
          .map((t) => ({ id: t.id as number, title: t.title ?? "", url: t.url ?? "" })),
      });
      break;
    }
    case "upload_to_page": {
      const error = await uploadToPage(msg).catch((e: unknown) => String(e));
      send(
        error === null
          ? { type: "upload_result", request_id: msg.request_id, ok: true }
          : { type: "upload_result", request_id: msg.request_id, ok: false, error },
      );
      break;
    }
    case "watch_download":
      void watchDownload(msg.request_id, msg.tab_id ?? null, msg.timeout_ms);
      break;
  }
}

// ---------- upload ----------

type UploadMsg = Extract<HubToExtension, { type: "upload_to_page" }>;

/** Returns null when the file is in the page, or an error message. */
async function uploadToPage(msg: UploadMsg): Promise<string | null> {
  const res = await fetch(msg.file_url, { cache: "no-store" });
  if (!res.ok) return `could not get the file from the hub (HTTP ${res.status})`;
  const bytes = new Uint8Array(await res.arrayBuffer());

  await chrome.scripting.executeScript({
    target: { tabId: msg.tab_id },
    files: ["content.js"],
  });

  const transfer = crypto.randomUUID();
  const toPage = (m: object) =>
    chrome.tabs.sendMessage(msg.tab_id, m, { frameId: 0 }) as Promise<PageAnswer>;

  await toPage({
    kind: "relay_begin",
    transfer,
    name: msg.file_name,
    mime: msg.mime_type,
    selector: msg.selector ?? null,
  });
  for (let start = 0; start < bytes.length; start += CHUNK_BYTES) {
    const data = toBase64(bytes.subarray(start, start + CHUNK_BYTES));
    await toPage({ kind: "relay_chunk", transfer, data });
  }
  const answer = await toPage({ kind: "relay_end", transfer });
  return answer?.ok ? null : (answer?.error ?? "the page did not answer");
}

interface PageAnswer {
  ok: boolean;
  error?: string;
}

function toBase64(bytes: Uint8Array): string {
  let binary = "";
  const step = 0x8000;
  for (let i = 0; i < bytes.length; i += step) {
    binary += String.fromCharCode(...bytes.subarray(i, i + step));
  }
  return btoa(binary);
}

// ---------- downloads ----------

function hostOf(url: string | undefined): string | null {
  if (!url) return null;
  try {
    return new URL(url.startsWith("blob:") ? url.slice(5) : url).hostname || null;
  } catch {
    return null;
  }
}

function sameSite(host: string | null, site: string): boolean {
  return host !== null && (host === site || host.endsWith(`.${site}`) || site.endsWith(`.${host}`));
}

async function watchDownload(
  request_id: string,
  tabId: number | null,
  timeoutMs: number,
): Promise<void> {
  const startedAt = Date.now() - 2000;
  let site: string | null = null;
  if (tabId !== null) {
    try {
      site = hostOf((await chrome.tabs.get(tabId)).url);
    } catch {
      send({ type: "download_failed", request_id, error: `there is no tab ${tabId}` });
      return;
    }
  }

  let done = false;
  const finish = (msg: ExtensionToHub) => {
    if (done) return;
    done = true;
    clearTimeout(timer);
    chrome.downloads.onChanged.removeListener(onChanged);
    send(msg);
  };

  const matches = (item: chrome.downloads.DownloadItem) =>
    new Date(item.startTime).getTime() >= startedAt &&
    (site === null ||
      [item.referrer, item.finalUrl, item.url].some((u) => sameSite(hostOf(u), site as string)));

  const onChanged = async (delta: chrome.downloads.DownloadDelta) => {
    const state = delta.state?.current;
    if (state !== "complete" && state !== "interrupted") return;
    const [item] = await chrome.downloads.search({ id: delta.id });
    if (!item || !matches(item)) return;
    if (state === "complete") {
      finish({
        type: "download_finished",
        request_id,
        url: item.finalUrl || item.url,
        path: item.filename,
      });
    } else {
      finish({
        type: "download_failed",
        request_id,
        error: `the download failed (${item.error ?? "unknown reason"})`,
      });
    }
  };

  const timer = setTimeout(
    () =>
      finish({
        type: "download_failed",
        request_id,
        error: `no download finished within ${Math.round(timeoutMs / 1000)} seconds`,
      }),
    timeoutMs,
  );
  chrome.downloads.onChanged.addListener(onChanged);
}

// ---------- start ----------

chrome.runtime.onStartup.addListener(connect);
chrome.runtime.onInstalled.addListener(connect);
connect();
