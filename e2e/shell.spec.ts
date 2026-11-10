import { expect, test } from "./app";

const sections: [string, string | RegExp][] = [
  ["Стратегии", "Стратегии"],
  ["Приложения", "Приложения и списки"],
  ["Диагностика", "Диагностика"],
  ["Настройки", "Настройки"],
  ["О программе", "О программе"],
  ["Главная", /Discord/],
];

test("every section opens without errors", async ({ app }) => {
  await app.open();
  for (const [section, heading] of sections) {
    await app.go(section);
    await expect(app.main.getByRole("heading", { level: 1, name: heading })).toBeVisible();
  }
});

test("the sidebar shows the real engine state", async ({ app }) => {
  await app.open("s=running");
  await expect(app.nav).toContainText("Обход включён");
  await expect(app.nav).toContainText("general (ALT2)");
  await app.open("s=stopped");
  await expect(app.nav).toContainText("Обход выключен");
  await expect(app.nav).not.toContainText("general (ALT2)");
});

test("English interface", async ({ app }) => {
  await app.open("s=running&l=en");
  for (const name of ["Home", "Strategies", "Apps", "Diagnostics", "Settings", "About"]) {
    await expect(app.nav.getByRole("button", { name, exact: true })).toBeVisible();
  }
  await expect(app.nav).toContainText("Bypass on");
});

test("light and dark themes are applied to the document", async ({ app, page }) => {
  await app.open("s=running&t=light");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await app.open("s=running&t=dark");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
});

test("update notices sit below the page and never cover it", async ({ app, page }) => {
  for (const size of [
    { width: 1100, height: 760 },
    { width: 880, height: 640 },
  ]) {
    await page.setViewportSize(size);
    await app.open();
    const notices = page.getByRole("status");
    await expect(notices).toHaveCount(2);
    const main = (await app.main.boundingBox())!;
    for (const n of await notices.all()) {
      const box = (await n.boundingBox())!;
      expect(box.y, "notice starts below the page").toBeGreaterThanOrEqual(main.y + main.height - 1);
      expect(box.y + box.height, "notice is inside the window").toBeLessThanOrEqual(size.height + 1);
    }
    // The control the floating notices used to cover in a small window.
    await app.main.getByRole("button", { name: "Ещё" }).last().click({ trial: true });
  }
});

test("«Позже» hides a notice for the session", async ({ app, page }) => {
  await app.open();
  await expect(page.getByRole("status")).toHaveCount(2);
  await page.getByRole("status").first().getByRole("button", { name: "Позже" }).click();
  await expect(page.getByRole("status")).toHaveCount(1);
});

test("onboarding: «Настрою сам» saves the choice and closes", async ({ app, page }) => {
  await app.open("s=onboarding");
  const dialog = page.getByRole("dialog", { name: "Добро пожаловать в Bridge" });
  await expect(dialog).toBeVisible();
  await app.calls();
  await dialog.getByRole("button", { name: "Настрою сам" }).click();
  const save = await app.sent("save_settings");
  expect((save.args.settings as { onboardingDone: boolean }).onboardingDone).toBe(true);
  await expect(dialog).toBeHidden();
});

test.describe("installing a Bridge update from the app", () => {
  test("without a verified signed manifest only the release page is offered", async ({ app, page }) => {
    await app.open();
    const notice = page.getByRole("status").filter({ hasText: "Доступна новая версия Bridge" });
    await expect(notice).toContainText("Установка вручную");
    await expect(notice.getByRole("button", { name: "Установить" })).toHaveCount(0);
    await app.go("Настройки");
    await app.main.getByRole("tab", { name: "Обновления", exact: true }).click();
    await expect(app.main.getByRole("button", { name: /Скачать и установить/ })).toHaveCount(0);
    await expect(app.main.getByRole("button", { name: /Открыть релиз/ })).toBeVisible();
  });

  test("a verified release is installed only after the confirmation", async ({ app, page }) => {
    await app.open("s=running&upd=signed");
    const notice = page.getByRole("status").filter({ hasText: "Доступна новая версия Bridge" });
    await expect(notice).toContainText("Подпись релиза проверена");
    await app.calls();
    await notice.getByRole("button", { name: "Установить" }).click();
    const dialog = page.getByRole("dialog", { name: "Установить Bridge 0.1.0-alpha.11?" });
    await expect(dialog).toContainText("Windows спросит разрешение администратора");
    await dialog.getByRole("button", { name: "Отмена" }).click();
    await expect(dialog).toBeHidden();
    expect((await app.calls()).map((c) => c.cmd)).not.toContain("app_update_install");

    await notice.getByRole("button", { name: "Установить" }).click();
    await dialog.getByRole("button", { name: "Скачать и установить" }).click();
    // The interface names nothing to download: the backend uses the manifest it verified.
    expect((await app.sent("app_update_install")).args).toEqual({});
  });

  test("the same action is in Settings and About", async ({ app }) => {
    await app.open("s=running&upd=signed");
    await app.go("Настройки");
    await app.main.getByRole("tab", { name: "Обновления", exact: true }).click();
    await expect(app.main.getByRole("button", { name: "Скачать и установить 0.1.0-alpha.11" })).toBeVisible();
    await app.go("О программе");
    await expect(app.main.getByRole("button", { name: "Установить", exact: true })).toBeVisible();
  });
});
