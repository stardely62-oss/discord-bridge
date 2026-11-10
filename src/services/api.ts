// Typed wrappers over Tauri commands. Every function maps 1:1 to a Rust command.
import { invoke } from "@tauri-apps/api/core";
import type {
  ActivationResult,
  ProviderHints,
  CatalogView,
  PackConfirm,
  PackDiff,
  AutoProgress,
  BundleState,
  CompatEntry,
  LaunchResult,
  TcpInfo,
  DiagnosticsReport,
  DiscordStatus,
  EditorParse,
  ImportPreview,
  OptionSpec,
  Overview,
  Settings,
  Strategy,
  StrategyDetails,
  StrategyDiff,
  StrategyListItem,
  UpdateInfo,
  TelegramView,
  TelegramDiagItem,
  ServicesOverview,
  CardDetails,
  ListPage,
  UpdatesView,
  FlowsealAction,
} from "./types";

export const api = {
  overview: () => invoke<Overview>("get_overview"),

  /** The single launch scenario: `preference` (saved setting), `withBypass`, `withoutBypass`. */
  launchDiscord: (mode: "preference" | "withBypass" | "withoutBypass" = "preference") => invoke<LaunchResult>("launch_discord", { mode }),
  discordStatus: () => invoke<DiscordStatus>("discord_status"),
  setDiscordPath: (path: string | null) => invoke<DiscordStatus>("set_discord_path", { path }),
  openExternal: (target: string) => invoke<void>("open_external", { target }),

  listStrategies: () => invoke<StrategyListItem[]>("list_strategies"),
  getStrategy: (id: string) => invoke<StrategyDetails>("get_strategy", { id }),
  previewStrategy: (strategy: Strategy) => invoke<StrategyDetails>("preview_strategy", { strategy }),
  parseStrategyJson: (text: string) => invoke<EditorParse>("parse_strategy_json", { text }),
  saveStrategy: (strategy: Strategy) => invoke<StrategyDetails>("save_strategy", { strategy }),
  createStrategy: (name: string) => invoke<Strategy>("create_strategy", { name }),
  duplicateStrategy: (id: string, name?: string) => invoke<Strategy>("duplicate_strategy", { id, name: name ?? null }),
  renameStrategy: (id: string, name: string) => invoke<Strategy>("rename_strategy", { id, name }),
  deleteStrategy: (id: string) => invoke<void>("delete_strategy", { id }),
  restoreOriginal: (id: string) => invoke<Strategy>("restore_original", { id }),
  compare: (left: string, right: string) => invoke<StrategyDiff>("compare_strategies", { left, right }),
  importPreview: (path: string) => invoke<ImportPreview>("import_preview", { path }),
  importCommit: (token: string, name: string | null, acceptPartial: boolean) => invoke<Strategy>("import_commit", { token, name, acceptPartial }),
  createDiscordOnly: () => invoke<Strategy>("create_discord_only"),
  exportStrategy: (id: string, format: "json" | "toml", path: string) => invoke<void>("export_strategy", { id, format, path }),
  catalogGet: () => invoke<CatalogView>("catalog_get"),
  /** Checks the signed remote catalog now (Rust side; never from React). */
  catalogRefresh: () => invoke<CatalogView>("catalog_refresh"),
  /** Installs or updates exactly the snapshot the user confirmed; Rust
   * rejects it (and changes nothing) if the catalog or the installed copy
   * changed since. */
  packInstall: (confirm: PackConfirm) => invoke<CatalogView>("pack_install", { confirm }),
  packRemove: (id: string) => invoke<CatalogView>("pack_remove", { id }),
  packRollback: (id: string) => invoke<CatalogView>("pack_rollback", { id }),
  packDiff: (id: string) => invoke<PackDiff>("pack_diff", { id }),
  /** Service cards: catalog facts + the original Flowseal lists + the running configuration. */
  servicesOverview: () => invoke<ServicesOverview>("services_overview"),
  serviceDetails: (id: string) => invoke<CardDetails>("service_details", { id }),
  /** A searchable page of an original Flowseal file of the active version. */
  flowsealListPage: (name: string, query: string, offset: number, limit: number) => invoke<ListPage>("flowseal_list_page", { name, query, offset, limit }),
  /** The whole original file, unchanged (for copying). */
  flowsealListText: (name: string) => invoke<string>("flowseal_list_text", { name }),
  readListFile: (path: string) => invoke<string[]>("read_list_file", { path }),
  writeListFile: (path: string, entries: string[]) => invoke<void>("write_list_file", { path, entries }),
  optionSpecs: () => invoke<OptionSpec[]>("get_option_specs"),
  resources: () => invoke<{ fakes: string[]; lists: string[] }>("get_resources"),

  bypassEnable: (id: string | null, ignoreConflicts = false) => invoke<ActivationResult>("bypass_enable", { id, ignoreConflicts }),
  bypassDisable: () => invoke<void>("bypass_disable"),
  selectStrategy: (id: string) => invoke<void>("select_strategy", { id }),
  autoStart: () => invoke<AutoProgress>("auto_start"),
  autoCancel: () => invoke<void>("auto_cancel"),
  engineLog: () => invoke<string[]>("engine_log"),

  runDiagnostics: () => invoke<DiagnosticsReport>("run_diagnostics"),
  confirmVoice: (strategyId: string, works: boolean) => invoke<void>("confirm_voice", { strategyId, works }),
  diagnosticsHistory: () => invoke<DiagnosticsReport[]>("diagnostics_history"),
  setTcpTimestamps: (enable: boolean, userConfirmed: boolean) => invoke<TcpInfo>("set_tcp_timestamps", { enable, userConfirmed }),
  restoreTcpTimestamps: () => invoke<TcpInfo>("restore_tcp_timestamps"),
  logs: (lines?: number) => invoke<string[]>("get_logs", { lines: lines ?? null }),
  clearLogs: () => invoke<void>("clear_logs"),
  exportReport: (path: string) => invoke<void>("export_report", { path }),

  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  providerHints: () => invoke<ProviderHints>("provider_hints"),

  flowsealCheck: () => invoke<UpdateInfo>("flowseal_check_update"),
  flowsealInstall: (version: string) => invoke<FlowsealAction>("flowseal_install", { version }),
  flowsealSetActive: (version: string) => invoke<FlowsealAction>("flowseal_set_active", { version }),
  flowsealCompat: (version: string) => invoke<CompatEntry[]>("flowseal_compat", { version }),
  appCheckUpdate: () => invoke<UpdateInfo>("app_check_update"),
  /** Last results of both checks (the startup check may have run while hidden). */
  updatesGet: () => invoke<UpdatesView>("updates_get"),
  bundle: () => invoke<BundleState>("get_bundle"),
  quit: () => invoke<void>("quit_app"),
  // Telegram WS Proxy — independent of the bypass and of the helper service.
  telegramGet: () => invoke<TelegramView>("telegram_get"),
  telegramStart: () => invoke<TelegramView>("telegram_start"),
  telegramStop: () => invoke<TelegramView>("telegram_stop"),
  telegramRestart: () => invoke<TelegramView>("telegram_restart"),
  /** The tg:// link contains the secret: only on an explicit user action. */
  telegramLink: () => invoke<string>("telegram_link"),
  telegramOpen: () => invoke<{ opened: boolean; reason?: string | null }>("telegram_open"),
  telegramRegenerateSecret: () => invoke<TelegramView>("telegram_regenerate_secret"),
  telegramDiagnostics: () => invoke<TelegramDiagItem[]>("telegram_diagnostics"),
};

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}
