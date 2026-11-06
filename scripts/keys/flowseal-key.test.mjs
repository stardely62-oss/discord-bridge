import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { check, generate, parseSeed, publicOf } from "./flowseal-key.mjs";

const tool = fileURLToPath(new URL("./flowseal-key.mjs", import.meta.url));
const tmp = () => fs.mkdtempSync(path.join(os.tmpdir(), "flowseal-key-"));

test("generate writes only the seed, prints only the public key, and check matches", () => {
  const d = tmp();
  try {
    const seedFile = path.join(d, "seed.txt");
    const out = execFileSync(process.execPath, [tool, "generate", "--seed-file", seedFile], { encoding: "utf8" });
    const seedText = fs.readFileSync(seedFile, "utf8").trim();
    assert.ok(!out.includes(seedText), "the seed must never be printed");
    const pub = out.match(/public key: (\S+)/)[1];
    assert.equal(publicOf(parseSeed(seedText)).toString("base64"), pub);
    if (process.platform !== "win32") assert.equal(fs.statSync(seedFile).mode & 0o077, 0, "owner-only permissions");
    const pubFile = path.join(d, "pub.b64");
    fs.writeFileSync(pubFile, pub + "\n");
    const ok = spawnSync(process.execPath, [tool, "check", "--seed-file", seedFile, "--pubkey", pubFile], { encoding: "utf8" });
    assert.equal(ok.status, 0);
    assert.match(ok.stdout, /^match/);
    assert.ok(!ok.stdout.includes(seedText) && !ok.stderr.includes(seedText));
  } finally {
    fs.rmSync(d, { recursive: true, force: true });
  }
});

test("generate refuses to overwrite an existing key file", () => {
  const d = tmp();
  try {
    const seedFile = path.join(d, "seed.txt");
    generate(seedFile);
    const before = fs.readFileSync(seedFile);
    assert.throws(() => generate(seedFile), /refusing to overwrite/);
    assert.deepEqual(fs.readFileSync(seedFile), before);
  } finally {
    fs.rmSync(d, { recursive: true, force: true });
  }
});

test("check reports a seed that does not belong to the pinned key", () => {
  const d = tmp();
  try {
    const a = path.join(d, "a.txt");
    const b = path.join(d, "b.txt");
    const pubA = generate(a).publicKey;
    generate(b);
    const pubFile = path.join(d, "pub.b64");
    fs.writeFileSync(pubFile, pubA);
    assert.equal(check(a, pubFile).match, true);
    assert.equal(check(b, pubFile).match, false);
    const r = spawnSync(process.execPath, [tool, "check", "--seed-file", b, "--pubkey", pubFile], { encoding: "utf8" });
    assert.equal(r.status, 1);
    assert.match(r.stdout, /MISMATCH/);
  } finally {
    fs.rmSync(d, { recursive: true, force: true });
  }
});

test("malformed seeds are rejected", () => {
  assert.throws(() => parseSeed("not base64"), /one base64 line/);
  assert.throws(() => parseSeed(Buffer.alloc(31).toString("base64")), /one base64 line/);
  assert.throws(() => publicOf(Buffer.alloc(16)), /exactly 32 bytes/);
});

test("the pinned public key file holds a valid 32-byte key", () => {
  const pinned = Buffer.from(fs.readFileSync("keys/flowseal-manifest-public-key.b64", "utf8").trim(), "base64");
  assert.equal(pinned.length, 32);
});
