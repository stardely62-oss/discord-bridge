import { expect, test } from "./app";

test("«Применить» restarts the bypass with the active strategy", async ({ app }) => {
  await app.open();
  await expect(app.main).toContainText("Изменения ещё не применены");
  await app.calls();
  await app.main.getByRole("button", { name: "Применить" }).click();
  const call = await app.sent("bypass_enable");
  expect(call.args).toEqual({ id: "flowseal-general-alt2", ignoreConflicts: false });
});

test.describe("TCP timestamps reminder", () => {
  test("a start with a fooling=ts strategy asks to enable them, and changes nothing without consent", async ({ app, page }) => {
    await app.open();
    await app.main.getByRole("button", { name: "Применить" }).click();
    const dialog = page.getByRole("dialog");
    await expect(dialog).toContainText("TCP timestamps в Windows выключены");
    await app.calls();
    await dialog.getByRole("button", { name: "Не включать" }).click();
    expect((await app.calls()).map((c) => c.cmd)).not.toContain("set_tcp_timestamps");
    // A «no» is remembered: the next start does not ask again.
    await app.main.getByRole("button", { name: "Применить" }).click();
    await app.sent("bypass_enable");
    await expect(dialog).toHaveCount(0);
  });

  test("«Включить» turns them on with the user's confirmation", async ({ app, page }) => {
    await app.open();
    await app.main.getByRole("button", { name: "Применить" }).click();
    await page.getByRole("dialog").getByRole("button", { name: "Включить" }).click();
    expect((await app.sent("set_tcp_timestamps")).args).toEqual({ enable: true, userConfirmed: true });
  });

  test("nothing is asked when they are already on", async ({ app, page }) => {
    await app.open("s=running&ts=on");
    await app.main.getByRole("button", { name: "Применить" }).click();
    await app.sent("tcp_timestamps_needed");
    await expect(page.getByRole("dialog")).toHaveCount(0);
  });
});

test("«Запустить Discord» asks the backend to launch by the saved preference", async ({ app }) => {
  await app.open();
  await app.calls();
  await app.main.getByRole("button", { name: "Запустить Discord" }).click();
  expect((await app.sent("launch_discord")).args).toEqual({ mode: "preference" });
});

test("voice is never shown as working without a confirmation", async ({ app }) => {
  await app.open("s=running");
  await expect(app.main).toContainText("Голос не проверен");
  await app.open("s=stopped");
  await expect(app.main).toContainText("Голос не проверен");
  await expect(app.main.getByRole("group", { name: "Голосовые каналы Discord работают?" })).toHaveCount(0);
});

test("the voice question is asked while the bypass runs, and the answer is saved for the running strategy", async ({ app }) => {
  await app.open("s=running");
  const ask = app.main.getByRole("group", { name: "Голосовые каналы Discord работают?" });
  await expect(ask).toBeVisible();
  await app.calls();
  await ask.getByRole("button", { name: "Да" }).click();
  expect((await app.sent("confirm_voice")).args).toEqual({ strategyId: "flowseal-general-alt2", works: true });
});

test.describe("Telegram WS Proxy card", () => {
  test("start and stop go to the proxy only, not to the bypass", async ({ app }) => {
    await app.open("s=running");
    await expect(app.main).toContainText(/MTProto\s*выключен/);
    await app.calls();
    await app.main.getByRole("button", { name: "Запустить", exact: true }).click();
    await expect(app.main).toContainText("ждёт подключения");
    await app.main.getByRole("button", { name: "Остановить" }).click();
    await expect(app.main).toContainText(/MTProto\s*выключен/);
    const sent = (await app.calls()).map((c) => c.cmd);
    expect(sent).toContain("telegram_start");
    expect(sent).toContain("telegram_stop");
    expect(sent.filter((c) => c.startsWith("bypass_"))).toEqual([]);
  });

  test("an open port alone is not shown as a connected client", async ({ app }) => {
    await app.open("s=running&tg=listening");
    await app.main.getByRole("button", { name: "Ещё" }).last().click();
    await expect(app.main).toContainText(/Клиент\s*не подключён/);
    await expect(app.main).toContainText("127.0.0.1:1443");
  });

  test("a busy port is an error with its reason", async ({ app }) => {
    await app.open("s=running&tg=error");
    await expect(app.main).toContainText("порт 1443 уже занят другой программой");
  });

  test("a planned automatic restart is announced; a plain error is not", async ({ app }) => {
    await app.open("s=running&tg=error&tgr=1");
    await expect(app.main).toContainText("Bridge запустит его снова через несколько секунд");
    await app.open("s=running&tg=error");
    await expect(app.main).not.toContainText("запустит его снова");
  });

  test("a missing component offers no start button", async ({ app }) => {
    await app.open("s=running&tg=notInstalled");
    await expect(app.main.getByRole("button", { name: "Запустить", exact: true })).toHaveCount(0);
  });
});
