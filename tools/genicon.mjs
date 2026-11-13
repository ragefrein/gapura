// Renders tools/icon.svg to src-tauri/icons/icon.png (1024x1024).
// Requires sharp: npm install --prefix tools sharp  (one-time)
// Then generate all platform sizes:
//   npx --prefix ui tauri icon src-tauri/icons/icon.png
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const out = join(root, "src-tauri", "icons", "icon.png");
mkdirSync(dirname(out), { recursive: true });

await sharp(join(root, "tools", "icon.svg"), { density: 300 })
  .resize(1024, 1024)
  .png()
  .toFile(out);

console.log("wrote", out);
