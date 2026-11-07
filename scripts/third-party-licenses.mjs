#!/usr/bin/env node
// License notices of the Rust crates compiled into discord-bridge.exe and
// discord-bridge-helper.exe and of the npm packages bundled into the
// interface (embedded in discord-bridge.exe).
//
// MIT, BSD, ISC and Apache-2.0 require their copyright and license notices
// to accompany binary copies. This script collects the license files that
// each package publishes and writes them next to the installer resources:
//
//   resources/third-party/THIRD_PARTY_LICENSES.txt   notices, verbatim
//   resources/third-party/packages.json              name, version, license, source
//
// It fails when a package has no license expression or when an expression
// has no branch made only of the licenses below (no copyleft creeps into the
// app binaries unnoticed). Components shipped as separate files (Flowseal,
// zapret, WinDivert, Cygwin, the Telegram WS Proxy engine) are documented in
// THIRD_PARTY_NOTICES.md and licenses/ instead.
//
// Inputs: `cargo metadata` for the Windows target, and
// target/third-party/npm-bundled.json written by the vite build
// (vite.config.ts). Run after `pnpm build`.

import { execFileSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const OUT = path.join(root, "resources", "third-party");
const NPM_LIST = path.join(root, "target", "third-party", "npm-bundled.json");
const TARGET = "x86_64-pc-windows-msvc";
const SHIPPED_BINARIES = ["discord-bridge", "discord-bridge-helper"];

// Permissive licenses accepted for code compiled into the app binaries.
// MPL-2.0 is file-level copyleft: allowed, its source is public on crates.io.
export const ALLOWED = new Set([
  "MIT", "MIT-0", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause",
  "ISC", "Zlib", "0BSD", "CC0-1.0", "Unlicense", "BSL-1.0", "Unicode-3.0", "Unicode-DFS-2016",
  "MPL-2.0", "CDLA-Permissive-2.0",
]);

const LICENSE_FILE = /^(licen[cs]e|copying|notice|copyright|unlicense|authors)([-._ ].*)?$/i;

// --- SPDX expression: true if some OR-branch uses only allowed licenses ----
export function spdxAllowed(expr, allowed = ALLOWED) {
  const tokens = String(expr).replace(/\//g, " OR ").match(/\(|\)|[^\s()]+/g) ?? [];
  let i = 0;
  const atom = () => {
    if (tokens[i] === "(") {
      i++;
      const v = or();
      if (tokens[i++] !== ")") throw new Error("unbalanced");
      return v;
    }
    let id = tokens[i++];
    if (!id || ["AND", "OR", "WITH", ")"].includes(id)) throw new Error("bad expression");
    if (tokens[i] === "WITH") {
      id = `${id} WITH ${tokens[i + 1]}`;
      i += 2;
    }
    return allowed.has(id.replace(/\+$/, ""));
  };
  const and = () => {
    let v = atom();
    while (tokens[i] === "AND") { i++; v = atom() && v; }
    return v;
  };
  const or = () => {
    let v = and();
    while (tokens[i] === "OR") { i++; v = and() || v; }
    return v;
  };
  try {
    const v = or();
    return i === tokens.length && v;
  } catch {
    return false;
  }
}

function licenseFiles(dir, extra) {
  const names = new Set();
  try {
    for (const n of fs.readdirSync(dir)) {
      if (LICENSE_FILE.test(n) && fs.statSync(path.join(dir, n)).isFile()) names.add(n);
    }
  } catch { /* unreadable: reported as missing */ }
  if (extra) names.add(path.relative(dir, path.resolve(dir, extra)));
  return [...names].sort().map((n) => ({ name: n, text: fs.readFileSync(path.join(dir, n), "utf8") }));
}

function rustPackages() {
  const meta = JSON.parse(
    execFileSync("cargo", ["metadata", "--format-version", "1", "--locked", "--filter-platform", TARGET], {
      cwd: root,
      maxBuffer: 256 * 1024 * 1024,
    }).toString(),
  );
  const byId = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const members = new Set(meta.workspace_members);
  const start = meta.packages.filter((p) => members.has(p.id) && SHIPPED_BINARIES.includes(p.name)).map((p) => p.id);
  if (start.length !== SHIPPED_BINARIES.length) throw new Error(`workspace binaries not found: ${SHIPPED_BINARIES}`);
  // Normal (non-dev, non-build) dependencies only: what is compiled into the binaries.
  const seen = new Set();
  const stack = [...start];
  while (stack.length) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const d of nodes.get(id)?.deps ?? []) {
      if (d.dep_kinds.some((k) => k.kind === null)) stack.push(d.pkg);
    }
  }
  return [...seen]
    .filter((id) => !members.has(id))
    .map((id) => byId.get(id))
    .map((p) => {
      const dir = path.dirname(p.manifest_path);
      return {
        ecosystem: "cargo",
        name: p.name,
        version: p.version,
        license: p.license ?? null,
        // The exact published source of this version (MPL-2.0 asks to say where it is).
        source: `https://crates.io/crates/${p.name}/${p.version}` + (p.repository ? ` (repository: ${p.repository})` : ""),
        files: licenseFiles(dir, p.license_file),
      };
    });
}

