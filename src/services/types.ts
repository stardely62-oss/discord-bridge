// Types mirroring the Rust backend (serde camelCase).

export type Source = "flowseal" | "custom" | "imported";

export type ArgValue = string | { fake: string } | { list: string };

export interface Arg {
  opt: string;
  value?: ArgValue;
}

export interface Rule {
  label?: string;
  enabled: boolean;
  args: Arg[];
}

export interface Strategy {
  schemaVersion: number;
  id: string;
  name: string;
  description: string;
  source: Source;
  engine: "winws";
  origin: { baseId?: string; flowsealVersion?: string; file?: string };
  requirements: { tcpTimestamps: boolean; gameFilter: boolean; userLists: boolean };
  global: Arg[];
  rules: Rule[];
  notes?: string[];
  metadata?: Record<string, string>;
}

export type Severity = "error" | "warning";
export interface Issue {
  severity: Severity;
  path: string;
  message: string;
}

export interface RuleSummary {
  protocol: string;
  tcpPorts?: string | null;
  udpPorts?: string | null;
  l7?: string | null;
  lists: string[];
  domains: string[];
  fakes: string[];
  desync?: string | null;
  other: string[];
}

export type ResourceRef = { kind: "fake" | "list"; name: string };

export interface StrategyDetails {
  strategy: Strategy;
  issues: Issue[];
  summaries: RuleSummary[];
  argsPreview: string[];
  linesPreview: string[];
  resources: ResourceRef[];
}

export interface StrategyListItem {
  id: string;
  name: string;
  description: string;
  source: Source;
  baseId?: string | null;
  flowsealVersion?: string | null;
  ruleCount: number;
  hasUdp: boolean;
  hasVoiceRule: boolean;
  notes: string[];
  errorCount: number;
  /** Saved with validation errors: editable, not startable. */
  draft: boolean;
}

export type OptionKind =
  | { type: "ports" }
  | { type: "flag" }
  | { type: "int"; min: number; max: number }
  | { type: "enum"; values: string[]; multi: boolean }
  | { type: "fake"; allowBang: boolean }
  | { type: "list" }
  | { type: "domains" }
  | { type: "cidrs" }
  | { type: "mods"; keys: string[] }
  | { type: "markers" }
  | { type: "cutoff" }
  | { type: "autoTtl" };

export interface OptionSpec {
  name: string;
  scope: "global" | "rule";
  category: "capture" | "filter" | "lists" | "desync" | "fake" | "split" | "misc";
  kind: OptionKind;
  repeatable: boolean;
  titleRu: string;
  helpRu: string;
  titleEn: string;
  helpEn: string;
}

export type EngineStateName = "stopped" | "running" | "failed";
export interface EngineStatus {
  state: EngineStateName;
  pid?: number | null;
  strategyId?: string | null;
  strategyName?: string | null;
  startedAt?: number | null;
  exitCode?: number | null;
  lastError?: string | null;
  bundleVersion?: string | null;
  argsPreview: string[];
  configHash?: string | null;
  trialActive?: boolean;
  ownerSid?: string | null;
  canControl?: boolean;
}

export interface Conflict {
  kind: string;
  name: string;
  pid?: number | null;
  detail: string;
}

export interface DiscordInstall {
  channel: string;
  root: string;
  exe: string;
  updateExe?: string | null;
  version?: string | null;
  custom: boolean;
}

export interface DiscordStatus {
  installed: boolean;
  install?: DiscordInstall | null;
  running: boolean;
  runningPids: number[];
  customPathError?: string | null;
}

export type CheckStatus = "ok" | "warning" | "error" | "unknown" | "skipped";
export interface CheckItem {
  id: string;
  group: string;
  title: string;
  status: CheckStatus;
  detail: string;
  hint?: string | null;
  durationMs?: number | null;
}

export type VoiceState = "noUdpRules" | "notChecked" | "userConfirmed" | "userReportedBroken";

export interface DiagnosticsReport {
  at: number;
  strategyId?: string | null;
  engineRunning: boolean;
  httpsOk?: boolean | null;
  voice: VoiceState;
  items: CheckItem[];
}

export type GameFilterMode = "disabled" | "all" | "tcp" | "udp";
/** What ipset-all.txt contains: nothing, the selected IPs (manual + catalog
 * packs), Flowseal's full list, both, or every address (risky). */
export type IpsetMode = "none" | "selected" | "flowseal" | "flowsealPlusSelected" | "any";

export interface Localized {
  ru: string;
  en: string;
}

