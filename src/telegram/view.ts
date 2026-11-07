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

/** Client state: only with evidence from the engine. */
export function clientText(v: TelegramView, tr: Tr): string {
  const s = v.status;
  if (s.state === "connected") return tr(`подключён (соединений: ${s.clientsActive})`, `connected (${s.clientsActive} connections)`);
  if (s.stats.connections_bad > 0 && s.relayed === 0) return tr("подключения с неверным секретом — обновите ссылку в Telegram", "connections with a wrong secret — update the link in Telegram");
  if (s.state === "listening") return tr("не подключён", "not connected");
  return "—";
}

export function validPort(p: number): boolean {
  return Number.isInteger(p) && p >= 1024 && p <= 65535;
}
