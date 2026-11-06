import { describe, expect, it } from "vitest";
import { access } from "../src/pages/Home/status";
import type { CheckItem, DiagnosticsReport } from "../src/services/types";
import { ruPlural } from "../src/stores/app";

const ru = (a: string) => a;
const https = (status: CheckItem["status"], n: number): CheckItem[] =>
  Array.from({ length: n }, (_, i) => ({ id: `https:${i}`, group: "https", title: `h${i}`, status, detail: "" }) as CheckItem);
const report = (items: CheckItem[], httpsOk: boolean, extra: Partial<DiagnosticsReport> = {}): DiagnosticsReport => ({
  at: Math.floor(Date.now() / 1000),
  strategyId: "a",
  engineRunning: true,
  httpsOk,
  voice: "notChecked",
  items,
  ...extra,
});

describe("Russian plurals", () => {
  const f: [string, string, string] = ["правило", "правила", "правил"];
  it.each([
    [1, "правило"],
    [2, "правила"],
    [4, "правила"],
    [5, "правил"],
    [11, "правил"],
    [12, "правил"],
    [14, "правил"],
    [21, "правило"],
    [22, "правила"],
    [25, "правил"],
    [101, "правило"],
    [111, "правил"],
    [0, "правил"],
  ])("%i → %s", (n, want) => expect(ruPlural(n, f)).toBe(want));
});

describe("home: Discord access status", () => {
  it("is unknown without a check", () => expect(access(null, "a", true, ru).level).toBe("unknown"));
  it("is ok only when every HTTPS check passed", () => expect(access(report(https("ok", 5), true), "a", true, ru).level).toBe("ok"));
  it("is partial when some hosts fail", () => {
    const r = access(report([...https("ok", 3), ...https("error", 1)], false), "a", true, ru);
    expect(r.level).toBe("warn");
    expect(r.value).toContain("1 из 4");
  });
  it("is bad when nothing answers", () => expect(access(report(https("error", 4), false), "a", true, ru).level).toBe("bad"));
  it("is stale for another strategy or bypass state", () => {
    expect(access(report(https("ok", 4), true), "b", true, ru).level).toBe("unknown");
    expect(access(report(https("ok", 4), true), "a", false, ru).level).toBe("unknown");
  });
});

import { filterPacks, ipUse, manualIpState, ownEntries, packIpState, presentCategories, stateLabel, syncLine } from "../src/catalog/apps";
import { MANUAL_LISTS } from "../src/catalog/lists";
import { strategyKind } from "../src/catalog/strategies";
import builtin from "../resources/catalog/builtin-catalog.json";
// Display rules are tested on the ten-pack test catalog (installable packs, IPs);
// the shipped built-in snapshot is checked separately below.
import fixture from "../crates/bridge-core/tests/fixtures/catalog/catalog-data.json";
import type { CatalogPack, CatalogView, IpSetStatus, PackView } from "../src/services/types";

const packs = (fixture as { packs: CatalogPack[] }).packs;
// Optional fields (partialCoverage, requires) are absent in the JSON: read it as unknown first.
const builtinPacks = (builtin as unknown as { packs: CatalogPack[] }).packs;
const view = (id: string, state: PackView["state"]): PackView => {
  const pack = packs.find((p) => p.id === id)!;
  return { pack, state, confirm: { id, version: pack.version, serial: 1, sha256: "0".repeat(64), installedSha256: null } };
};

describe("catalog display", () => {
  it("the built-in snapshot has the four Flowseal service cards with real icons", () => {
    expect(builtinPacks.map((p) => p.id).sort()).toEqual(["cloudflare", "discord", "google", "youtube"]);
    expect(builtinPacks.every((p) => p.icon?.endsWith(".png"))).toBe(true);
    expect(builtinPacks.every((p) => p.flowsealList && p.ips.length === 0)).toBe(true);
  });

  it("the test catalog keeps the ten packs the display rules are tested on", () => {
    expect(packs.map((p) => p.id).sort()).toEqual(["battlenet", "discord", "epic", "riot", "roblox", "steam", "telegram", "twitch", "valorant", "youtube"]);
    expect(packs.find((p) => p.id === "telegram")!.ips).toHaveLength(14);
  });

  it("never merges statuses into one “works”", () => {
    const labels = (["flowseal", "available", "current", "updateAvailable", "differs"] as const).map((s) => stateLabel(s, "ru"));
    expect(new Set(labels).size).toBe(5);
    expect(labels.join(" ")).not.toMatch(/работает/);
    expect(stateLabel("flowseal", "ru")).toBe("входит в Flowseal");
  });

  it("searches by name and domain and filters by category and source", () => {
    const all = packs.map((p) => ({ pack: p }));
    expect(filterPacks(all, "steamcommunity", "all", "official").map((v) => v.pack.id)).toEqual(["steam"]);
    expect(filterPacks(all, "", "game", "official").map((v) => v.pack.id).sort()).toEqual(["roblox", "valorant"]);
    expect(filterPacks(all, "", "all", "community")).toEqual([]);
    expect(presentCategories(all)).toContain("launcher");
  });

  it("sync line is honest about origin, offline and rejected updates", () => {
    const base: CatalogView = { origin: "builtin", offline: false, repaired: false, dataSerial: 1, packs: [], orphans: [], icons: {} };
    expect(syncLine(base, "ru").text).toMatch(/Встроенный каталог/);
    expect(syncLine({ ...base, origin: "remote", published: "2026-11-06T00:00:00Z", lastCheck: 1000 }, "ru", 1030).text).toMatch(/Подписанный каталог от 2026-11-06.*только что/);
    const off = syncLine({ ...base, origin: "remote", published: "2026-11-06T00:00:00Z", error: "x", offline: true }, "ru");
    expect(off.tone).toBe("warn");
    expect(off.text).toMatch(/последняя проверенная/);
    expect(syncLine({ ...base, error: "сервер отдал более старый каталог", offline: false }, "ru").text).toMatch(/не принято/);
  });

  it("ownEntries skips comments", () => expect(ownEntries(["# note", "a.com", " "])).toEqual(["a.com"]));
});

