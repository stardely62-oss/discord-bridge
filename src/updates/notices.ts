import type { UpdateInfo, UpdatesView } from "../services/types";

/**
 * Update notices shown once per session and version:
 * - `app` — a newer Bridge release (open its exact page);
 * - `flowsealReady` — a newer Flowseal with a verified signed manifest;
 * - `flowsealUnavailable` — a newer official Flowseal release that Discord
 *   Bridge cannot install yet (no verified signed manifest). No install action.
 * Failed checks and "nothing new" never produce a notice.
 */
export type NoticeKind = "app" | "flowsealReady" | "flowsealUnavailable";

export interface Notice {
  key: string;
  kind: NoticeKind;
  version: string;
  reason?: string | null;
}

export function noticesFor(view: UpdatesView | null | undefined, seen: ReadonlySet<string>): Notice[] {
  const out: Notice[] = [];
  const app = view?.app;
  if (app?.status === "available" && app.newer && app.latest) {
    out.push({ key: `app:${app.latest}`, kind: "app", version: app.latest });
  }
  const fs = view?.flowseal;
  if (fs?.newer && fs.latest) {
    if (fs.status === "available" && fs.installable) {
      out.push({ key: `flowsealReady:${fs.latest}`, kind: "flowsealReady", version: fs.latest });
    } else if (fs.status === "unavailable") {
      out.push({ key: `flowsealUnavailable:${fs.latest}`, kind: "flowsealUnavailable", version: fs.latest, reason: fs.unavailableReason });
    }
  }
  return out.filter((n) => !seen.has(n.key));
}

/** What the Updates page says about one component. */
export type CheckState = "notChecked" | "checking" | "upToDate" | "available" | "unavailable" | "error";

export function checkState(info: UpdateInfo | null | undefined, checking: boolean): CheckState {
  if (checking) return "checking";
  if (!info) return "notChecked";
  return info.status;
}

/** Target for `open_external`: the exact release page, or null. */
export function appReleaseTarget(info: UpdateInfo | null | undefined): string | null {
  return info?.status === "available" && info.newer && info.latest ? `app-release:${info.latest}` : null;
}