function npmPackages() {
  if (!fs.existsSync(NPM_LIST)) throw new Error(`${path.relative(root, NPM_LIST)} is missing: run "pnpm build" first`);
  const dirs = JSON.parse(fs.readFileSync(NPM_LIST, "utf8"));
  // Tailwind CSS generates the stylesheet (including its preflight reset).
  dirs.push(path.join(root, "node_modules", "tailwindcss"));
  return dirs.map((d) => {
    const dir = fs.realpathSync(d);
    const pkg = JSON.parse(fs.readFileSync(path.join(dir, "package.json"), "utf8"));
    const license = typeof pkg.license === "string" ? pkg.license : pkg.license?.type ?? null;
    const repo = typeof pkg.repository === "string" ? pkg.repository : pkg.repository?.url;
    return {
      ecosystem: "npm",
      name: pkg.name,
      version: pkg.version,
      license,
      source: repo ?? pkg.homepage ?? `https://www.npmjs.com/package/${pkg.name}/v/${pkg.version}`,
      files: licenseFiles(dir),
    };
  });
}

function main() {
  const pkgs = [...rustPackages(), ...npmPackages()].sort(
    (a, b) => a.ecosystem.localeCompare(b.ecosystem) || a.name.localeCompare(b.name) || a.version.localeCompare(b.version),
  );
  const errors = [];
  for (const p of pkgs) {
    if (!p.license) errors.push(`${p.ecosystem} ${p.name} ${p.version}: no license expression`);
    else if (!spdxAllowed(p.license)) errors.push(`${p.ecosystem} ${p.name} ${p.version}: license "${p.license}" is not on the allowed list`);
  }
  if (errors.length) {
    console.error(errors.map((e) => `ERROR ${e}`).join("\n"));
    process.exit(1);
  }

  // Identical texts (e.g. the Apache-2.0 license) are printed once.
  const texts = new Map();
  const norm = (t) => t.replace(/\s+/g, " ").trim();
  const out = [
    "Bridge — notices of third-party packages compiled into discord-bridge.exe",
    "and discord-bridge-helper.exe (Rust crates) and bundled into the interface",
    "(npm packages). Generated by scripts/third-party-licenses.mjs at build time.",
    "Components shipped as separate files are listed in THIRD_PARTY_NOTICES.md.",
    "",
  ];
  const index = [];
  let missing = 0;
  for (const p of pkgs) {
    out.push("=".repeat(78), `${p.name} ${p.version} (${p.ecosystem})`, `License: ${p.license}`, `Source: ${p.source}`);
    if (!p.files.length) {
      missing++;
      out.push(`The published package contains no license file; it is distributed under ${p.license} (see the source above).`);
    }
    for (const f of p.files) {
      const h = crypto.createHash("sha256").update(norm(f.text)).digest("hex").slice(0, 12);
      if (texts.has(h)) {
        out.push(`--- ${f.name}: identical to the text [${h}] printed for ${texts.get(h)}`);
      } else {
        texts.set(h, `${p.name} ${p.version}`);
        out.push(`--- ${f.name} [${h}]`, f.text.replace(/\r\n/g, "\n").trimEnd());
      }
    }
    out.push("");
    index.push({ ecosystem: p.ecosystem, name: p.name, version: p.version, license: p.license, source: p.source, licenseFiles: p.files.map((f) => f.name) });
  }
  fs.mkdirSync(OUT, { recursive: true });
  fs.writeFileSync(path.join(OUT, "THIRD_PARTY_LICENSES.txt"), out.join("\n") + "\n");
  fs.writeFileSync(path.join(OUT, "packages.json"), JSON.stringify({ packages: index }, null, 2) + "\n");
  const n = (e) => pkgs.filter((p) => p.ecosystem === e).length;
  console.log(`ok: ${n("cargo")} crates, ${n("npm")} npm packages, ${missing} without a license file in the package; ${path.relative(root, OUT)}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
