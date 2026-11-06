// UI preview with a mocked backend: pnpm vite --config scripts/preview/vite.config.ts
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

const here = (p: string) => fileURLToPath(new URL(p, import.meta.url));
export default defineConfig({
  root: here("../.."),
  plugins: [react()],
  resolve: {
    alias: {
      "@tauri-apps/api/core": here("./mock-core.ts"),
      "@tauri-apps/api/event": here("./mock-event.ts"),
      "@tauri-apps/plugin-dialog": here("./mock-dialog.ts"),
    },
  },
  build: { target: "es2022" },
  server: { port: 1430, strictPort: true },
});
