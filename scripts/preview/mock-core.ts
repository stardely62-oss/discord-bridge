// Browser-only mock of the Tauri backend for UI previews/screenshots.
// Not part of the app build (used only by scripts/preview/vite.config.ts).
import catalog from "./catalog.json";
import packCatalog from "../../resources/catalog/builtin-catalog.json";
import type { Arg, Strategy } from "../../src/services/types";

const strategies: Strategy[] = (catalog as { strategies: Strategy[] }).strategies;
const custom: Strategy = { ...structuredClone(strategies[3]), id: "my-alt3", name: "Моя ALT3 (дом)", source: "custom", origin: { baseId: strategies[3].id, flowsealVersion: "1.10.4" } };
custom.rules[0].args[2] = { opt: "--dpi-desync-repeats", value: "8" };
const all = [...strategies, custom];
const scenario = new URLSearchParams(location.search).get("s") ?? "running";
const theme = new URLSearchParams(location.search).get("t") ?? "dark";

const val = (a: Arg) => (a.value === undefined ? "" : typeof a.value === "string" ? a.value : "fake" in a.value ? a.value.fake : a.value.list);
const summary = (s: Strategy) =>
  s.rules.map((r) => {
    const g = (o: string) => r.args.find((a) => a.opt === o);
    const tcp = g("--filter-tcp"), udp = g("--filter-udp");
    return {
      protocol: tcp && udp ? "tcp+udp" : tcp ? "tcp" : udp ? "udp" : "any",
      tcpPorts: tcp ? val(tcp) : null,
      udpPorts: udp ? val(udp) : null,
      l7: g("--filter-l7") ? val(g("--filter-l7")!) : null,
      lists: r.args.filter((a) => a.opt.startsWith("--hostlist") || a.opt.startsWith("--ipset")).filter((a) => !a.opt.endsWith("domains")).map(val),
      domains: r.args.filter((a) => a.opt.endsWith("domains")).map(val),
      fakes: [...new Set(r.args.filter((a) => typeof a.value === "object" && a.value && "fake" in a.value).map(val))],
      desync: g("--dpi-desync") ? val(g("--dpi-desync")!) : null,
      other: r.args.filter((a) => /repeats|cutoff|fooling|split-pos|seqovl$/.test(a.opt)).map((a) => `${a.opt}=${val(a)}`),
    };
  });
const details = (s: Strategy) => ({
  strategy: s,
  issues: s.id === "my-alt3" ? [{ severity: "warning", path: "rules[2].args[4]", message: "fooling=ts работает только при включённых TCP timestamps в Windows" }] : [],
  summaries: summary(s),
  argsPreview: [...s.global.map((a) => `${a.opt}=${val(a)}`), ...s.rules.flatMap((r, i) => [...(i ? ["--new"] : []), ...r.args.map((a) => `${a.opt}=${val(a)}`)])],
  linesPreview: [s.global.map((a) => `${a.opt}=${val(a)}`).join(" "), ...s.rules.map((r, i) => `[${i + 1}] ` + r.args.map((a) => `${a.opt}=${val(a)}`).join(" "))],
  resources: [],
});
const list = () =>
  all.map((s) => ({
    id: s.id, name: s.name, description: s.description, source: s.source, baseId: s.origin.baseId, flowsealVersion: "1.10.4",
    ruleCount: s.rules.length, hasUdp: true, hasVoiceRule: s.rules.some((r) => r.args.some((a) => a.opt === "--filter-l7")), notes: s.notes ?? [], errorCount: s.id === "my-alt3" ? 2 : 0, draft: s.id === "my-alt3",
  }));

