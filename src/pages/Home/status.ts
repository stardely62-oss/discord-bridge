import type { DiagnosticsReport } from "../../services/types";

export type Tr = (a: string, b: string) => string;
export type Level = "ok" | "warn" | "bad" | "unknown" | "busy";

export function timeAgo(ts: number, tr: Tr) {
  const s = Math.max(0, Math.floor(Date.now() / 1000 - ts));
  if (s < 60) return tr("только что", "just now");
  if (s < 3600) return tr(`${Math.floor(s / 60)} мин назад`, `${Math.floor(s / 60)} min ago`);
  if (s < 86400) return tr(`${Math.floor(s / 3600)} ч назад`, `${Math.floor(s / 3600)} h ago`);
  return new Date(ts * 1000).toLocaleString();
}

/** What «Discord is reachable» means: every planned HTTPS check passed. */
export function access(diag: DiagnosticsReport | null | undefined, activeId: string, running: boolean, tr: Tr): { level: Level; value: string; hint?: string } {
  if (!diag) return { level: "unknown", value: tr("ещё не проверялся", "not checked yet") };
  const https = diag.items.filter((i) => i.group === "https");
  const failed = https.filter((i) => i.status !== "ok").length;
  const stale = diag.engineRunning !== running || (running && diag.strategyId != null && diag.strategyId !== activeId);
  const when = timeAgo(diag.at, tr);
  if (stale) return { level: "unknown", value: tr("нужна повторная проверка", "needs a new check"), hint: tr(`Последняя проверка (${when}) была при другом состоянии обхода.`, `Last check (${when}) was made with a different bypass state.`) };
  if (diag.httpsOk) return { level: "ok", value: tr("открывается", "reachable"), hint: tr(`Проверено ${when}`, `Checked ${when}`) };
  if (https.length > 0 && failed < https.length)
    return {
      level: "warn",
      value: tr(`частично: ${failed} из ${https.length} адресов не отвечают`, `partly: ${failed} of ${https.length} addresses fail`),
      hint: tr(`Проверено ${when}. Подробности — в «Диагностике».`, `Checked ${when}. Details in Diagnostics.`),
    };
  return { level: "bad", value: tr("не открывается", "unreachable"), hint: tr(`Проверено ${when}`, `Checked ${when}`) };
}

