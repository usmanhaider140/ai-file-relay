// Copies files that are not built by TypeScript into dist/.
import { copyFileSync, mkdirSync } from "node:fs";

mkdirSync("dist", { recursive: true });
copyFileSync("manifest.json", "dist/manifest.json");
console.log("Copied manifest.json to dist/");
