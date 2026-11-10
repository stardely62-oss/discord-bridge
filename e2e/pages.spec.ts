import { expect, test } from "./app";

test.describe("Strategies", () => {
  test("all 22 Flowseal strategies are listed and the running one is marked", async ({ app }) => {
    await app.open();
    await app.go("Стратегии");
    await expect(app.main).toContainText("Flowseal 1.10.4 · 22");
    await expect(app.main.getByRole("listitem")).toHaveCount(22);
    await expect(app.main.getByRole("button", { name: /^работает сейчас general \(ALT2\)/ })).toBeVisible();
  });

  test("search narrows the list; a built-in strategy is read-only", async ({ app }) => {
    await app.open();
    await app.go("Стратегии");
    await app.main.getByPlaceholder("Поиск по названию").fill("FAKE TLS");
    await expect(app.main.getByRole("listitem")).toHaveCount(4);
    await app.calls();
    await app.main.getByRole("listitem").first().getByRole("button").click();
    expect((await app.sent("get_strategy")).args).toEqual({ id: "flowseal-general-fake-tls-auto-alt" });
    await expect(app.main.getByRole("heading", { level: 2, name: "general (FAKE TLS AUTO ALT)" })).toBeVisible();
    await expect(app.main).toContainText("встроенная");
    await expect(app.main.getByRole("button", { name: "Копия для правки" })).toBeVisible();
    await expect(app.main.getByRole("button", { name: "Сохранить" })).toHaveCount(0);
  });

  test("«Включить» activates exactly the selected strategy", async ({ app }) => {
    await app.open();
    await app.go("Стратегии");
    await app.main.getByRole("button", { name: /^general \(ALT3\)/ }).click();
    await expect(app.main.getByRole("heading", { level: 2, name: "general (ALT3)" })).toBeVisible();
    await app.calls();
    await app.main.getByRole("button", { name: "Включить", exact: true }).click();
    const call = await app.sent("bypass_enable");
    expect(call.args.id).toBe("flowseal-general-alt3");
  });
});

test.describe("Apps", () => {
  test("catalog, installed packs and manual lists", async ({ app }) => {
    await app.open();
    await app.go("Приложения");
    await expect(app.main).toContainText("Подписанный каталог от 2026-11-06");
    await expect(app.main.getByRole("button", { name: "Discord: подробности" })).toContainText("Встроено в Flowseal");
    await app.main.getByRole("tab", { name: /^Мои/ }).click();
    await expect(app.main).toContainText("steam");
    await app.main.getByRole("tab", { name: "Ручные списки" }).click();
    await expect(app.main).toContainText("example.org");
  });

  test("an unreachable catalog is reported, the last verified one stays in use", async ({ app }) => {
    await app.open("s=running&cat=offline");
    await app.go("Приложения");
    await expect(app.main).toContainText("Нет связи с сервером каталога — используется последняя проверенная версия");
    await expect(app.main.getByRole("button", { name: "Discord: подробности" })).toBeVisible();
  });
});

test.describe("Diagnostics", () => {
  test("problems are listed with their reasons; a re-check asks the backend", async ({ app }) => {
    await app.open();
    await app.go("Диагностика");
    await expect(app.main).toContainText("Нужно внимание: 2");
    await expect(app.main).toContainText("updates.discord.com");
    await app.calls();
    await app.main.getByRole("button", { name: "Проверить снова" }).click();
    await app.sent("run_diagnostics");
  });

  test("TCP timestamps are changed only after an explicit confirmation", async ({ app, page }) => {
    await app.open();
    await app.go("Диагностика");
    await app.calls();
    await app.main.getByRole("button", { name: "Включить TCP timestamps…" }).click();
    await expect(page.getByRole("dialog")).toBeVisible();
    expect((await app.calls()).map((c) => c.cmd)).not.toContain("set_tcp_timestamps");
  });
});

test.describe("Settings", () => {
  test("a switch saves the changed value at once", async ({ app }) => {
    await app.open();
    await app.go("Настройки");
    const sw = app.main.getByRole("switch", { name: "auto discord" });
    await expect(sw).not.toBeChecked();
    await app.calls();
    await sw.click();
    const save = await app.sent("save_settings");
    expect((save.args.settings as { autoLaunchDiscord: boolean }).autoLaunchDiscord).toBe(true);
  });

  test("the Flowseal check row appears only when a newer version exists", async ({ app }) => {
    await app.open("s=running&readme=1");
    await app.go("Настройки");
    await app.main.getByRole("tab", { name: "Обновления", exact: true }).click();
    await expect(app.main).toContainText("Последняя официальная");
    await expect(app.main).not.toContainText("Проверка для Bridge");
    await expect(app.main).not.toContainText("манифест");

    await app.open("s=running");
    await app.go("Настройки");
    await app.main.getByRole("tab", { name: "Обновления", exact: true }).click();
    await expect(app.main).toContainText("Проверка для Bridge");
    await expect(app.main).toContainText("ещё не проверен для Bridge");
  });

  test("all settings tabs open", async ({ app }) => {
    await app.open();
    await app.go("Настройки");
    for (const tab of ["Обход", "Telegram Proxy", "Обновления", "Основные"]) {
      await app.main.getByRole("tab", { name: tab, exact: true }).click();
      await expect(app.main.getByRole("tab", { name: tab, exact: true })).toHaveAttribute("aria-selected", "true");
    }
  });
});

test("About shows the versions of every component", async ({ app }) => {
  await app.open();
  await app.go("О программе");
  await expect(app.main).toContainText("Flowseal 1.10.4");
  await expect(app.main).toContainText("winws v72.9");
});
