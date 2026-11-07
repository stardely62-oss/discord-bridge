// Display rules of the Telegram WS Proxy card. Everything comes from Rust
// (the real child process); nothing here claims that Telegram, media or
// calls work.
import type { TelegramState, TelegramView } from "../services/types";

export type Tr = (ru: string, en: string) => string;
export type Level = "ok" | "warn" | "bad" | "unknown" | "busy";

export function stateLabel(state: TelegramState, tr: Tr): { level: Level; text: string } {
  switch (state) {
    case "notInstalled":
      return { level: "bad", text: tr("не установлен", "not installed") };
    case "stopped":
      return { level: "unknown", text: tr("выключен", "off") };
    case "starting":
      return { level: "busy", text: tr("запускается", "starting") };
    case "listening":
      return { level: "ok", text: tr("ждёт подключения", "listening") };
    case "connected":
      return { level: "ok", text: tr("Telegram подключён", "Telegram connected") };
    case "error":
      return { level: "bad", text: tr("ошибка", "error") };
  }
}

export type Action = "start" | "stop" | "restart" | "connect" | "copyLink" | "diagnostics" | "settings";

/** Only the actions that make sense in the current state. */
export function actions(v: TelegramView | null | undefined): Action[] {
  if (!v) return [];
  const s = v.status.state;
  if (s === "notInstalled" || !v.component.installed) return ["diagnostics", "settings"];
  if (s === "stopped" || s === "error") return ["start", "copyLink", "diagnostics", "settings"];
  if (s === "starting") return ["stop", "diagnostics", "settings"];
  return ["stop", "restart", "connect", "copyLink", "diagnostics", "settings"];
}

/** Link shown on screen with the secret hidden (the full link only via copy/open). */
export function maskedLink(port: number): string {
  return `tg://proxy?server=127.0.0.1&port=${port}&secret=dd••••••••••••••••`;
}

/** What is actually known about the route, from counters of the engine. */
export function routeText(v: TelegramView, tr: Tr): string {
  const x = v.status.stats;
  const used: string[] = [];
  if (x.connections_ws) used.push(tr("WebSocket напрямую", "direct WebSocket"));
  if (x.connections_tcp_fallback) used.push(tr("TCP напрямую", "direct TCP"));
  if (x.connections_cfproxy || x.connections_h2) used.push(tr("через Cloudflare", "via Cloudflare"));
  if (used.length) return used.join(", ");
  return v.settings.fallbackCfproxy ? tr("прямой WebSocket, резерв через Cloudflare разрешён", "direct WebSocket, Cloudflare fallback allowed") : tr("прямой WebSocket, без резервных узлов", "direct WebSocket, no relays");
}

/** Client state now: only authorised connections relayed to Telegram count. */
export function clientText(v: TelegramView, tr: Tr): string {
  const s = v.status;
  if (s.relaysActive > 0) return tr(`подключён (соединений: ${s.relaysActive})`, `connected (${s.relaysActive} connections)`);
  if (s.state !== "listening" && s.state !== "connected") return "—";
  if (s.clientsActive > 0 && s.stats.connections_bad > 0) return tr("подключения с неверным секретом — обновите ссылку в Telegram", "connections with a wrong secret — update the link in Telegram");
  if (s.relayed > 0) return tr(`не подключён (ранее за сеанс: ${s.relayed})`, `not connected (earlier this run: ${s.relayed})`);
  return tr("не подключён", "not connected");
}

/** Saved port differs from the running one: the link uses the running port. */
export function portNote(v: TelegramView, tr: Tr): string | null {
  if (!v.pendingRestart || v.connectionPort === v.settings.port) return null;
  return tr(`Прокси работает на порту ${v.connectionPort}; сохранённый порт ${v.settings.port} применится после перезапуска. Ссылка сейчас указывает на ${v.connectionPort}.`, `The proxy runs on port ${v.connectionPort}; the saved port ${v.settings.port} applies after a restart. The link points at ${v.connectionPort} for now.`);
}

export function validPort(p: number): boolean {
  return Number.isInteger(p) && p >= 1024 && p <= 65535;
}