export interface InstalledPack {
  id: string;
  /** Catalog version of the snapshot ("3"); alpha.8 stored a date. */
  version?: string | null;
  domains: string[];
  /** IP addresses / subnets; applied only in "selected" ipset modes. */
  ips?: string[];
  /** Name at install time (kept if the pack leaves the catalog). */
  title?: Localized | null;
  /** The verified snapshot this one replaced (rollback point). */
  previous?: { version?: string | null; domains: string[]; ips: string[] } | null;
}

export type PackCategory = "messenger" | "voice" | "game" | "launcher" | "video" | "streaming" | "service" | "other";

export interface CatalogPack {
  schema: number;
  id: string;
  version: number;
  updated: string;
  name: Localized;
  description: Localized;
  category: PackCategory;
  source: "official" | "community";
  icon?: string | null;
  domains: string[];
  ips: string[];
  provenance: { kind: "domains" | "ips" | "icon"; url: string; note?: string | null }[];
  notes?: Localized | null;
  partialCoverage: boolean;
  requires: { selectedIps: boolean; gameFilter: boolean };
  flowsealList?: string | null;
  minAppVersion?: string | null;
}

/** flowseal: part of Flowseal lists; available: not installed; current: same
 * addresses as the catalog; updateAvailable: newer catalog version;
 * differs: snapshot differs from the catalog version. */
export type PackState = "flowseal" | "available" | "current" | "updateAvailable" | "differs";

/** Exactly what the user saw: one pack snapshot of one catalog publication
 * (SHA-256 of the canonical pack JSON, computed in Rust) and the installed
 * copy the preview was computed against. Passed back unchanged to install. */
export interface PackConfirm {
  id: string;
  version: number;
  serial: number;
  sha256: string;
  installedSha256?: string | null;
}

export interface PackDiff {
  id: string;
  confirm: PackConfirm;
  fromVersion?: string | null;
  toVersion: string;
  addedDomains: string[];
  removedDomains: string[];
  addedIps: string[];
  removedIps: string[];
}

export interface PackView {
  pack: CatalogPack;
  state: PackState;
  installed?: InstalledPack | null;
  diff?: PackDiff | null;
  /** Token for installing exactly this shown snapshot. */
  confirm: PackConfirm;
}

export interface CatalogView {
  /** remote: verified published catalog (cached); builtin: shipped with the app. */
  origin: "remote" | "builtin";
  serial?: number | null;
  published?: string | null;
  lastCheck?: number | null;
  lastSuccess?: number | null;
  error?: string | null;
  offline: boolean;
  repaired: boolean;
  /** Serial of the catalog in use (remote or built-in). */
  dataSerial: number;
  packs: PackView[];
  /** Installed packs that are no longer in the catalog. */
  orphans: InstalledPack[];
  icons: Record<string, string>;
}

export interface Settings {
  schemaVersion: number;
  language: "ru" | "en";
  theme: "system" | "dark" | "light";
  onboardingDone: boolean;
  launchAtStartup: boolean;
  startMinimized: boolean;
  autoEnableBypass: boolean;
  autoLaunchDiscord: boolean;
  closeBehavior: "tray" | "exit";
  stopBypassOnExit: boolean;
  trayNotifications: boolean;
  discordPath?: string | null;
  activeStrategyId?: string | null;
  lastWorkingStrategyId?: string | null;
  bypassEnabled: boolean;
  gameFilter: { mode: GameFilterMode; tcpRange: string; udpRange: string };
  ipsetMode: IpsetMode;
  /** The user's own entries only. */
  userLists: { general: string[]; exclude: string[]; ipsetExclude: string[]; ipsetInclude: string[] };
  /** Catalog packs, stored apart from userLists and merged only for the engine. */
  installedPacks: InstalledPack[];
  keepLogs: boolean;
  checkUpdatesOnStart: boolean;
  /** Telegram WS Proxy (independent of the bypass; the secret is never here). */
  telegramProxy: TelegramProxySettings;
  autoCandidates: string[];
  autoMaxAttempts: number;
  voiceConfirmations: Record<string, { at: number; works: boolean; strategyId: string }>;
  launchMode: "withBypass" | "withoutBypass";
}

export interface BundleSummary {
  version: string;
  winwsVersion: string;
  active: boolean;
  builtin: boolean;
  ok: boolean;
  problems: string[];
}

export interface BundleState {
  version?: string | null;
  winwsVersion?: string | null;
  source: string;
  bundles: BundleSummary[];
  error?: string | null;
}

