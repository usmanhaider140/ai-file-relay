// Sets one version everywhere: Rust workspace, desktop app, extension.
//   node scripts/set-version.mjs 0.2.0
// Then commit, and tag with: git tag v0.2.0

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$/.test(version ?? "")) {
  console.error("usage: node scripts/set-version.mjs <version>   (for example 0.2.0)");
  process.exit(1);
}
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const edit = (file, change) => {
  const path = join(root, file);
  const before = readFileSync(path, "utf8");
  const after = change(before);
  if (after === before) throw new Error(`nothing changed in ${file}`);
  writeFileSync(path, after);
  console.log(`updated ${file}`);
};

// Rust: the workspace version, and the version on each internal dependency.
edit("Cargo.toml", (s) =>
  s
    .replace(/(\[workspace\.package\][^\[]*?\nversion = ")[^"]+"/, `$1${version}"`)
    .replace(/(package = "ai-file-relay[^"]*", path = "[^"]+", version = ")[^"]+"/g, `$1${version}"`),
);
// JSON files with a top-level "version".
for (const file of [
  "apps/desktop/src-tauri/tauri.conf.json",
  "apps/desktop/package.json",
  "apps/extension/package.json",
  "apps/extension/manifest.json",
]) {
  edit(file, (s) => s.replace(/("version": ")[^"]+"/, `$1${version}"`));
}
console.log(`\nNow run: cargo check && git commit -am "chore: release v${version}" && git tag v${version}`);