const settings = {
  schemaVersion: 2, language: new URLSearchParams(location.search).get("l") ?? "ru", theme, onboardingDone: scenario !== "onboarding", launchAtStartup: true, startMinimized: true, autoEnableBypass: true,
  autoLaunchDiscord: false, closeBehavior: "tray", stopBypassOnExit: true, trayNotifications: true, discordPath: null,
  activeStrategyId: "flowseal-general-alt2", lastWorkingStrategyId: "flowseal-general-alt2", bypassEnabled: true,
  gameFilter: { mode: "disabled", tcpRange: "1024-65535", udpRange: "1024-65535" }, ipsetMode: (new URLSearchParams(location.search).get("ip") ?? "none"),
  userLists: { general: ["example.org", "steampowered.com"], exclude: ["bank.example"], ipsetExclude: ["192.168.0.0/16"], ipsetInclude: [] },
  installedPacks: [
    { id: "steam", version: "2026-11-05", domains: ["steampowered.com", "steamcommunity.com", "steamstatic.com", "steamcontent.com", "steamserver.net", "steamgames.com", "steam-chat.com", "valvesoftware.com"] },
    { id: "twitch", version: "2026-10-01", domains: ["twitch.tv", "ttvnw.net", "jtvnw.net"] },
    { id: "telegram", version: "2026-11-05", domains: ["telegram.org", "t.me", "telegram.me", "telegra.ph", "telesco.pe", "cdn-telegram.org"], ips: ["91.108.56.0/22", "91.108.4.0/22", "91.108.8.0/22", "91.108.16.0/22", "91.108.12.0/22", "149.154.160.0/20", "91.105.192.0/23", "91.108.20.0/22", "185.76.151.0/24", "2001:b28:f23d::/48", "2001:b28:f23f::/48", "2001:67c:4e8::/48", "2001:b28:f23c::/48", "2a0a:f280::/32"] },
  ], keepLogs: true, checkUpdatesOnStart: true,
  autoCandidates: [], autoMaxAttempts: 8, voiceConfirmations: {}, launchMode: "withBypass",
};
const running = scenario !== "stopped" && scenario !== "onboarding";
const diag = {
  at: Math.floor(Date.now() / 1000) - 240, strategyId: "flowseal-general-alt2", engineRunning: running, httpsOk: false, voice: "notChecked",
  items: [
    { id: "discord", group: "discord", title: "Клиент Discord", status: "ok", detail: "Stable 1.0.9213, запущен" },
    { id: "helper", group: "zapret", title: "Служба Discord Bridge Helper", status: "ok", detail: "служба отвечает" },
    { id: "engine", group: "zapret", title: "Zapret (winws)", status: "ok", detail: "запущен, PID 8812, стратегия «general (ALT2)»" },
    { id: "flowseal", group: "zapret", title: "Компоненты Flowseal", status: "ok", detail: "версия 1.10.4, целостность подтверждена (SHA-256)" },
    { id: "tcp-ts", group: "system", title: "TCP timestamps", status: "warning", detail: "выключены, а активная стратегия использует fooling=ts", hint: "Flowseal включает их автоматически; Discord Bridge — только с вашего согласия." },
    { id: "bfe", group: "system", title: "Base Filtering Engine", status: "ok", detail: "служба запущена" },
    { id: "conflicts", group: "system", title: "Конфликты", status: "ok", detail: "других движков обхода DPI не обнаружено" },
    ...["discord.com", "gateway.discord.gg", "cdn.discordapp.com", "updates.discord.com"].flatMap((h, i) => [
      { id: "dns:" + h, group: "dns", title: h, status: "ok", detail: `162.159.${130 + i}.23${i}`, durationMs: 12 + i * 7 },
      { id: "https:" + h, group: "https", title: h, status: i === 3 ? "error" : "ok", detail: i === 3 ? "тайм-аут — характерно для блокировки DPI" : `ответ HTTP ${i ? 404 : 200}`, durationMs: 180 + i * 40 },
    ]),
    { id: "udp:voice-rules", group: "udp", title: "UDP-правила голоса", status: "ok", detail: "есть правило для голоса (порты 19294-19344,50000-50100)" },
    { id: "voice", group: "udp", title: "Голосовые каналы", status: "unknown", detail: "голос не проверен: подключитесь к голосовому каналу и подтвердите результат" },
  ],
};
const overview = () => ({
  appVersion: "0.1.0-alpha.2", settings,
  discord: { installed: true, install: { channel: "Stable", root: "", exe: "C:\\Users\\user\\AppData\\Local\\Discord\\app-1.0.9213\\Discord.exe", version: "1.0.9213", custom: false }, running: false, runningPids: [] },
  helper: { available: true, version: "0.1.0-alpha.2" },
  engine: running ? { state: "running", canControl: true, configHash: "ab12", pid: 8812, strategyId: "flowseal-general-alt2", strategyName: "general (ALT2)", startedAt: Math.floor(Date.now() / 1000) - 3600, argsPreview: [] } : { state: "stopped", argsPreview: [] },
  bundle: { version: "1.10.4", winwsVersion: "v72.9", source: "helper", bundles: [{ version: "1.10.4", winwsVersion: "v72.9", active: true, builtin: true, ok: true, problems: [] }] },
  activeStrategy: list().find((s) => s.id === "flowseal-general-alt2"), lastDiagnostics: diag, voice: "notChecked", autostartEnabled: true, autoRunning: false,
  settingsRecovered: false, pendingApply: true,
  ipSets: { mode: settings.ipsetMode, modeUsesSelected: ["selected", "flowsealPlusSelected"].includes(settings.ipsetMode), prepared: ["selected", "flowsealPlusSelected"].includes(settings.ipsetMode) ? 14 : 0, strategyId: "flowseal-general-alt2", strategy: { ipsetRules: 4, ipsetRulesWithUserExclude: 4 }, engineRunsStrategy: running, engineUpToDate: false, applied: false, packs: [{ id: "telegram", ips: 14, effective: 14 }] }, strategyLoadErrors: [], catalogErrors: [],
});