export interface Overview {
  appVersion: string;
  settings: Settings;
  discord: DiscordStatus;
  helper: { available: boolean; version?: string | null; error?: string | null };
  engine?: EngineStatus | null;
  bundle: BundleState;
  activeStrategy?: StrategyListItem | null;
  lastDiagnostics?: DiagnosticsReport | null;
  voice: VoiceState;
  autostartEnabled: boolean;
  autoRunning: boolean;
  settingsRecovered: boolean;
  strategyLoadErrors: string[];
  catalogErrors: string[];
  pendingApply: boolean;
  ipSets: IpSetStatus;
}

export interface IpSetStatus {
  mode: IpsetMode;
  modeUsesSelected: boolean;
  /** Selected IP entries written to ipset-all.txt with the current settings. */
  prepared: number;
  strategyId?: string | null;
  strategy: { ipsetRules: number; ipsetRulesWithUserExclude: number };
  engineRunsStrategy: boolean;
  engineUpToDate: boolean;
  /** Confirmed: the running engine matches against these IPs. */
  applied: boolean;
  packs: { id: string; ips: number; effective: number }[];
  /** The IP list cannot be built (limit exceeded); starts fail with this message. */
  error?: string | null;
}

export interface ActivationResult {
  ok: boolean;
  requestedId?: string | null;
  outcome: "running" | "rolledBack" | "rollbackFailed" | "failed" | "prepareFailed" | "conflicts" | "invalid" | "helperUnavailable" | "";
  message?: string | null;
  conflicts: Conflict[];
  issues: Issue[];
  httpsOk?: boolean | null;
  httpsItems: CheckItem[];
  rolledBackTo?: string | null;
  rollbackError?: string | null;
  status?: EngineStatus | null;
}

export interface LaunchResult {
  launched: boolean;
  discord?: DiscordStatus | null;
  bypass?: ActivationResult | null;
  bypassWasRunning: boolean;
  message: string;
}

export interface TcpInfo {
  current?: boolean | null;
  changedByUs: boolean;
  message: string;
}

export interface CompatEntry {
  id: string;
  name: string;
  problems: string[];
}

export interface AutoProgress {
  running: boolean;
  attempt: number;
  maxAttempts: number;
  current?: string | null;
  log: string[];
  result?: string | null;
  successId?: string | null;
  restored?: boolean | null;
}

export interface ImportNote {
  line: number;
  text: string;
  reason: string;
}

export interface ImportPreview {
  token: string;
  format: string;
  strategy: Strategy;
  notes: ImportNote[];
  unsupported: ImportNote[];
  issues: Issue[];
  migratedFrom?: number | null;
}

export type DiffOp = "same" | "added" | "removed";
export interface DiffLine {
  op: DiffOp;
  text: string;
}
export interface FieldChange {
  field: string;
  left: string;
  right: string;
}
export interface StrategyDiff {
  identical: boolean;
  meta: FieldChange[];
  global: DiffLine[];
  rules: { leftIndex?: number | null; rightIndex?: number | null; changes: FieldChange[]; args: DiffLine[] }[];
  orderChanged: boolean;
}

/** upToDate · available (Flowseal: and installable) · unavailable (newer Flowseal, not installable) · error (nothing known). */
export type UpdateStatus = "upToDate" | "available" | "unavailable" | "error";

export interface UpdateInfo {
  status: UpdateStatus;
  current?: string | null;
  latest?: string | null;
  newer: boolean;
  changelog?: string | null;
  /** Page of exactly the latest release (built from the validated tag). */
  url?: string | null;
  error?: string | null;
  installable: boolean;
  unavailableReason?: string | null;
  /** Unix seconds. */
  checkedAt: number;
}

export interface UpdatesView {
  app?: UpdateInfo | null;
  flowseal?: UpdateInfo | null;
  checkingApp: boolean;
  checkingFlowseal: boolean;
}

/** activated · rolledBack (previous version restored) · rollbackFailed · refused (nothing changed) · installFailed (nothing changed). */
export interface FlowsealAction {
  ok: boolean;
  outcome: "activated" | "rolledBack" | "rollbackFailed" | "refused" | "installFailed";
  message: string;
  version: string;
}

export interface EditorParse {
  ok: boolean;
  details?: StrategyDetails | null;
  error?: string | null;
  line?: number | null;
  column?: number | null;
}

// ---------------- Service cards (Rust: bridge_core::services) ----------------

