// node --test scripts/sign/sign-ed25519.test.mjs — one-time keys generated in
// memory for each test; nothing is published and no real key is involved.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash, createPublicKey, generateKeyPairSync, verify } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";

const SIGNER = path.resolve("scripts/sign/sign-ed25519.mjs");
const payload = Buffer.from(JSON.stringify({ format: 1, flowsealVersion: "1.2.3", sourceUrl: "https://github.com/Flowseal/zapret-discord-youtube/releases/download/1.2.3/zapret-discord-youtube-1.2.3.zip", sourceSha256: "a".repeat(64), files: [{ path: "bin/winws.exe", sha256: "b".repeat(64), size: 1 }] }));

function setup() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "signer-"));
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const seed = privateKey.export({ format: "der", type: "pkcs8" }).subarray(-32).toString("base64");
  const pub = publicKey.export({ format: "der", type: "spki" }).subarray(-32).toString("base64");
  fs.writeFileSync(path.join(dir, "payload.json"), payload);
  fs.writeFileSync(path.join(dir, "pub.b64"), pub + "\n");
  return { dir, seed, pub, publicKey };
}
const run = (s, extra = [], env = {}) =>
  spawnSync(process.execPath, [SIGNER, "--payload", path.join(s.dir, "payload.json"), "--out", path.join(s.dir, "out.json"), "--pubkey", path.join(s.dir, "pub.b64"), "--key-env", "K", ...extra], { env: { PATH: process.env.PATH, K: s.seed, ...env }, encoding: "utf8" });
const sha = createHash("sha256").update(payload).digest("hex");
const flow = ["--expect-version", "1.2.3", "--expect-source-sha256", "a".repeat(64)];

test("signs exactly the expected payload; the output verifies and holds no key material", () => {
  const s = setup();
  const r = run(s, ["--expect-sha256", sha, ...flow]);
  assert.equal(r.status, 0, r.stderr);
  const env = JSON.parse(fs.readFileSync(path.join(s.dir, "out.json"), "utf8"));
  assert.deepEqual(Object.keys(env).sort(), ["keyId", "manifest", "signature"]);
  assert.ok(Buffer.from(env.manifest, "base64").equals(payload));
  assert.ok(verify(null, payload, s.publicKey, Buffer.from(env.signature, "base64")));
  const all = fs.readFileSync(path.join(s.dir, "out.json"), "utf8") + r.stdout + r.stderr;
  assert.ok(!all.includes(s.seed), "seed never printed or written");
  assert.deepEqual(fs.readdirSync(s.dir).sort(), ["out.json", "payload.json", "pub.b64"], "no temporary files");
});

test("refuses a payload other than the one the build job reported", () => {
  const s = setup();
  const r = run(s, ["--expect-sha256", "0".repeat(64), ...flow]);
  assert.notEqual(r.status, 0);
  assert.match(r.stderr, /not the one the build job produced/);
  assert.ok(!fs.existsSync(path.join(s.dir, "out.json")));
});

test("refuses a key that is not the pinned one, a missing key, and wrong Flowseal inputs", () => {
  const s = setup();
  const other = setup();
  let r = run(s, ["--expect-sha256", sha, ...flow], { K: other.seed });
  assert.match(r.stderr, /does not match the pinned public key/);
  assert.ok(!r.stderr.includes(other.seed));
  r = run(s, ["--expect-sha256", sha, ...flow], { K: "" });
  assert.match(r.stderr, /K is not set/);
  r = run(s, ["--expect-sha256", sha, "--expect-version", "9.9.9", "--expect-source-sha256", "a".repeat(64)]);
  assert.match(r.stderr, /requested Flowseal version/);
  r = run(s, ["--expect-sha256", sha, "--expect-version", "1.2.3", "--expect-source-sha256", "c".repeat(64)]);
  assert.match(r.stderr, /reviewed archive/);
});

test("app release: signs only the reviewed version and installer, never mixed with Flowseal options", () => {
  const s = setup();
  const app = Buffer.from(JSON.stringify({ kind: "bridge-app-release", format: 1, version: "0.2.0", installerName: "Bridge_0.2.0_x64-setup.exe", installerSize: 20000000, installerSha256: "c".repeat(64) }));
  fs.writeFileSync(path.join(s.dir, "payload.json"), app);
  const h = createHash("sha256").update(app).digest("hex");
  const ok = ["--expect-sha256", h, "--expect-app-version", "0.2.0", "--expect-installer-sha256", "c".repeat(64)];
  const r = run(s, ok);
  assert.equal(r.status, 0, r.stderr);
  const env = JSON.parse(fs.readFileSync(path.join(s.dir, "out.json"), "utf8"));
  assert.ok(verify(null, app, s.publicKey, Buffer.from(env.signature, "base64")));
  fs.rmSync(path.join(s.dir, "out.json"));
  for (const bad of [
    ["--expect-sha256", h, "--expect-app-version", "0.2.1", "--expect-installer-sha256", "c".repeat(64)],
    ["--expect-sha256", h, "--expect-app-version", "0.2.0", "--expect-installer-sha256", "d".repeat(64)],
    ["--expect-sha256", h, "--expect-app-version", "0.2.0"],
    ["--expect-sha256", h],
    [...ok, ...flow],
    ["--expect-sha256", h, ...flow],
  ]) {
    assert.notEqual(run(s, bad).status, 0, bad.join(" "));
    assert.ok(!fs.existsSync(path.join(s.dir, "out.json")));
  }
  // A Flowseal payload is not signed as an app release, and the other way round.
  fs.writeFileSync(path.join(s.dir, "payload.json"), payload);
  assert.notEqual(run(s, ["--expect-sha256", sha, "--expect-app-version", "0.2.0", "--expect-installer-sha256", "c".repeat(64)]).status, 0);
});