const specsMod = await import("./specs.json");
// Catalog of address packs: built-in snapshot + query flags.
const q = new URLSearchParams(location.search);
const catPacks = (packCatalog as { packs: Record<string, unknown>[] }).packs;
const catIcons = Object.fromEntries((packCatalog as { icons: { name: string; data: string }[] }).icons.map((i) => [i.name, `data:image/png;base64,${i.data}`]));
const catalogView = () => {
  const installed = settings.installedPacks as { id: string; version?: string; domains: string[]; ips?: string[] }[];
  const views = catPacks.map((p) => {
    const inst = installed.find((i) => i.id === p.id) ?? null;
    let state = p.flowsealList ? "flowseal" : inst ? "current" : "available";
    let diff = null;
    if (inst && p.id === "steam" && q.get("cat") === "update") {
      state = "updateAvailable";
      diff = { id: "steam", fromVersion: "1", toVersion: "2", addedDomains: ["steamusercontent.com"], removedDomains: [], addedIps: [], removedIps: [] };
      return { pack: { ...p, version: 2, updated: "2026-11-20" }, state, installed: { ...inst, previous: { version: "0", domains: [], ips: [] } }, diff };
    }
    if (inst && p.id === "twitch") state = "updateAvailable";
    return { pack: p, state, installed: inst, diff };
  });
  return {
    origin: q.get("cat") === "builtin" ? "builtin" : "remote",
    serial: 1793924651,
    published: "2026-11-06T00:24:11Z",
    lastCheck: Math.floor(Date.now() / 1000) - 600,
    lastSuccess: Math.floor(Date.now() / 1000) - 600,
    error: q.get("cat") === "offline" ? "сервер каталога недоступен" : null,
    offline: q.get("cat") === "offline",
    repaired: false,
    packs: views,
    orphans: q.get("cat") === "orphan" ? [{ id: "oldgame", version: "3", domains: ["oldgame.example"], ips: [], title: { ru: "Old Game", en: "Old Game" } }] : [],
    icons: catIcons,
  };
};

const handlers: Record<string, (a: Record<string, unknown>) => unknown> = {
  catalog_get: catalogView,
  catalog_refresh: catalogView,
  pack_diff: () => catalogView().packs.find((v) => v.diff)?.diff,
  get_overview: overview,
  list_strategies: list,
  get_strategy: (a) => details(all.find((s) => s.id === a.id)!),
  preview_strategy: (a) => details(a.strategy as Strategy),
  parse_strategy_json: (a) => ({ ok: true, details: details(JSON.parse(a.text as string)) }),
  get_option_specs: () => specsMod.default,
  get_resources: () => ({ fakes: ["ACTIVE_DISCORD_UDP.bin", "stun.bin", "tls_clienthello_www_google_com.bin", "quic_initial_www_google_com.bin"], lists: ["list-general.txt", "list-google.txt", "list-exclude.txt", "ipset-all.txt"] }),
  run_diagnostics: () => diag,
  diagnostics_history: () => [diag],
  get_logs: () => ["2026-11-05T10:01:12Z INFO Discord Bridge 0.1.0-alpha.2 started", "2026-11-05T10:01:14Z INFO activating strategy flowseal-general-alt2", "2026-11-05T10:01:16Z INFO strategy running, https_ok=true"],
  engine_log: () => ["winws v72.9", "windivert initialized. capture is started."],
  get_settings: () => settings,
  save_settings: (a) => Object.assign(settings, a.settings),
  get_bundle: () => overview().bundle,
  // Preview only: example results (no real releases are implied).
  updates_get: () => ({
    app: { status: "available", current: "0.1.0-alpha.10", latest: "0.1.0-alpha.11", newer: true, installable: false, url: "https://github.com/stardely62-oss/discord-bridge/releases/tag/v0.1.0-alpha.11", changelog: "Пример заметок к релизу (превью).", checkedAt: Math.floor(Date.now() / 1000) },
    flowseal: { status: "unavailable", current: "1.10.4", latest: "1.10.5", newer: true, installable: false, unavailableReason: "Вышел Flowseal 1.10.5, но его пакет ещё не проверен для Discord Bridge: нужен подписанный манифест, опубликованный проектом.", checkedAt: Math.floor(Date.now() / 1000) },
    checkingApp: false,
    checkingFlowseal: false,
  }),
};
export async function invoke<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const h = handlers[cmd];
  if (!h) return undefined as T;
  return h(args) as T;
}
