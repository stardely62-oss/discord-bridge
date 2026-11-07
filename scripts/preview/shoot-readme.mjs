// node scripts/preview/shoot-readme.mjs [outdir] — the README screenshots (docs/screenshots/).
// Preview mode readme=1: current version, a normal working setup, the built-in
// catalog; demo data, no real Windows state. Needs the preview server
// (vite --config scripts/preview/vite.config.ts) and playwright.
import { chromium } from "playwright";
const out = process.argv[2] ?? "docs/screenshots";
const b = await chromium.launch();
async function shot(name, q, w, h, steps = []) {
  const p = await b.newPage({ viewport: { width: w, height: h }, deviceScaleFactor: 1 });
  p.on("pageerror", (e) => console.log(name, "PAGEERROR", e.message));
  await p.goto(`http://localhost:1430/?readme=1&${q}`);
  await p.waitForTimeout(1000);
  for (const s of steps) {
    const [k, v] = s.split(/:(.*)/);
    if (k === "nav") await p.getByRole("button", { name: v, exact: true }).first().click();
    if (k === "tab") await p.getByText(v, { exact: true }).first().click();
    if (k === "btn") await p.getByRole("button", { name: v }).first().click();
    await p.waitForTimeout(600);
  }
  const notices = await p.locator('[role="status"]').count();
  const text = await p.evaluate(() => document.body.innerText);
  console.log(name, "notices", notices, "alpha", /alpha/.test(text), "steam", /steam|twitch/i.test(text));
  await p.screenshot({ path: `${out}/${name}.png` });
  await p.close();
}
await shot("home", "s=running&tg=connected", 1100, 720);
await shot("strategies", "s=running", 1100, 720, ["nav:Стратегии"]);
await shot("apps", "s=running", 1100, 720, ["nav:Приложения"]);
await shot("telegram", "s=running&tg=connected", 1100, 720, ["nav:Настройки", "tab:Telegram Proxy"]);
await b.close();
