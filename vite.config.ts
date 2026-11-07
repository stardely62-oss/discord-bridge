import { defineConfig, type Plugin } from "vitest/config";
import react from "@vitejs/plugin-react";
import fs from "node:fs";
import path from "node:path";

/**
 * Records which npm packages end up in the production bundle (embedded in
 * discord-bridge.exe), so that scripts/third-party-licenses.mjs can ship
 * their license notices. Only packages with code in the output are listed;
 * build tools and type packages are not.
 */
function bundledPackages(): Plugin {
  return {
    name: "bridge-bundled-packages",
    apply: "build",
    generateBundle() {
      const roots = new Set<string>();
      for (const id of this.getModuleIds()) {
        const file = id.replace(/^\0/, "").split("?")[0].replace(/\\/g, "/");
        const i = file.lastIndexOf("/node_modules/");
        if (i < 0 || !this.getModuleInfo(id)?.isIncluded) continue;
        const rest = file.slice(i + "/node_modules/".length).split("/");
        const name = rest[0].startsWith("@") ? `${rest[0]}/${rest[1]}` : rest[0];
        roots.add(file.slice(0, i + "/node_modules/".length) + name);
      }
      const out = path.resolve(__dirname, "target/third-party");
      fs.mkdirSync(out, { recursive: true });
      fs.writeFileSync(path.join(out, "npm-bundled.json"), JSON.stringify([...roots].sort(), null, 2));
    },
  };
}

export default defineConfig({
  plugins: [react(), bundledPackages()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**", "**/crates/**"] } },
  build: {
    target: "es2021",
    chunkSizeWarningLimit: 1500,
    sourcemap: false,
  },
  test: { environment: "node", include: ["tests/**/*.test.ts"] },
});
