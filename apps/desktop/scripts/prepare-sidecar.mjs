// Builds relay-mcp and copies it to src-tauri/binaries/ with the target
// triple in its name, which is how Tauri finds a sidecar ("externalBin").
// The installed app then ships relay-mcp next to its own executable: Chrome
// starts it as the native messaging host, and AI apps start it over stdio.
//
// SIDECAR_PROFILE=debug makes a faster, unoptimised build (for CI checks).

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../../..");
const profile = process.env.SIDECAR_PROFILE ?? "release";
const target = process.env.TAURI_ENV_TARGET_TRIPLE ??
  execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (.+)$/m)[1];
const ext = target.includes("windows") ? ".exe" : "";

const args = ["build", "-p", "relay-mcp", "--target", target];
if (profile === "release") args.push("--release");
execFileSync("cargo", args, { cwd: root, stdio: "inherit" });

const built = join(root, "target", target, profile, `relay-mcp${ext}`);
const out = join(here, "..", "src-tauri", "binaries", `relay-mcp-${target}${ext}`);
mkdirSync(dirname(out), { recursive: true });
copyFileSync(built, out);
console.log(`sidecar ready: ${out}`);
