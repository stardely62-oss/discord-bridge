// node scripts/preview/shoot.mjs <outdir> — screenshots of the key screens (mocked backend).
// Each shot: [file, query, width, height, ...steps]; a step is "nav:<label>", "tab:<label>", "btn:<label>" or "text:<text>".
import { chromium } from "playwright";
const out = process.argv[2];
const base = process.env.PREVIEW_URL ?? "http://localhost:1430/";
const browser = await chromium.launch();
const shots = [
  ["01-home", "s=running", 1100, 720],
  ["02-home-more", "s=running", 1100, 860, "btn:Ещё"],
  ["03-home-stopped", "s=stopped", 1100, 720],
  ["04-home-telegram-active", "s=running&tg=active", 1100, 720],
  ["05-home-light", "s=running&t=light", 1100, 720],
  ["06-home-narrow", "s=running", 880, 640],
  ["07-strategies-builtin", "s=running", 1100, 720, "nav:Стратегии"],
  ["08-strategies-community", "s=running", 1100, 720, "nav:Стратегии", "tab:Сообщество"],
  ["09-strategies-mine-editor", "s=running", 1100, 720, "nav:Стратегии", "tab:Мои", "tab:Редактор"],
  ["10-strategies-import", "s=running", 1100, 720, "nav:Стратегии", "tab:Импорт"],
  ["11-apps-catalog", "s=running", 1100, 800, "nav:Приложения"],
  ["12-apps-mine", "s=running", 1100, 720, "nav:Приложения", "tab:Мои · 5 · ↑1"],
  ["13-apps-manual", "s=running", 1100, 720, "nav:Приложения", "tab:Ручные списки"],
  ["22-catalog-update", "s=running&cat=update", 1100, 800, "nav:Приложения", "tab:Мои · 5 · ↑2"],
  ["23-catalog-diff", "s=running&cat=update", 1100, 800, "nav:Приложения", "btn:Обновить…"],
  ["24-catalog-details", "s=running&ip=none", 1100, 800, "nav:Приложения", "text:Telegram"],
  ["25-catalog-offline", "s=running&cat=offline", 1100, 720, "nav:Приложения"],
  ["26-catalog-orphan", "s=running&cat=orphan", 1100, 720, "nav:Приложения", "tab:Мои · 6 · ↑1"],
  ["27-catalog-light", "s=running&t=light", 1100, 800, "nav:Приложения"],
  ["19-apps-catalog-ip-none", "s=running&ip=none", 1100, 900, "nav:Приложения"],
  ["20-apps-catalog-ip-selected", "s=running&ip=selected", 1100, 900, "nav:Приложения"],
  ["21-apps-manual-ip-include", "s=running&ip=none", 1100, 720, "nav:Приложения", "tab:Ручные списки", "text:IP через обход"],
  ["14-apps-catalog-light", "s=running&t=light", 1100, 800, "nav:Приложения"],
  ["15-diagnostics", "s=running", 1100, 760, "nav:Диагностика"],
  ["16-settings", "s=running", 1100, 900, "nav:Настройки", "tab:Обход"],
  ["17-onboarding", "s=onboarding", 1100, 720],
  ["18-home-en", "s=running&l=en", 1100, 720],
];
for (const [name, q, w, h, ...steps] of shots) {
  const page = await browser.newPage({ viewport: { width: w, height: h }, deviceScaleFactor: 1 });
  page.on("pageerror", (e) => console.log(name, "PAGEERROR", e.message));
  await page.goto(`${base}?${q}`);
  await page.waitForTimeout(800);
  for (const st of steps) {
    const [k, v] = [st.slice(0, st.indexOf(":")), st.slice(st.indexOf(":") + 1)];
    if (k === "nav") await page.getByRole("navigation").getByRole("button", { name: v, exact: true }).click();
    if (k === "tab") await page.getByRole("tab", { name: v, exact: true }).first().click();
    if (k === "btn") await page.getByRole("button", { name: v, exact: true }).first().click();
    if (k === "text") await page.getByText(v, { exact: true }).first().click();
    await page.waitForTimeout(450);
  }
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${out}/${name}.png` });
  await page.close();
}
await browser.close();
