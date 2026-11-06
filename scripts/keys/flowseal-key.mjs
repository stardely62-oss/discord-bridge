// Flowseal manifest key: generate a new Ed25519 pair on the owner's trusted
// computer, or check that a stored seed matches the pinned public key.
// Dependency-free (Node's built-in crypto). The seed is never printed.
//
//   node scripts/keys/flowseal-key.mjs generate --seed-file <path>
//     Writes the 32-byte seed (base64, one line) to a NEW file (refuses to
//     overwrite) with owner-only permissions where the OS supports them, and
//     prints only the public key and its key id.
//
//   node scripts/keys/flowseal-key.mjs check --seed-file <path> [--pubkey keys/flowseal-manifest-public-key.b64]
//     Derives the public key from the seed and compares it with the pinned
//     one. Prints only "match" or "MISMATCH" (exit code 0 / 1).
import { createPrivateKey, createPublicKey, generateKeyPairSync, sign, verify } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const PKCS8_PREFIX = Buffer.from("302e020100300506032b657004220420", "hex");
const SPKI_PREFIX = Buffer.from("302a300506032b6570032100", "hex");

export const keyId = (pub) => pub.subarray(0, 8).toString("hex");

/** Public key (32 bytes) of a 32-byte seed; throws on a malformed seed. */
export function publicOf(seed) {
  if (seed.length !== 32) throw new Error("the seed must be exactly 32 bytes");
  const key = createPrivateKey({ key: Buffer.concat([PKCS8_PREFIX, seed]), format: "der", type: "pkcs8" });
  const pub = createPublicKey(key).export({ format: "der", type: "spki" }).subarray(-32);
  // Sign/verify round trip, so a broken seed is never accepted.
  const probe = Buffer.from("discord-bridge flowseal key check");
  const spki = createPublicKey({ key: Buffer.concat([SPKI_PREFIX, pub]), format: "der", type: "spki" });
  if (!verify(null, probe, spki, sign(null, probe, key))) throw new Error("self-check failed");
  return pub;
}

/** Decodes a base64 seed strictly (no whitespace inside, exactly 32 bytes). */
export function parseSeed(text) {
  const t = text.trim();
  if (!/^[A-Za-z0-9+/]{43}=$/.test(t)) throw new Error("the seed file must hold one base64 line of 32 bytes");
  return Buffer.from(t, "base64");
}

export function generate(seedFile) {
  if (fs.existsSync(seedFile)) throw new Error(`${seedFile} already exists — refusing to overwrite a key`);
  const { privateKey } = generateKeyPairSync("ed25519");
  const seed = privateKey.export({ format: "der", type: "pkcs8" }).subarray(-32);
  const pub = publicOf(seed);
  fs.writeFileSync(seedFile, seed.toString("base64") + "\n", { flag: "wx", mode: 0o600 });
  seed.fill(0);
  return { publicKey: pub.toString("base64"), keyId: keyId(pub) };
}

export function check(seedFile, pubFile) {
  const seed = parseSeed(fs.readFileSync(seedFile, "utf8"));
  const pinned = Buffer.from(fs.readFileSync(pubFile, "utf8").trim(), "base64");
  if (pinned.length !== 32) throw new Error("the pinned public key must be 32 bytes");
  const pub = publicOf(seed);
  seed.fill(0);
  return { match: pub.equals(pinned), keyId: keyId(pub), pinnedKeyId: keyId(pinned) };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [cmd, ...a] = process.argv.slice(2);
  const opt = (k, d) => (a.indexOf(k) >= 0 ? a[a.indexOf(k) + 1] : d);
  const seedFile = opt("--seed-file");
  try {
    if (!seedFile) throw new Error("--seed-file <path> is required");
    if (cmd === "generate") {
      const r = generate(seedFile);
      console.log(`seed written to ${seedFile} (never share it, never commit it)`);
      console.log(`public key: ${r.publicKey}`);
      console.log(`key id:     ${r.keyId}`);
    } else if (cmd === "check") {
      const r = check(seedFile, opt("--pubkey", "keys/flowseal-manifest-public-key.b64"));
      console.log(r.match ? `match (key id ${r.keyId})` : `MISMATCH: seed key id ${r.keyId}, pinned key id ${r.pinnedKeyId}`);
      process.exit(r.match ? 0 : 1);
    } else {
      throw new Error("usage: flowseal-key.mjs generate|check --seed-file <path> [--pubkey <file>]");
    }
  } catch (e) {
    console.error(`flowseal-key: ${e.message}`);
    process.exit(2);
  }
}