/** unknown: no verified source · known: in a verified source · prepared: the selected strategy has rules · applied: the running engine uses exactly this configuration. Never "works": that needs a real check. */
export type ApplyState = "unknown" | "known" | "prepared" | "applied";
export type CardKind = "flowseal" | "installed" | "available";
export type ListKind = "hostlist" | "hostlistExclude" | "ipset" | "ipsetExclude" | "ipsetSource" | "other";

export interface CardSummary {
  id: string;
  kind: CardKind;
  domains: number;
  inFlowseal: number;
  excluded: number;
  shared: number;
  ips: number;
  rules: number;
  extraRules: number;
  state: ApplyState;
}

export interface SourceFile {
  name: string;
  kind: ListKind;
  present: boolean;
  lines: number;
  entries: number;
}

export interface ServicesOverview {
  bundleVersion?: string | null;
  sources: SourceFile[];
  errors: string[];
  cards: CardSummary[];
}

export interface LineRef {
  list: string;
  line: number;
  raw: string;
  parent: boolean;
}

export interface DomainInfo {
  domain: string;
  group: string;
  sources: LineRef[];
  excludedBy: LineRef[];
  sharedWith: string[];
}

export interface RuleInfo {
  index: number;
  label?: string | null;
  enabled: boolean;
  tcp?: string | null;
  udp?: string | null;
  hostlists: string[];
  hostlistDomains: string[];
  hostlistExcludes: string[];
  ipsets: string[];
  ipsetExcludes: string[];
  l7: string[];
  desync?: string | null;
}

export interface StrategyFacts {
  id: string;
  name: string;
  wfTcp?: string | null;
  wfUdp?: string | null;
  rules: RuleInfo[];
}

export type RuleMatch = { by: "hostlist"; value: string } | { by: "domains"; value: string[] } | { by: "protocol"; value: string[] };

export interface CardRule {
  index: number;
  matched: RuleMatch;
  /** Service addresses this rule really handles (after its own exclusions); protocol rules: 0. */
  covers: number;
}

export interface IpsetFacts {
  mode: IpsetMode;
  runtimeEntries?: number | null;
  runtimePlaceholder: boolean;
  runtimeAll: boolean;
  sourceEntries?: number | null;
  rules: number[];
}

export interface CardDetails {
  summary: CardSummary;
  domains: DomainInfo[];
  serviceIps: string[];
  rules: CardRule[];
  strategy?: StrategyFacts | null;
  ipset: IpsetFacts;
  sources: SourceFile[];
  bundleVersion?: string | null;
  runningStrategy?: string | null;
  pendingApply: boolean;
}

export interface ListPage {
  name: string;
  total: number;
  matched: number;
  offset: number;
  /** [1-based line number, original line] */
  lines: [number, string][];
}

// ---------------- Telegram WS Proxy (Rust: bridge_telegram, src-tauri/src/telegram.rs) ----------------

export interface TelegramProxySettings {
  autostart: boolean;
  port: number;
  /** Cloudflare relays when a direct WebSocket fails (off by default). */
  fallbackCfproxy: boolean;
  h2Media: boolean;
  cfproxyDomains: string[];
  cfproxyWorkerDomains: string[];
}

/** From the real child process: never a UI flag. `connected` only with an authenticated client relayed to Telegram. */
export type TelegramState = "notInstalled" | "stopped" | "starting" | "listening" | "connected" | "error";

export interface TelegramEngineStats {
  connections_total: number;
  connections_active: number;
  connections_ws: number;
  connections_h2: number;
  connections_tcp_fallback: number;
  connections_cfproxy: number;
  connections_fronting: number;
  connections_bad: number;
  connections_masked: number;
  ws_errors: number;
  bytes_up: number;
  bytes_down: number;
}

export interface TelegramStatus {
  state: TelegramState;
  version?: string | null;
  host: string;
  port: number;
  pid?: number | null;
  startedAt?: number | null;
  listening: boolean;
  clientsActive: number;
  relayed: number;
  dataFromTelegram: boolean;
  stats: TelegramEngineStats;
  fallbackCfproxy: boolean;
  errorCode?: string | null;
  lastError?: string | null;
  exitCode?: number | null;
}

export interface TelegramView {
  status: TelegramStatus;
  component: { installed: boolean; version?: string | null; upstreamCommit?: string | null; problem?: string | null };
  settings: TelegramProxySettings;
  /** Running with older settings: a restart applies the saved ones. */
  pendingRestart: boolean;
}

export interface TelegramDiagItem {
  id: string;
  status: "ok" | "warn" | "bad" | "unknown";
  title: string;
  detail: string;
}
