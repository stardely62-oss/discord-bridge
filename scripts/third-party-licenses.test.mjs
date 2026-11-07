import { test } from "node:test";
import assert from "node:assert/strict";
import { spdxAllowed } from "./third-party-licenses.mjs";

test("permissive expressions are accepted", () => {
  for (const e of ["MIT", "MIT OR Apache-2.0", "MIT/Apache-2.0", "Apache-2.0 / MIT", "(MIT OR Apache-2.0) AND Unicode-3.0",
    "Apache-2.0 WITH LLVM-exception OR MIT", "MPL-2.0", "BSD-3-Clause AND MIT", "Unlicense OR MIT"]) {
    assert.equal(spdxAllowed(e), true, e);
  }
});

test("copyleft or unknown licenses are rejected unless an alternative is permissive", () => {
  for (const e of ["GPL-3.0", "LGPL-2.1-or-later", "AGPL-3.0", "MIT AND GPL-2.0", "SSPL-1.0", "", "MIT OR", "(MIT"]) {
    assert.equal(spdxAllowed(e), false, e);
  }
  assert.equal(spdxAllowed("GPL-3.0 OR MIT"), true);
});
