import { expect, test as base, type Page } from "@playwright/test";

export type Call = { cmd: string; args: Record<string, unknown> };

export class App {
  readonly errors: string[] = [];
  constructor(readonly page: Page) {
    page.on("pageerror", (e) => this.errors.push(e.message));
    page.on("console", (m) => {
      if (m.type() === "error") this.errors.push(m.text());
    });
  }

  /** Opens the preview in a scenario, e.g. `s=running&tg=connected`. */
  async open(query = "s=running") {
    await this.page.goto(`/?${query}`);
    await expect(this.nav).toBeVisible();
    await expect(this.main.getByRole("heading", { level: 1 })).toBeVisible();
  }

  get nav() {
    return this.page.getByRole("navigation");
  }
  get main() {
    return this.page.getByRole("main");
  }

  async go(section: string) {
    await this.nav.getByRole("button", { name: section, exact: true }).click();
  }

  /** Commands sent to the backend since the last call of this method. */
  async calls(): Promise<Call[]> {
    return this.page.evaluate(() => (window as unknown as { __bridgeCalls: Call[] }).__bridgeCalls.splice(0));
  }

  /** Waits until the UI has sent `cmd` and returns that call. */
  async sent(cmd: string): Promise<Call> {
    const seen: Call[] = [];
    await expect
      .poll(async () => {
        seen.push(...(await this.calls()));
        return seen.map((c) => c.cmd);
      })
      .toContain(cmd);
    return seen.find((c) => c.cmd === cmd)!;
  }
}

export const test = base.extend<{ app: App }>({
  app: async ({ page }, provide) => {
    const app = new App(page);
    await provide(app);
    expect(app.errors, "page errors and console errors").toEqual([]);
  },
});
export { expect };
