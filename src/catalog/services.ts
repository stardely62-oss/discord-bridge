// Display helpers for service cards. All facts come from Rust
// (bridge_core::services): catalog domains, the original Flowseal lines of
// the active version, the selected/running strategy. Nothing here infers that
// a service *works*.

import type { ApplyState, CardDetails, DomainInfo, LineRef, RuleInfo, RuleMatch } from "../services/types";

export type Tr = (ru: string, en: string) => string;
export type Tone = "neutral" | "accent" | "ok" | "warn" | "bad";

export function applyLabel(state: ApplyState, tr: Tr): { label: string; tone: Tone; hint: string } {
  switch (state) {
    case "unknown":
      return { label: tr("Источник недоступен", "Source unavailable"), tone: "warn", hint: tr("Адреса сервиса не найдены в проверенных списках активной версии Flowseal или исключены.", "The service's addresses are not in the verified lists of the active Flowseal version, or are excluded.") };
    case "known":
      return { label: tr("Адреса известны", "Addresses known"), tone: "neutral", hint: tr("Адреса есть в проверенном источнике, но выбранная стратегия их не обрабатывает (или набор не установлен).", "The addresses are in a verified source, but the selected strategy does not handle them (or the set is not installed).") };
    case "prepared":
      return { label: tr("Правила подготовлены", "Rules prepared"), tone: "accent", hint: tr("В выбранной стратегии есть правила для этих адресов; они применятся при запуске или перезапуске обхода.", "The selected strategy has rules for these addresses; they apply when the bypass starts or restarts.") };
    case "applied":
      return { label: tr("Правила применяются", "Rules applied"), tone: "ok", hint: tr("Работающий WinWS использует ровно эту конфигурацию. Это не проверка доступности.", "The running WinWS uses exactly this configuration. This is not a reachability check.") };
  }
}

/** "TCP 80,443" / "UDP 443" / "TCP 443 · UDP 443" / "все порты перехвата". */
export function rulePorts(r: RuleInfo, tr: Tr): string {
  const p = [r.tcp ? `TCP ${r.tcp}` : null, r.udp ? `UDP ${r.udp}` : null].filter(Boolean);
  return p.length ? p.join(" · ") : tr("все перехватываемые порты", "all captured ports");
}

/** QUIC is HTTP/3 over UDP 443. */
export function ruleProtocols(r: RuleInfo): string[] {
  const out: string[] = [];
  if (r.tcp) out.push("TCP");
  if (r.udp) out.push(r.udp.split(",").some((x) => x.trim() === "443") ? "UDP/QUIC" : "UDP");
  return out;
}

export function matchText(m: RuleMatch, tr: Tr): string {
  switch (m.by) {
    case "hostlist":
      return tr(`список ${m.value}`, `list ${m.value}`);
    case "domains":
      return tr(`домены в правиле: ${m.value.join(", ")}`, `domains in the rule: ${m.value.join(", ")}`);
    case "protocol":
      return tr(`протокол ${m.value.join(", ")} (L7)`, `protocol ${m.value.join(", ")} (L7)`);
  }
}

export function sourceText(r: LineRef, tr: Tr): string {
  return r.parent ? tr(`${r.list}:${r.line} — родительский домен «${r.raw}»`, `${r.list}:${r.line} — parent domain “${r.raw}”`) : `${r.list}:${r.line}`;
}

/** Domains grouped by their base domain, filtered by a case-insensitive query. */
export function groupDomains(domains: DomainInfo[], query: string): { group: string; items: DomainInfo[] }[] {
  const q = query.trim().toLowerCase();
  const m = new Map<string, DomainInfo[]>();
  for (const d of domains) {
    if (q && !d.domain.toLowerCase().includes(q) && !d.sources.some((s) => s.list.includes(q))) continue;
    const g = m.get(d.group) ?? [];
    g.push(d);
    m.set(d.group, g);
  }
  return [...m.entries()].sort((a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0])).map(([group, items]) => ({ group, items }));
}

/** Steps shown in the overview; `confirmed` only with a real check. */
export function ladder(d: CardDetails, confirmed: boolean | null): { key: "known" | "prepared" | "applied" | "confirmed"; done: boolean | null }[] {
  const order: ApplyState[] = ["unknown", "known", "prepared", "applied"];
  const at = order.indexOf(d.summary.state);
  return [
    { key: "known", done: at >= 1 },
    { key: "prepared", done: at >= 2 },
    { key: "applied", done: at >= 3 },
    // null: no real check exists for this service.
    { key: "confirmed", done: at >= 3 ? confirmed : confirmed === null ? null : false },
  ];
}
