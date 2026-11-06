import { describe, expect, it } from "vitest";
import { review, sameSnapshot } from "../src/catalog/confirm";
import type { CatalogPack, PackConfirm, PackDiff, PackView } from "../src/services/types";

const pack = (version: number, domains: string[]): CatalogPack => ({
  schema: 1,
  id: "steam",
  version,
  updated: "2026-11-06",
  name: { ru: "Steam", en: "Steam" },
  description: { ru: "", en: "" },
  category: "launcher",
  source: "official",
  domains,
  ips: [],
  provenance: [],
  partialCoverage: false,
  requires: { selectedIps: false, gameFilter: false },
});
const token = (serial: number, sha256: string, version = 2, installedSha256: string | null = "i1"): PackConfirm => ({ id: "steam", version, serial, sha256, installedSha256 });
const diff = (c: PackConfirm, added: string[]): PackDiff => ({ id: "steam", confirm: c, fromVersion: "1", toVersion: String(c.version), addedDomains: added, removedDomains: [], addedIps: [], removedIps: [] });
const view = (c: PackConfirm, domains: string[], withDiff = true): PackView => ({
  pack: pack(c.version, domains),
  state: "updateAvailable",
  confirm: c,
  diff: withDiff ? diff(c, domains.slice(1)) : null,
});

const A = token(100, "aaa");
const B = token(200, "bbb"); // same pack version, other addresses

describe("confirmation of the exact reviewed snapshot", () => {
  it("compares every part of the token", () => {
    expect(sameSnapshot(A, { ...A })).toBe(true);
    expect(sameSnapshot(A, { ...A, serial: 101 })).toBe(false);
    expect(sameSnapshot(A, { ...A, sha256: "x" })).toBe(false);
    expect(sameSnapshot(A, { ...A, version: 3 })).toBe(false);
    expect(sameSnapshot(A, { ...A, installedSha256: null })).toBe(false);
    expect(sameSnapshot({ ...A, installedSha256: null }, { ...A, installedSha256: undefined })).toBe(true);
    expect(sameSnapshot(A, null)).toBe(false);
  });

  it("an unchanged snapshot can be confirmed with exactly its token", () => {
    const v = view(A, ["steampowered.com", "a.example"]);
    const r = review(v, v, null, true);
    expect(r.stale).toBe(false);
    expect(r.confirm).toEqual(A);
    expect(r.diff?.addedDomains).toEqual(["a.example"]);
  });

  it("catalog B arriving while A is reviewed blocks the confirmation and keeps A on screen", () => {
    const shown = view(A, ["steampowered.com", "a.example"]);
    const live = view(B, ["steampowered.com", "b.example"]);
    const r = review(shown, live, null, true);
    expect(r.stale).toBe(true);
    expect(r.confirm).toBeNull();
    expect(r.diff?.addedDomains).toEqual(["a.example"]); // never B silently
  });

  it("an async pack_diff answer from a newer catalog is ignored", () => {
    const shown = view(A, ["steampowered.com", "a.example"], false);
    const r = review(shown, shown, diff(B, ["b.example"]), true);
    expect(r.diff).toBeNull();
    expect(r.confirm).toBeNull(); // not ready: nothing to confirm yet
    const ok = review(shown, shown, diff(A, ["a.example"]), true);
    expect(ok.confirm).toEqual(A);
  });

  it("the button stays disabled until the diff of the shown snapshot is there", () => {
    const shown = view(A, ["steampowered.com"], false);
    expect(review(shown, shown, null, true).confirm).toBeNull();
    // A first install needs no diff, only an unchanged snapshot.
    expect(review(shown, shown, null, false).confirm).toEqual(A);
  });

  it("a new version or a changed installed copy needs a new review", () => {
    const shown = view(A, ["steampowered.com"]);
    expect(review(shown, view(token(100, "aaa", 3), ["steampowered.com"]), null, true).confirm).toBeNull();
    expect(review(shown, view(token(100, "aaa", 2, "i2"), ["steampowered.com"]), null, true).confirm).toBeNull();
    expect(review(shown, null, null, true).confirm).toBeNull(); // pack left the catalog
  });
});
