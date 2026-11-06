// node --test scripts/ci/check-workflows.test.mjs — offline.
import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import { checkAll, checkWorkflow } from "./check-workflows.mjs";

const policy = JSON.parse(fs.readFileSync(".github/workflow-policy.json", "utf8"));
const CO = "actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0";
const DL = "actions/download-artifact@d3f86a106a0bac45b974a628896c90dbdf5c8093 # v4.3.0";
const wf = (body, on = "workflow_dispatch", perms = "permissions: {}") => `name: t\non: ${on}\n${perms}\njobs:\n${body}`;
const has = (errs, re) => assert.ok(errs.some((e) => re.test(e)), `expected ${re} in:\n${errs.join("\n")}`);

test("the repository's workflows follow the policy", () => {
  assert.deepEqual(checkAll(".github/workflows", policy).errors, []);
});

test("the alpha.9 workflows are rejected (floating tags, key next to third-party actions, write token in build)", () => {
  const fl = checkWorkflow("flowseal-manifest.yml", fs.readFileSync("scripts/ci/fixtures/flowseal-manifest.alpha9.yml", "utf8"), policy);
  has(fl, /dtolnay\/rust-toolchain@stable" is not pinned/);
  has(fl, /write permissions are granted per job/);
  has(fl, /environment flowseal-sign/);
  const rel = checkWorkflow("release.yml", fs.readFileSync("scripts/ci/fixtures/release.alpha9.yml", "utf8"), policy);
  has(rel, /tauri-apps\/tauri-action@v0" is not pinned/);
  has(rel, /write permissions are granted per job/);
});

test("pins need a full SHA and a version comment", () => {
  has(checkWorkflow("x.yml", wf(`  a:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n`), policy), /not pinned/);
  has(checkWorkflow("x.yml", wf(`  a:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: foo/bar@11d5960a326750d5838078e36cf38b85af677262\n`), policy), /no "# version" comment/);
});

test("checkout must not persist credentials", () => {
  has(checkWorkflow("x.yml", wf(`  a:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: ${CO}\n`), policy), /persist-credentials/);
});

test("write access only in listed publish jobs that build nothing", () => {
  const body = (steps) => `  publish:\n    runs-on: ubuntu-latest\n    permissions:\n      contents: write\n    steps:\n${steps}`;
  has(checkWorkflow("x.yml", wf(body(`      - run: gh release create v1\n`)), policy), /not a policy publish job/);
  has(checkWorkflow("release.yml", wf(body(`      - run: pnpm tauri build\n`)), policy), /must not build/);
  has(checkWorkflow("release.yml", wf(body(`      - uses: ${CO}\n        with:\n          persist-credentials: false\n`)), policy), /may only download artifacts/);
  has(checkWorkflow("release.yml", wf(body(`      - env:\n          K: \${{ secrets.X }}\n        run: gh release create v1\n`)), policy), /must not read secrets/);
  assert.deepEqual(checkWorkflow("release.yml", wf(body(`      - uses: ${DL}\n      - run: gh release create v1 a.exe\n`)), policy), []);
});

test("secrets only in the protected, dispatch-only signer step", () => {
  const sign = (opts = {}) =>
    wf(
      `  sign:\n    if: github.ref == 'refs/heads/main'\n    runs-on: ubuntu-latest\n    environment: ${opts.env ?? "flowseal-sign"}\n    permissions:\n      contents: read\n    steps:\n${opts.steps ?? `      - env:\n          K: \${{ secrets.FLOWSEAL_MANIFEST_SIGNING_KEY }}\n        run: node scripts/sign/sign-ed25519.mjs --payload p\n`}`,
      opts.on ?? "workflow_dispatch",
    );
  assert.deepEqual(checkWorkflow("flowseal-manifest.yml", sign(), policy), []);
  has(checkWorkflow("flowseal-manifest.yml", sign({ env: "other" }), policy), /protected environment/);
  has(checkWorkflow("flowseal-manifest.yml", sign({ on: "[workflow_dispatch, pull_request]" }), policy), /only be triggered by workflow_dispatch/);
  has(checkWorkflow("flowseal-manifest.yml", sign({ steps: `      - uses: dtolnay/rust-toolchain@b725eda525362e7cf34af9a7243cac1e3f01a7ec # stable\n      - env:\n          K: \${{ secrets.FLOWSEAL_MANIFEST_SIGNING_KEY }}\n        run: node scripts/sign/sign-ed25519.mjs\n` }), policy), /may not use dtolnay/);
  has(checkWorkflow("flowseal-manifest.yml", sign({ steps: `      - env:\n          K: \${{ secrets.FLOWSEAL_MANIFEST_SIGNING_KEY }}\n        run: cargo run -p bridge-tools -- sign m o\n` }), policy), /only the dependency-free signer/);
  has(checkWorkflow("flowseal-manifest.yml", sign({ steps: `      - run: node scripts/sign/sign-ed25519.mjs --key \${{ secrets.FLOWSEAL_MANIFEST_SIGNING_KEY }}\n` }), policy), /only through env/);
  has(checkWorkflow("flowseal-manifest.yml", sign({ steps: `      - uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4.6.2\n        with:\n          path: .\n` }), policy), /only \*\.signed\.json/);
  has(checkWorkflow("ci.yml", wf(`  t:\n    runs-on: ubuntu-latest\n    steps:\n      - env:\n          K: \${{ secrets.FLOWSEAL_MANIFEST_SIGNING_KEY }}\n        run: echo\n`, "pull_request"), policy), /not a policy signing job/);
});

test("dangerous triggers and script injection are rejected", () => {
  has(checkWorkflow("x.yml", wf(`  a:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo\n`, "pull_request_target"), policy), /pull_request_target/);
  has(checkWorkflow("x.yml", wf(`  a:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo "\${{ github.event.pull_request.title }}"\n`, "pull_request"), policy), /untrusted event field/);
  has(checkWorkflow("x.yml", `name: t\non: push\njobs:\n  a:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo\n`, policy), /declare permissions/);
});
