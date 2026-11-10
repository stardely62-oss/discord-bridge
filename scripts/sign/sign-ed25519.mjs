// Ed25519 signer for the Flowseal manifest workflow. Dependency-free: only
// Node's built-in crypto, so the signing job runs no package manager, no
// build script and no third-party action next to the key.
//
//   node scripts/sign/sign-ed25519.mjs --payload <file> --out <signed.json>
//     --pubkey <pinned.b64> --expect-sha256 <hex> --key-env <ENV_NAME>
//     [--expect-version <v> --expect-source-sha256 <hex>]
//     [--expect-app-version <v> --expect-installer-sha256 <hex>]
//
// It refuses to sign unless
// - the payload's SHA-256 is the one the build job reported (job output,
//   not the artifact store);
// - the payload is a Flowseal manifest for the reviewed version and archive
//   hash, from the official Flowseal release URL — or, with the app options,
//   a Bridge release manifest for the reviewed version and installer hash
//   (exactly one of the two kinds must be requested);
// - the key from the environment derives to the pinned public key.
// It then verifies its own signature with the pinned key. The key is read
// from the environment once, removed from it and never written anywhere;
// the output contains only the payload, the signature and the key id.
import { createHash, createPrivateKey, createPublicKey, sign, verify } from "node:crypto";
import fs from "node:fs";

const args = process.argv.slice(2);
const opt = (k) => {
  const i = args.indexOf(k);
  return i >= 0 ? args[i + 1] : undefined;
};
const die = (msg) => {
  console.error(`sign: ${msg}`);
  process.exit(1);
};

const payloadPath = opt("--payload");
const outPath = opt("--out");
const pubPath = opt("--pubkey");
const expectSha = opt("--expect-sha256");
const keyEnv = opt("--key-env");
if (!payloadPath || !outPath || !pubPath || !expectSha || !keyEnv) die("missing arguments");
if (!/^[0-9a-f]{64}$/.test(expectSha)) die("--expect-sha256 must be 64 lowercase hex characters");

const payload = fs.readFileSync(payloadPath);
const got = createHash("sha256").update(payload).digest("hex");
if (got !== expectSha) die(`payload SHA-256 ${got} is not the one the build job produced (${expectSha})`);

let doc;
try {
  doc = JSON.parse(payload.toString("utf8"));
} catch {
  die("payload is not JSON");
}
const version = opt("--expect-version");
const sourceSha = opt("--expect-source-sha256");
if (version !== undefined || sourceSha !== undefined) {
  if (doc.flowsealVersion !== version) die("payload is not for the requested Flowseal version");
  if (doc.sourceSha256 !== sourceSha) die("payload is not for the reviewed archive SHA-256");
  const official = `https://github.com/Flowseal/zapret-discord-youtube/releases/download/${version}/`;
  if (typeof doc.sourceUrl !== "string" || !doc.sourceUrl.startsWith(official)) die("payload source is not the official Flowseal release");
  if (!Array.isArray(doc.files) || doc.files.length === 0) die("payload lists no files");
}

const appVersion = opt("--expect-app-version");
const installerSha = opt("--expect-installer-sha256");
const wantsApp = appVersion !== undefined || installerSha !== undefined;
const wantsFlowseal = version !== undefined || sourceSha !== undefined;
if (wantsApp === wantsFlowseal) die("say what is signed: either the Flowseal options or the app release options");
if (wantsApp) {
  if (!appVersion || !/^[0-9a-f]{64}$/.test(installerSha ?? "")) die("--expect-app-version and --expect-installer-sha256 go together");
  if (doc.kind !== "bridge-app-release") die("payload is not a Bridge release manifest");
  if (doc.version !== appVersion) die("payload is not for the requested Bridge version");
  if (doc.installerSha256 !== installerSha) die("payload is not for the reviewed installer SHA-256");
  if (doc.installerName !== `Bridge_${appVersion}_x64-setup.exe`) die("payload names another installer");
} else if (doc.kind !== undefined) {
  die("payload is not a Flowseal manifest");
}

const pinned = Buffer.from(fs.readFileSync(pubPath, "utf8").trim(), "base64");
if (pinned.length !== 32) die("pinned public key must be 32 bytes");

const seedB64 = process.env[keyEnv];
delete process.env[keyEnv];
if (!seedB64) die(`${keyEnv} is not set`);
const seed = Buffer.from(seedB64.trim(), "base64");
if (seed.length !== 32) die(`${keyEnv} must be a base64 32-byte Ed25519 seed`);
const key = createPrivateKey({ key: Buffer.concat([Buffer.from("302e020100300506032b657004220420", "hex"), seed]), format: "der", type: "pkcs8" });
seed.fill(0);
const pub = createPublicKey(key).export({ format: "der", type: "spki" }).subarray(-32);
if (!pub.equals(pinned)) die("the signing key does not match the pinned public key");

const signature = sign(null, payload, key);
const pinnedKey = createPublicKey({ key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), pinned]), format: "der", type: "spki" });
if (!verify(null, payload, pinnedKey, signature)) die("self-check of the signature failed");

const envelope = { manifest: payload.toString("base64"), signature: signature.toString("base64"), keyId: pinned.subarray(0, 8).toString("hex") };
fs.writeFileSync(outPath, JSON.stringify(envelope, null, 2) + "\n");
console.log(`signed ${payloadPath} (sha256 ${got}) with key ${envelope.keyId} → ${outPath}`);
