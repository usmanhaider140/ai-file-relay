// End-to-end test of the desktop app (Linux, needs an X display):
//
//   desktop app (hub + approval popup) ⇄ Chrome extension ⇄ test page
//   AI app → relay-mcp over stdio → forwards to the desktop hub
//
// It answers the popup with real key presses (xdotool), so it tests what a
// person sees and clicks.
//
//   cargo build -p ai-file-relay
//   cargo build -p ai-file-relay-desktop --features tauri/custom-protocol
//   (cd apps/extension && npm ci && npm run build)
//   xvfb-run -a node tests/e2e/desktop.mjs      # needs xdotool
//
// SHOTS=/some/folder saves screenshots of the app and the popup.

import { spawn, execFileSync } from "node:child_process";
import { createServer } from "node:http";
import { mkdtempSync, readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { createInterface } from "node:readline";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../..");
const relayBin = process.env.RELAY_MCP ?? join(root, "target", "debug", "relay-mcp");
const appBin = process.env.DESKTOP_APP ?? join(root, "target", "debug", "ai-file-relay-desktop");
const extensionDir = join(root, "apps", "extension", "dist");
const chromePath = process.env.CHROME_PATH ?? "/usr/bin/google-chrome";
const shots = process.env.SHOTS;

const results = [];
function check(name, ok, detail = "") {
  results.push({ name, ok });
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? `  (${detail})` : ""}`);
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
function screenshot(name) {
  if (!shots) return;
  mkdirSync(shots, { recursive: true });
  try {
    execFileSync("import", ["-window", "root", join(shots, `${name}.png`)]);
  } catch (e) {
    console.warn(`screenshot failed: ${e.message}`);
  }
}

for (const [what, path] of [["relay-mcp", relayBin], ["desktop app", appBin], ["built extension", join(extensionDir, "manifest.json")], ["Chrome", chromePath]]) {
  if (!existsSync(path)) {
    console.error(`missing ${what}: ${path}`);
    process.exit(2);
  }
}
if (!process.env.DISPLAY) {
  console.error("needs an X display: run with xvfb-run");
  process.exit(2);
}

// ---------- a private home folder, so nothing touches the real one ----------

const work = mkdtempSync(join(tmpdir(), "relay-desktop-"));
const home = join(work, "home");
const docs = join(home, "docs");
mkdirSync(docs, { recursive: true });
writeFileSync(join(docs, "cv.txt"), "Usman's CV: Rust, TypeScript");
const configPath = join(home, "relay", "config.toml");
const env = {
  ...process.env,
  HOME: home,
  XDG_CONFIG_HOME: join(home, ".config"),
  XDG_DATA_HOME: join(home, ".local", "share"),
  AI_FILE_RELAY_CONFIG: configPath,
  RUST_LOG: "warn",
  WEBKIT_DISABLE_DMABUF_RENDERER: "1",
  NO_AT_BRIDGE: "1",
};
const relay = (...args) => execFileSync(relayBin, args, { env, encoding: "utf8" });
relay("init");
relay("allow", docs);
writeFileSync(
  configPath,
  readFileSync(configPath, "utf8")
    .replace(/^inbox_folder = .*$/m, `inbox_folder = ${JSON.stringify(join(work, "inbox"))}`),
);

// ---------- test web site ----------

const page = readFileSync(join(here, "page.html"));
const site = createServer((req, res) => {
  if (req.url === "/download") {
    res.writeHead(200, { "Content-Type": "text/csv", "Content-Disposition": 'attachment; filename="report.csv"' });
    res.end("month,total\nOct,42\n");
  } else {
    res.writeHead(200, { "Content-Type": "text/html" });
    res.end(page);
  }
});
await new Promise((r) => site.listen(0, "127.0.0.1", r));
const siteUrl = `http://127.0.0.1:${site.address().port}/`;

// ---------- the desktop app ----------

// The app's own log, printed if something goes wrong.
const appLog = [];
const app = spawn(appBin, [], {
  env: { ...env, RUST_LOG: "info" },
  stdio: ["ignore", "pipe", "pipe"],
});
for (const stream of [app.stdout, app.stderr]) {
  stream.on("data", (d) => appLog.push(d.toString()));
}
let appExit = null;
app.on("exit", (code, signal) => (appExit = `exited with ${code ?? signal}`));
let hubUp = false;
const port = Number(readFileSync(configPath, "utf8").match(/^port = (\d+)/m)[1]);
// The first start can be slow on CI machines (WebKit warms up).
for (let i = 0; i < 240 && !hubUp && appExit === null; i++) {
  await sleep(250);
  hubUp = await fetch(`http://127.0.0.1:${port}/health`)
    .then((r) => r.json())
    .then((j) => j.role === "desktop")
    .catch(() => false);
}

// ---------- Chrome ----------

const profile = join(work, "chrome-profile");
relay("install-native-host", "--dir", join(profile, "NativeMessagingHosts"));
const browser = await chromium.launchPersistentContext(profile, {
  executablePath: chromePath,
  headless: true,
  env,
  args: [`--disable-extensions-except=${extensionDir}`, `--load-extension=${extensionDir}`, "--no-sandbox"],
});

// ---------- the AI app: relay-mcp over stdio (forwards to the desktop hub) ----------

const mcp = spawn(relayBin, [], { env, stdio: ["pipe", "pipe", "inherit"] });
const waiting = new Map();
createInterface({ input: mcp.stdout }).on("line", (line) => {
  const msg = JSON.parse(line);
  if (waiting.has(msg.id)) {
    waiting.get(msg.id)(msg);
    waiting.delete(msg.id);
  }
});
let nextId = 1;
const rpc = (method, params) => {
  const id = nextId++;
  mcp.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
  return new Promise((r) => waiting.set(id, r));
};
const tool = async (name, args = {}) => {
  const r = (await rpc("tools/call", { name, arguments: args })).result;
  return { error: r.isError === true, data: r.structuredContent, text: r.content?.[0]?.text ?? "" };
};
await rpc("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "e2e", version: "1" } });
mcp.stdin.write(JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" }) + "\n");

// ---------- answering the popup like a person ----------

function popupWindow() {
  try {
    const ids = execFileSync("xdotool", ["search", "--onlyvisible", "--name", "approve transfer"], { encoding: "utf8" })
      .trim()
      .split("\n")
      .filter(Boolean);
    return ids[0] ?? null;
  } catch {
    return null;
  }
}
async function waitForPopup(visible, timeoutMs = 10_000) {
  const until = Date.now() + timeoutMs;
  while (Date.now() < until) {
    if ((popupWindow() !== null) === visible) return true;
    await sleep(150);
  }
  return false;
}
/** Press keys in the popup. "Allow once" has the focus; Tab moves on. */
function pressInPopup(keys) {
  const id = popupWindow();
  execFileSync("xdotool", ["windowfocus", "--sync", id]);
  for (const key of keys) {
    execFileSync("xdotool", ["key", "--clearmodifiers", key]);
  }
}
async function pageText(locator, startsWith, timeoutMs = 10_000) {
  const until = Date.now() + timeoutMs;
  let text = "";
  while (Date.now() < until) {
    text = (await locator.textContent()) ?? "";
    if (text.startsWith(startsWith)) return text;
    await sleep(100);
  }
  return text;
}

let exitCode = 1;
try {
  check("desktop app runs the hub", hubUp, appExit ?? "");
  if (!hubUp) {
    console.log("---- desktop app log ----\n" + appLog.join("").slice(-4000));
    throw new Error("the desktop app did not start its hub");
  }
  screenshot("1-app");

  const tab = browser.pages()[0] ?? (await browser.newPage());
  await tab.goto(siteUrl);
  let status;
  for (let i = 0; i < 40; i++) {
    status = (await tool("relay_status")).data;
    if (status?.browser_connected) break;
    await sleep(500);
  }
  check("AI app's relay-mcp forwards to the desktop hub", status?.hub === "desktop", `hub=${status?.hub}`);
  check("extension connects to the desktop hub", status?.browser_connected === true, status?.browser_detail);
  check("approvals are set to ask", status?.approval === "ask");

  const tabs = await tool("list_tabs");
  const testTab = tabs.data?.tabs?.find((t) => t.url.startsWith(siteUrl));
  const cv = join(docs, "cv.txt");

  // 1) Popup → "Allow once".
  let upload = tool("upload_to_page", { path: cv, tab_id: testTab.id });
  check("popup appears for an upload", await waitForPopup(true));
  await sleep(600); // let the popup draw
  screenshot("2-popup");
  pressInPopup(["Return"]);
  let r = await upload;
  check("Allow once: upload succeeds", !r.error, r.text.slice(0, 120));
  check("the page received the file", (await pageText(tab.locator("#cv-result"), "cv.txt")).startsWith("cv.txt|28|"));
  check("popup hides when nothing is waiting", await waitForPopup(false));

  // 2) Popup → "Deny".
  upload = tool("upload_to_page", { path: cv, tab_id: testTab.id, selector: "#drop" });
  await waitForPopup(true);
  await sleep(400);
  pressInPopup(["Tab", "Tab", "Return"]);
  r = await upload;
  check("Deny: upload is refused", r.error && /said no/.test(r.text), r.text.slice(0, 120));

  // 3) Popup → "Allow 127.0.0.1 for 1 hour", then the next upload needs no popup.
  upload = tool("upload_to_page", { path: cv, tab_id: testTab.id, selector: "#drop" });
  await waitForPopup(true);
  await sleep(400);
  pressInPopup(["Tab", "Return"]);
  r = await upload;
  check("Allow site: upload succeeds", !r.error, r.text.slice(0, 120));
  const t0 = Date.now();
  r = await tool("upload_to_page", { path: cv, tab_id: testTab.id });
  check("same site again: allowed without a popup", !r.error && Date.now() - t0 < 3000 && popupWindow() === null);

  await sleep(2000); // let the window refresh its status
  screenshot("3-app-after");
  exitCode = results.every((x) => x.ok) ? 0 : 1;
} catch (e) {
  console.error(e);
  if (results.some((r) => !r.ok)) console.log("---- desktop app log ----\n" + appLog.join("").slice(-4000));
} finally {
  await browser.close().catch(() => {});
  mcp.kill();
  app.kill();
  site.close();
}
console.log(`\n${results.filter((x) => x.ok).length}/${results.length} checks passed`);
process.exit(exitCode);
