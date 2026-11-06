// Downloads the pinned Flowseal release from the official GitHub release,
// verifies its SHA-256 (resources/flowseal.lock.json) and unpacks it into
// resources/flowseal/<version>/ with a bundle-manifest.json.
// Usage: node scripts/fetch-flowseal.mjs
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";

const root = new URL("..", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const lock = JSON.parse(readFileSync(join(root, "resources/flowseal.lock.json"), "utf8"));
if (!lock.url.startsWith("https://github.com/Flowseal/zapret-discord-youtube/releases/download/")) {
  throw new Error("refusing to download from a non-official source");
}
const cache = join(root, "target", "flowseal-cache");
mkdirSync(cache, { recursive: true });
const zipPath = join(cache, `zapret-discord-youtube-${lock.version}.zip`);

if (!existsSync(zipPath)) {
  console.log(`downloading ${lock.url}`);
  const res = await fetch(lock.url, { redirect: "follow" });
  if (!res.ok) throw new Error(`download failed: ${res.status}`);
  writeFileSync(zipPath, Buffer.from(await res.arrayBuffer()));
}
const hash = createHash("sha256").update(readFileSync(zipPath)).digest("hex");
if (hash !== lock.sha256) throw new Error(`SHA-256 mismatch: ${hash} != ${lock.sha256}`);
console.log(`verified ${zipPath}`);

const r = spawnSync("cargo", ["run", "-q", "-p", "bridge-tools", "--", "bundle", join(root, "resources/flowseal.lock.json"), zipPath, join(root, "resources/flowseal")], {
  stdio: "inherit",
  cwd: root,
});
process.exit(r.status ?? 1);