describe("IP sets in the catalog and manual lists", () => {
  const telegram = view("telegram", "current").pack;
  const steam = view("steam", "current").pack;

  it("knows which ipset modes apply selected IPs", () => {
    expect(ipUse("none")).toBe("ignored");
    expect(ipUse("flowseal")).toBe("ignored");
    expect(ipUse("selected")).toBe("applied");
    expect(ipUse("flowsealPlusSelected")).toBe("applied");
    expect(ipUse("any")).toBe("allAddresses");
  });

  const ip = (o: Partial<IpSetStatus> = {}): IpSetStatus => ({
    mode: "selected",
    modeUsesSelected: true,
    prepared: 14,
    strategyId: "flowseal-general",
    strategy: { ipsetRules: 4, ipsetRulesWithUserExclude: 4 },
    engineRunsStrategy: true,
    engineUpToDate: true,
    applied: true,
    packs: [{ id: "telegram", ips: 14, effective: 14 }],
    ...o,
  });

  it("calls IPs applied only when the backend confirms it (saved but not restarted is not applied)", () => {
    expect(packIpState(telegram, "current", ip())).toBe("applied");
    expect(packIpState(telegram, "current", ip({ applied: false, engineUpToDate: false }))).toBe("notApplied");
    expect(packIpState(telegram, "updateAvailable", ip({ applied: false, engineRunsStrategy: false, engineUpToDate: false }))).toBe("notApplied");
  });

  it("distinguishes installed, mode, exclusions and strategy", () => {
    expect(packIpState(telegram, "available", ip({ mode: "none", modeUsesSelected: false, applied: false }))).toBe("notInstalledIgnored");
    expect(packIpState(telegram, "available", ip())).toBe("notInstalled");
    expect(packIpState(telegram, "current", ip({ mode: "flowseal", modeUsesSelected: false, applied: false }))).toBe("modeIgnores");
    expect(packIpState(telegram, "current", ip({ packs: [{ id: "telegram", ips: 14, effective: 0 }], prepared: 0, applied: false }))).toBe("allExcluded");
    expect(packIpState(telegram, "current", ip({ strategy: { ipsetRules: 0, ipsetRulesWithUserExclude: 0 }, applied: false }))).toBe("strategyNoIpset");
    expect(packIpState(telegram, "current", ip({ mode: "any", modeUsesSelected: false, applied: false }))).toBe("allAddresses");
    expect(packIpState(steam, "current", ip())).toBeNull();
  });

  it("uses the same ladder for the manual IP list", () => {
    expect(manualIpState(0, ip())).toBeNull();
    expect(manualIpState(3, ip())).toBe("applied");
    expect(manualIpState(3, ip({ prepared: 0, applied: false }))).toBe("allExcluded");
    expect(manualIpState(3, ip({ mode: "none", modeUsesSelected: false, applied: false }))).toBe("modeIgnores");
    expect(manualIpState(3, ip({ strategy: { ipsetRules: 0, ipsetRulesWithUserExclude: 0 }, applied: false }))).toBe("strategyNoIpset");
  });

  it("offers four manual lists including “IP via bypass”", () => {
    expect(MANUAL_LISTS.map((l) => l.key)).toEqual(["general", "ipsetInclude", "exclude", "ipsetExclude"]);
    const inc = MANUAL_LISTS.find((l) => l.key === "ipsetInclude")!;
    expect(inc.kind).toBe("ip");
    expect(inc.through).toBe(true);
  });
});

describe("strategy kinds", () => {
  it.each([
    ["general", "базовая"],
    ["general (ALT3)", "альтернативная"],
    ["general (FAKE TLS AUTO ALT2)", "для проблемных сетей"],
    ["general (SIMPLE FAKE)", "упрощённая"],
    ["general (EXP)", "экспериментальная"],
    ["Моя", "своя"],
  ])("%s → %s", (n, k) => expect(strategyKind(n).ru).toBe(k));
  it("flags not recommended", () => expect(strategyKind("general (ALT5)", ["not recommended"]).tone).toBe("warn"));
});
