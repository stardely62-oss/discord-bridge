// Renders the tray icons from assets/brand/tray-*.svg (the approved app icon
// plus a status indicator). The app icon itself is NOT generated: the PNG set
// and icon.ico in assets/brand/png come from the approved logo package as is.
//   node scripts/icons/render.mjs [outdir=assets/brand/png]
// Needs Playwright Chromium (not a project dependency). See assets/brand/README.md.
import fs from "node:fs";
import path from "node:path";
import { chromium } from "playwright";

const src = path.resolve("assets/brand");
const out = path.resolve(process.argv[2] ?? "assets/brand/png");
fs.mkdirSync(out, { recursive: true });

const browser = await chromium.launch();
async function render(file, size, dest) {
  const svg = fs.readFileSync(path.join(src, file), "utf8").replace("<svg ", `<svg width="${size}" height="${size}" `);
  const page = await browser.newPage({ viewport: { width: size, height: size } });
  await page.setContent(`<html><body style="margin:0;background:transparent">${svg}</body></html>`);
  await page.screenshot({ path: path.join(out, dest), omitBackground: true });
  await page.close();
}
for (const s of ["on", "off"]) {
  await render(`tray-${s}.svg`, 64, `tray-${s}.png`);
  await render(`tray-${s}.svg`, 32, `tray-${s}-32.png`);
}
await browser.close();
console.log(`tray icons written to ${out}`);
