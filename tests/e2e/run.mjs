// End-to-end test: real Chrome + the real extension + the real relay-mcp.
//
// It plays the AI app: it starts `relay-mcp` over stdio (like Claude Desktop
// does) and calls the MCP tools, while Chrome shows a test page.
//
//   cargo build -p ai-file-relay
//   (cd apps/extension && npm ci && npm run build)
//   (cd tests/e2e && npm ci && CHROME_PATH=/path/to/chrome npm test)
//
// Chrome must allow --load-extension: use Chromium or Chrome for Testing
// (branded Google Chrome ignores that flag).

import { spawn, execFileSync } from "node:child_process";
import { createServer } from "node:http";
import { mkdtempSync, readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { createHash } from "node:crypto";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { createInterface } from "node:readline";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../..");
const exe = process.platform === "win32" ? "relay-mcp.exe" : "relay-mcp";
const relayBin = process.env.RELAY_MCP ?? join(root, "target", "debug", exe);
const extensionDir = join(root, "apps", "extension", "dist");
const chromePath = process.env.CHROME_PATH ?? "/usr/bin/google-chrome";

const results = [];
function check(name, ok, detail = "") {
  results.push({ name, ok });
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? `  (${detail})` : ""}`);
}

for (const [what, path] of [["relay-mcp", relayBin], ["built extension", join(extensionDir, "manifest.json")], ["Chrome", chromePath]]) {
  if (!existsSync(path)) {
    console.error(`missing ${what}: ${path}`);
    process.exit(2);
  }
}

// ---------- test files and settings ----------

const work = mkdtempSync(join(tmpdir(), "relay-e2e-"));
const docs = join(work, "docs");
mkdirSync(docs);
writeFileSync(join(docs, "cv.txt"), "Usman's CV: Rust, TypeScript");
writeFileSync(join(work, "secret.txt"), "not for the AI");
// 2.5 MB, so it travels to the page in several pieces.
const big = Buffer.alloc(2.5 * 1024 * 1024);
for (let i = 0; i < big.length; i++) big[i] = (i * 31 + (i >> 8)) & 0xff;
writeFileSync(join(docs, "big.bin"), big);
const bigHash = createHash("sha256").update(big).digest("hex");

const configPath = join(work, "config.toml");
const env = { ...process.env, AI_FILE_RELAY_CONFIG: configPath, RUST_LOG: "warn" };
const relay = (...args) => execFileSync(relayBin, args, { env, encoding: "utf8" });
relay("init");
relay("allow", docs);
let config = readFileSync(configPath, "utf8")
  .replace('approval = "ask"', 'approval = "allow_all"')
  .replace(/^inbox_folder = .*$/m, `inbox_folder = ${JSON.stringify(join(work, "inbox"))}`)
  .replace(/^history_file = .*$/m, `history_file = ${JSON.stringify(join(work, "history.jsonl"))}`);
writeFileSync(configPath, config);

// ---------- the test web site ----------

const page = readFileSync(join(here, "page.html"));
const site = createServer((req, res) => {
  if (req.url === "/download") {
    res.writeHead(200, {
      "Content-Type": "text/csv",
      "Content-Disposition": 'attachment; filename="report.csv"',
    });
    res.end("month,total\nOct,42\nNov,57\n");
  } else {
    res.writeHead(200, { "Content-Type": "text/html" });
    res.end(page);
  }
});
await new Promise((r) => site.listen(0, "127.0.0.1", r));
const siteUrl = `http://127.0.0.1:${site.address().port}/`;

// ---------- the AI app: relay-mcp over stdio ----------

const mcp = spawn(relayBin, [], { env, stdio: ["pipe", "pipe", "inherit"] });
const lines = createInterface({ input: mcp.stdout });
const waiting = new Map();
lines.on("line", (line) => {
  const msg = JSON.parse(line);
  if (msg.id !== undefined && waiting.has(msg.id)) {
    waiting.get(msg.id)(msg);
    waiting.delete(msg.id);
  }
});
let nextId = 1;
function rpc(method, params) {
  const id = nextId++;
  mcp.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
  return new Promise((r) => waiting.set(id, r));
}
async function tool(name, args = {}) {
  const res = await rpc("tools/call", { name, arguments: args });
  const r = res.result;
  return { error: r.isError === true, data: r.structuredContent, text: r.content?.[0]?.text ?? "" };
}
await rpc("initialize", {
  protocolVersion: "2025-06-18",
  capabilities: {},
  clientInfo: { name: "e2e", version: "1" },
});
mcp.stdin.write(JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" }) + "\n");

// ---------- Chrome with the extension ----------

const profile = join(work, "chrome-profile");
// Chrome looks for native hosts inside the profile folder on Linux/macOS.
relay("install-native-host", "--dir", join(profile, "NativeMessagingHosts"));
const downloads = join(work, "chrome-downloads");
mkdirSync(downloads);

const browser = await chromium.launchPersistentContext(profile, {
  executablePath: chromePath,
  headless: true,
  acceptDownloads: true,
  downloadsPath: downloads,
  env,
  args: [
    `--disable-extensions-except=${extensionDir}`,
    `--load-extension=${extensionDir}`,
    "--no-sandbox",
  ],
});

// The page handles a new file in an async event handler, so wait for the
// result text to change instead of reading it at once.
async function pageText(locator, startsWith, timeoutMs = 10_000) {
  const until = Date.now() + timeoutMs;
  let text = "";
  while (Date.now() < until) {
    text = (await locator.textContent()) ?? "";
    if (text.startsWith(startsWith)) return text;
    await new Promise((r) => setTimeout(r, 100));
  }
  return text;
}

let exitCode = 1;
try {
  const tab = browser.pages()[0] ?? (await browser.newPage());
  await tab.goto(siteUrl);

  // Wait for the extension to reach the hub.
  let status;
  for (let i = 0; i < 40; i++) {
    status = (await tool("relay_status")).data;
    if (status.browser_connected) break;
    await new Promise((r) => setTimeout(r, 500));
  }
  check("extension connects to the hub", status.browser_connected, status.browser_detail);

  const tabs = await tool("list_tabs");
  const testTab = tabs.data?.tabs?.find((t) => t.url.startsWith(siteUrl));
  check("list_tabs finds the test page", Boolean(testTab), tabs.text.slice(0, 120));

  // Upload into a hidden file input.
  const up = await tool("upload_to_page", { path: join(docs, "cv.txt"), tab_id: testTab.id });
  check("upload_to_page succeeds", !up.error, up.text.slice(0, 200));
  const chosen = await pageText(tab.locator("#cv-result"), "cv.txt");
  check(
    "the page received the file",
    chosen === "cv.txt|28|text/plain|Usman's CV: Rust, TypeScript",
    chosen,
  );

  // Upload into a drag-and-drop area.
  const drop = await tool("upload_to_page", {
    path: join(docs, "cv.txt"),
    tab_id: testTab.id,
    selector: "#drop",
  });
  check("upload to a drop zone succeeds", !drop.error, drop.text.slice(0, 200));
  const dropped = await pageText(tab.locator("#drop-result"), "cv.txt");
  check("the drop zone received the file", dropped.startsWith("cv.txt|28|"), dropped);

  // A bigger file arrives in pieces and is put back together exactly.
  const bigUp = await tool("upload_to_page", { path: join(docs, "big.bin"), tab_id: testTab.id });
  check("2.5 MB upload succeeds", !bigUp.error, bigUp.text.slice(0, 200));
  const bigResult = await pageText(tab.locator("#cv-result"), "big.bin");
  check("2.5 MB file arrives byte for byte", bigResult === `big.bin|${big.length}|sha256:${bigHash}`, bigResult);

  // A file outside the allowed folder is refused before anything happens.
  const bad = await tool("upload_to_page", { path: join(work, "secret.txt"), tab_id: testTab.id });
  check("file outside allowed folders is refused", bad.error && /outside/.test(bad.text), bad.text);

  // A wrong selector gives a clear error.
  const nosel = await tool("upload_to_page", {
    path: join(docs, "cv.txt"),
    tab_id: testTab.id,
    selector: "#nope",
  });
  check("unknown selector is a clear error", nosel.error && /#nope/.test(nosel.text), nosel.text);

  // Download: start waiting, then click the link.
  // (Playwright saves downloads under random names, so the inbox file gets
  // that name here; in normal Chrome it is the real file name.)
  const wait = tool("wait_for_download", { tab_id: testTab.id, timeout_seconds: 20 });
  await new Promise((r) => setTimeout(r, 500));
  await tab.click("#dl");
  const dl = await wait;
  check("wait_for_download returns a path", !dl.error, dl.text.slice(0, 200));
  if (!dl.error) {
    const text = readFileSync(dl.data.path, "utf8");
    check("downloaded file is in the inbox with the right content",
      dl.data.path.startsWith(join(work, "inbox")) && text === "month,total\nOct,42\nNov,57\n",
      dl.data.path);
  }

  // The history shows the site of each transfer.
  const history = readFileSync(join(work, "history.jsonl"), "utf8").trim().split("\n").map((l) => JSON.parse(l));
  check("history records the site", history.some((e) => e.site === "127.0.0.1" && e.result === "done"),
    `${history.length} entries`);

  exitCode = results.every((r) => r.ok) ? 0 : 1;
} catch (e) {
  console.error(e);
} finally {
  await browser.close().catch(() => {});
  mcp.stdin.end();
  mcp.kill();
  site.close();
}
console.log(`\n${results.filter((r) => r.ok).length}/${results.length} checks passed`);
process.exit(exitCode);
