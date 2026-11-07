import { describe, expect, it } from "vitest";
import type { UpdateInfo, UpdatesView } from "../src/services/types";
import { appReleaseTarget, checkState, noticesFor } from "../src/updates/notices";

const info = (over: Partial<UpdateInfo>): UpdateInfo => ({ status: "upToDate", newer: false, installable: false, checkedAt: 1, ...over });
const view = (app?: Partial<UpdateInfo>, flowseal?: Partial<UpdateInfo>): UpdatesView => ({
  app: app ? info(app) : null,
  flowseal: flowseal ? info(flowseal) : null,
  checkingApp: false,
  checkingFlowseal: false,
});
const none = new Set<string>();

describe("update notices", () => {
  it("a newer Bridge gives one notice with its exact release", () => {
    const v = view({ status: "available", newer: true, latest: "0.1.0-alpha.11", url: "https://github.com/stardely62-oss/discord-bridge/releases/tag/v0.1.0-alpha.11" });
    expect(noticesFor(v, none)).toEqual([{ key: "app:0.1.0-alpha.11", kind: "app", version: "0.1.0-alpha.11" }]);
    expect(appReleaseTarget(v.app)).toBe("app-release:0.1.0-alpha.11");
  });

  it("nothing new, failed checks and missing results give no notice", () => {
    expect(noticesFor(view({ status: "upToDate", latest: "0.1.0-alpha.10" }, { status: "upToDate", latest: "1.10.4" }), none)).toEqual([]);
    expect(noticesFor(view({ status: "error", error: "нет соединения" }, { status: "error" }), none)).toEqual([]);
    expect(noticesFor(null, none)).toEqual([]);
    expect(appReleaseTarget(info({ status: "error" }))).toBeNull();
  });

  it("an installable Flowseal and a not-yet-installable one are different notices", () => {
    const ready = noticesFor(view(undefined, { status: "available", newer: true, installable: true, latest: "1.10.5" }), none);
    expect(ready.map((n) => n.kind)).toEqual(["flowsealReady"]);
    const notYet = noticesFor(view(undefined, { status: "unavailable", newer: true, installable: false, latest: "1.10.5", unavailableReason: "нужен подписанный манифест" }), none);
    expect(notYet).toEqual([{ key: "flowsealUnavailable:1.10.5", kind: "flowsealUnavailable", version: "1.10.5", reason: "нужен подписанный манифест" }]);
    // "available" without a verified manifest is never offered for installation.
    expect(noticesFor(view(undefined, { status: "available", newer: true, installable: false, latest: "1.10.5" }), none)).toEqual([]);
  });

  it("a notice dismissed for a version is not repeated in the session, a newer version is", () => {
    const v = view({ status: "available", newer: true, latest: "0.1.0-alpha.11" }, { status: "unavailable", newer: true, latest: "1.10.5" });
    const seen = new Set(noticesFor(v, none).map((n) => n.key));
    expect(noticesFor(v, seen)).toEqual([]);
    // Re-checking (another event, another tab) yields the same keys → still nothing.
    expect(noticesFor({ ...v }, seen)).toEqual([]);
    const later = view({ status: "available", newer: true, latest: "0.1.0-alpha.12" }, { status: "available", newer: true, installable: true, latest: "1.10.5" });
    expect(noticesFor(later, seen).map((n) => n.key)).toEqual(["app:0.1.0-alpha.12", "flowsealReady:1.10.5"]);
  });

  it("the Updates page distinguishes checking, not checked and each result", () => {
    expect(checkState(null, false)).toBe("notChecked");
    expect(checkState(info({ status: "upToDate" }), true)).toBe("checking");
    for (const s of ["upToDate", "available", "unavailable", "error"] as const) expect(checkState(info({ status: s }), false)).toBe(s);
  });
});
