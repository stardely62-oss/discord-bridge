// Display helpers for the catalog of address packs.
//
// The data comes from the Rust side only (catalog_get / catalog_refresh):
// a signed, verified remote catalog or the snapshot shipped with the app.
// Installing, updating, rolling back and removing go through Rust commands
// that validate and save `installedPacks`; nothing here touches settings.

import type { CatalogPack, CatalogView, GameFilterMode, InstalledPack, IpSetStatus, IpsetMode, Localized, NeededPorts, PackCategory, PackPorts, PackState } from "../services/types";

export const CATEGORY_RU: Record<PackCategory, string> = {
  messenger: "мессенджер",
  voice: "голос",
  game: "игра",
  launcher: "лаунчер",
  video: "видео",
  streaming: "стриминг",
  service: "сервис",
  other: "другое",
};
export const CATEGORY_EN: Record<PackCategory, string> = {
  messenger: "messenger",
  voice: "voice",
  game: "game",
  launcher: "launcher",
  video: "video",
  streaming: "streaming",
  service: "service",
  other: "other",
};

export const loc = (l: Localized | null | undefined, lang: "ru" | "en") => (l ? (lang === "en" ? l.en : l.ru) : "");

/** Status label of a pack against the installed snapshots (one meaning each). */
export function stateLabel(state: PackState, lang: "ru" | "en"): string {
  const ru: Record<PackState, string> = {
    flowseal: "входит в Flowseal",
    available: "не установлен",
    current: "установлен",
    updateAvailable: "есть новая версия",
    differs: "отличается от каталога",
  };
  const en: Record<PackState, string> = {
    flowseal: "part of Flowseal",
    available: "not installed",
    current: "installed",
    updateAvailable: "new version available",
    differs: "differs from the catalog",
  };
  return (lang === "en" ? en : ru)[state];
}

/** Search by name, id or domain; filter by category and source. */
export function filterPacks<T extends { pack: CatalogPack }>(items: T[], query: string, category: PackCategory | "all", source: "official" | "community"): T[] {
  const q = query.trim().toLowerCase();
  return items.filter(({ pack: p }) => {
    if (p.source !== source) return false;
    if (category !== "all" && p.category !== category) return false;
    if (!q) return true;
    return [p.id, p.name.ru, p.name.en, ...p.domains].some((s) => s.toLowerCase().includes(q));
  });
}

/** Categories present in the catalog, in a stable order. */
export function presentCategories(items: { pack: CatalogPack }[]): PackCategory[] {
  const order: PackCategory[] = ["messenger", "voice", "game", "launcher", "video", "streaming", "service", "other"];
  const have = new Set(items.map((i) => i.pack.category));
  return order.filter((c) => have.has(c));
}

/** One honest line about where the catalog comes from and how fresh it is. */
export function syncLine(v: CatalogView, lang: "ru" | "en", now = Date.now() / 1000): { tone: "ok" | "warn" | "info"; text: string } {
  const tr = (ru: string, en: string) => (lang === "en" ? en : ru);
  const ago = (t?: number | null) => {
    if (!t) return tr("никогда", "never");
    const s = Math.max(0, Math.floor(now - t));
    if (s < 90) return tr("только что", "just now");
    if (s < 5400) return tr(`${Math.round(s / 60)} мин назад`, `${Math.round(s / 60)} min ago`);
    if (s < 129600) return tr(`${Math.round(s / 3600)} ч назад`, `${Math.round(s / 3600)} h ago`);
    return tr(`${Math.round(s / 86400)} дн назад`, `${Math.round(s / 86400)} d ago`);
  };
  const date = v.published ? v.published.slice(0, 10) : "";
  const base =
    v.origin === "remote"
      ? tr(`Подписанный каталог от ${date}`, `Signed catalog of ${date}`)
      : tr("Встроенный каталог (опубликованный ещё не загружен)", "Built-in catalog (no published catalog loaded yet)");
  if (v.error) {
    return {
      tone: "warn",
      text: v.offline
        ? tr(`${base}. Нет связи с сервером каталога — используется последняя проверенная версия.`, `${base}. No connection to the catalog server — the last verified version is used.`)
        : tr(`${base}. Обновление не принято: ${v.error}`, `${base}. Update rejected: ${v.error}`),
    };
  }
  return { tone: v.origin === "remote" ? "ok" : "info", text: `${base}. ${tr("Проверен", "Checked")} ${ago(v.lastCheck)}.` };
}

