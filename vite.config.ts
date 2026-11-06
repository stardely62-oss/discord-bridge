import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**", "**/crates/**"] } },
  build: {
    target: "es2021",
    chunkSizeWarningLimit: 1500,
    sourcemap: false,
  },
  test: { environment: "node", include: ["tests/**/*.test.ts"] },
});
