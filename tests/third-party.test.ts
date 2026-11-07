import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";

const root = path.resolve(__dirname, "..");
const read = (p: string) => fs.readFileSync(path.join(root, p), "utf8");

interface Component {
  name: string;
  file: string;
  sha256: string;
  version: string;
  license: string;
  licenseFiles: string[];
  source: { repository: string; tag: string; commit: string; archive: string };
}
const sources = JSON.parse(read("licenses/third-party-sources.json")) as { flowsealVersion: string; components: Component[] };

describe("third-party documentation", () => {
  it("documents the pinned Flowseal release", () => {
    expect(sources.flowsealVersion).toBe(JSON.parse(read("resources/flowseal.lock.json")).version);
  });

  it("lists every engine binary with its hash, license files and exact source", () => {
    const notices = read("THIRD_PARTY_NOTICES.md");
    expect(sources.components.map((c) => c.file).sort()).toEqual(["bin/WinDivert.dll", "bin/WinDivert64.sys", "bin/cygwin1.dll", "bin/winws.exe"]);
    for (const c of sources.components) {
      expect(c.sha256, c.file).toMatch(/^[0-9a-f]{64}$/);
      expect(c.source.commit, c.file).toMatch(/^[0-9a-f]{40}$/);
      expect(c.source.archive, c.file).toMatch(/^https:\/\//);
      expect(notices, `${c.file} hash`).toContain(c.sha256);
      expect(notices, `${c.file} source commit`).toContain(c.source.commit);
      for (const f of c.licenseFiles) expect(fs.existsSync(path.join(root, f)), f).toBe(true);
    }
  });

  it("points to the copy of the LGPL sources kept in this repository", () => {
    const copies = (JSON.parse(read("licenses/third-party-sources.json")) as { sourceCopies: { release: string; files: Record<string, string> } }).sourceCopies;
    expect(copies.release).toBe(`https://github.com/stardely62-oss/discord-bridge/releases/tag/sources-flowseal-${sources.flowsealVersion}`);
    expect(Object.keys(copies.files).sort()).toEqual(["WinDivert-2.2.2-source.tar.gz", "cygwin-3.4.10-source.tar.gz", "zapret-v72.9-source.tar.gz"]);
    for (const h of Object.values(copies.files)) expect(h).toMatch(/^[0-9a-f]{64}$/);
    expect(read("THIRD_PARTY_NOTICES.md")).toContain(copies.release);
    expect(read("licenses/Cygwin-LICENSE.txt")).toContain(copies.release);
  });

  it("ships the LGPL texts and the Cygwin linking exception", () => {
    for (const f of ["LGPL-3.0.txt", "GPL-3.0.txt", "GPL-2.0.txt", "WinDivert-LICENSE.txt", "Cygwin-LICENSE.txt", "zapret-LICENSE.txt", "Flowseal-LICENSE.txt"]) {
      expect(fs.existsSync(path.join(root, "licenses", f)), f).toBe(true);
    }
    expect(read("licenses/Cygwin-LICENSE.txt")).toContain("Cygwin(TM) Linking Exception");
  });
});

describe("README download links", () => {
  const readme = read("README.md");

  it("never sends users to releases/latest of the app repository (a Flowseal manifest release can be marked latest)", () => {
    expect(readme).not.toMatch(/stardely62-oss\/discord-bridge\/releases\/latest/);
  });

  it("points to the installer and SHA256SUMS.txt of one released app version", () => {
    const links = [...readme.matchAll(/https:\/\/github\.com\/stardely62-oss\/discord-bridge\/releases\/download\/([^/)\s]+)\/([^)\s]+)/g)];
    expect(links.length).toBeGreaterThanOrEqual(2);
    const tags = new Set(links.map((m) => m[1]));
    expect(tags.size).toBe(1);
    const tag = [...tags][0];
    expect(tag).toMatch(/^v\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$/);
    const version = tag.slice(1);
    expect(links.map((m) => m[2]).sort()).toEqual([`Bridge_${version}_x64-setup.exe`, "SHA256SUMS.txt"].sort());
    // Only a released version: its CHANGELOG section has a date.
    expect(read("CHANGELOG.md")).toMatch(new RegExp(`^## \\[${version.replace(/\./g, "\\.")}\\] — \\d{4}-\\d{2}-\\d{2}`, "m"));
    expect(readme).toContain(`releases/tag/${tag}`);
  });
});
