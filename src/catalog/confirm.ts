import type { PackConfirm, PackDiff, PackView } from "../services/types";

/** Same pack snapshot of the same catalog against the same installed copy. */
export function sameSnapshot(a: PackConfirm | null | undefined, b: PackConfirm | null | undefined): boolean {
  if (!a || !b) return false;
  return a.id === b.id && a.version === b.version && a.serial === b.serial && a.sha256 === b.sha256 && (a.installedSha256 ?? null) === (b.installedSha256 ?? null);
}

export interface Review {
  /** The snapshot shown in the window (fixed when the window opened). */
  shown: PackView;
  /** Diff of exactly `shown` (never one of a newer catalog). */
  diff: PackDiff | null;
  /** The catalog or the installed copy changed after the window opened. */
  stale: boolean;
  /** The confirmation that may be sent, or null (button disabled). */
  confirm: PackConfirm | null;
}

/**
 * State of a review window. `shown` is the view frozen at opening, `live` is
 * the current one from the store, `fetched` an asynchronous `pack_diff`
 * answer. A diff is used only if it belongs to the shown snapshot; once the
 * live view differs, nothing can be confirmed until the user reloads the
 * review explicitly.
 */
export function review(shown: PackView, live: PackView | null, fetched: PackDiff | null, needsDiff: boolean): Review {
  const own = (d: PackDiff | null | undefined) => (d && sameSnapshot(d.confirm, shown.confirm) ? d : null);
  const diff = own(shown.diff) ?? own(fetched);
  const stale = !live || !sameSnapshot(live.confirm, shown.confirm);
  const ready = !needsDiff || diff !== null;
  return { shown, diff, stale, confirm: !stale && ready ? shown.confirm : null };
}
