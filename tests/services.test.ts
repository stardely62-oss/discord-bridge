import { describe, expect, it } from "vitest";
import { applyLabel, groupDomains, ladder, matchText, rulePorts, ruleProtocols, sourceText } from "../src/catalog/services";
import type { CardDetails, DomainInfo, RuleInfo } from "../src/services/types";

const tr = (ru: string) => ru;
const rule = (o: Partial<RuleInfo>): RuleInfo => ({ index: 1, enabled: true, hostlists: [], hostlistDomains: [], hostlistExcludes: [], ipsets: [], ipsetExcludes: [], l7: [], ...o });
const dom = (domain: string, group: string, list = "list-google.txt"): DomainInfo => ({ domain, group, sources: [{ list, line: 1, raw: domain, parent: false }], excludedBy: [], sharedWith: [] });
const details = (state: CardDetails["summary"]["state"]): CardDetails => ({
  summary: { id: "x", kind: "flowseal", domains: 1, inFlowseal: 1, excluded: 0, shared: 0, ips: 0, rules: 1, extraRules: 0, state },
  domains: [],
  serviceIps: [],
  rules: [],
  ipset: { mode: "none", runtimePlaceholder: true, runtimeAll: false, rules: [] },
  sources: [],
  pendingApply: false,
});

describe("service cards", () => {
  it("apply labels never claim that a service works", () => {
    for (const s of ["unknown", "known", "prepared", "applied"] as const) {
      const l = applyLabel(s, tr);
      expect(l.label.toLowerCase()).not.toContain("работает");
    }
    expect(applyLabel("applied", tr).hint).toContain("не проверка доступности");
  });

  it("the confirmed step needs a real check and an applied configuration", () => {
    expect(ladder(details("applied"), null).at(-1)!.done).toBeNull();
    expect(ladder(details("applied"), true).at(-1)!.done).toBe(true);
    expect(ladder(details("prepared"), true).at(-1)!.done).toBe(false);
    expect(ladder(details("known"), null).map((s) => s.done)).toEqual([true, false, false, null]);
  });

  it("ports and protocols are shown as the rule has them", () => {
    expect(rulePorts(rule({ udp: "19294-19344,50000-50100" }), tr)).toBe("UDP 19294-19344,50000-50100");
    expect(rulePorts(rule({ tcp: "80,443", udp: "443" }), tr)).toBe("TCP 80,443 · UDP 443");
    expect(ruleProtocols(rule({ tcp: "443", udp: "443" }))).toEqual(["TCP", "UDP/QUIC"]);
    expect(matchText({ by: "protocol", value: ["discord", "stun"] }, tr)).toContain("discord, stun");
    expect(sourceText({ list: "list-general.txt", line: 44, raw: "discordapp.net", parent: true }, tr)).toContain("родительский домен");
  });

  it("domains are grouped and searched", () => {
    const g = groupDomains([dom("youtube.com", "youtube.com"), dom("yt3.ggpht.com", "ggpht.com"), dom("yt4.ggpht.com", "ggpht.com")], "");
    expect(g.map((x) => [x.group, x.items.length])).toEqual([["ggpht.com", 2], ["youtube.com", 1]]);
    expect(groupDomains([dom("youtube.com", "youtube.com"), dom("yt3.ggpht.com", "ggpht.com")], "YT3").map((x) => x.group)).toEqual(["ggpht.com"]);
  });
});
