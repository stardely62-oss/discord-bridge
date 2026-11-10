// UI tests against the preview build: the real React app with the mocked
// backend from scripts/preview (no Tauri, no Windows, no network).
// Needs the generated preview data — see docs/maintainers/building.md, "Тесты интерфейса".
import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: process.env.CI ? [["list"], ["github"]] : "list",
  use: {
    ...devices["Desktop Chrome"],
    baseURL: "http://localhost:1430",
    viewport: { width: 1100, height: 760 },
    trace: "off",
  },
  webServer: {
    command: "pnpm vite --config scripts/preview/vite.config.ts",
    url: "http://localhost:1430",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