export function installedTitle(i: InstalledPack, lang: "ru" | "en"): string {
  return loc(i.title, lang) || i.id;
}

/** How an ipset mode treats the selected IPs (manual list + catalog packs). */
export type IpUse = "applied" | "ignored" | "allAddresses";
export function ipUse(mode: IpsetMode): IpUse {
  if (mode === "selected" || mode === "flowsealPlusSelected") return "applied";
  if (mode === "any") return "allAddresses";
  return "ignored";
}

/**
 * Where a pack's IPs stand, step by step, from the backend's IpSetStatus.
 * "applied" only when the backend confirms the running engine uses them.
 */
export type PackIpState =
  | "notInstalledIgnored" // not installed, and the mode would not use its IPs
  | "notInstalled"
  | "allAddresses" // mode "any": every address is processed anyway
  | "modeIgnores"
  | "allExcluded"
  | "strategyNoIpset"
  | "notApplied" // in the prepared configuration, not in the running engine
  | "applied";

export function packIpState(p: { id: string; ips: string[] }, state: PackState, ip: IpSetStatus): PackIpState | null {
  if (!p.ips?.length) return null;
  if (state === "available" || state === "flowseal") return ip.modeUsesSelected || ip.mode === "any" ? "notInstalled" : "notInstalledIgnored";
  if (ip.strategy.ipsetRules === 0) return "strategyNoIpset";
  if (ip.mode === "any") return "allAddresses";
  if (!ip.modeUsesSelected) return "modeIgnores";
  const own = ip.packs.find((x) => x.id === p.id);
  if (own && own.effective === 0) return "allExcluded";
  return ip.applied ? "applied" : "notApplied";
}

/** Same ladder for the manual "IP via bypass" list. */
export function manualIpState(includeCount: number, ip: IpSetStatus): PackIpState | null {
  if (includeCount === 0) return null;
  if (ip.strategy.ipsetRules === 0) return "strategyNoIpset";
  if (ip.mode === "any") return "allAddresses";
  if (!ip.modeUsesSelected) return "modeIgnores";
  if (ip.prepared === 0) return "allExcluded";
  return ip.applied ? "applied" : "notApplied";
}

/** The user's own "through the bypass" entries (comments excluded). */
export function ownEntries(manual: string[]): string[] {
  return manual.filter((l) => l.trim() && !l.trim().startsWith("#"));
}

/** Declared ports of a pack as a list (`ports.tcp`, `ports.udp`). */
export function declaredPorts(p: PackPorts | null | undefined): NeededPorts[] {
  const out: NeededPorts[] = [];
  if (p?.tcp) out.push({ protocol: "tcp", ports: p.tcp });
  if (p?.udp) out.push({ protocol: "udp", ports: p.udp });
  return out;
}

/** `UDP 5055–5058, TCP 4530` */
export function portsText(list: NeededPorts[]): string {
  return list.map((n) => `${n.protocol.toUpperCase()} ${n.ports.replace(/-/g, "–").replace(/,/g, ", ")}`).join("; ");
}

export const GAME_FILTER_MODE_RU: Record<GameFilterMode, string> = { disabled: "выключен", all: "TCP и UDP", tcp: "TCP", udp: "UDP" };
export const GAME_FILTER_MODE_EN: Record<GameFilterMode, string> = { disabled: "off", all: "TCP and UDP", tcp: "TCP", udp: "UDP" };
