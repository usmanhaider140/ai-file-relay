// Copies files that are not built by TypeScript into dist/.
import { copyFileSync, cpSync, mkdirSync } from "node:fs";

mkdirSync("dist", { recursive: true });
copyFileSync("manifest.json", "dist/manifest.json");
cpSync("icons", "dist/icons", { recursive: true });
console.log("Copied manifest.json and icons to dist/");
