import { describe, expect, it } from "vitest";
import type { TelegramState, TelegramView } from "../src/services/types";
import { actions, clientText, maskedLink, portNote, routeText, stateLabel, validPort } from "../src/telegram/view";

const ru = (a: string) => a;
const en = (_: string, b: string) => b;
const zero = { connections_total: 0, connections_active: 0, connections_ws: 0, connections_h2: 0, connections_tcp_fallback: 0, connections_cfproxy: 0, connections_fronting: 0, connections_bad: 0, connections_masked: 0, ws_errors: 0, bytes_up: 0, bytes_down: 0, relays_active: 0, relays_total: 0 };
const view = (state: TelegramState, over: Partial<TelegramView["status"]> = {}, installed = true, fallback = false): TelegramView => ({
  status: { state, host: "127.0.0.1", port: 1443, listening: state === "listening" || state === "connected", clientsActive: 0, relaysActive: state === "connected" ? 1 : 0, relayed: 0, dataFromTelegram: false, stats: { ...zero }, fallbackCfproxy: fallback, ...over },
  component: { installed, version: "1.11.4" },
  settings: { autostart: false, port: 1443, fallbackCfproxy: fallback, h2Media: true, cfproxyDomains: [], cfproxyWorkerDomains: [] },
  pendingRestart: false,
  connectionPort: 1443,
});

describe("Telegram proxy card", () => {
  it("never claims that Telegram, media or calls work", () => {
    const all: TelegramState[] = ["notInstalled", "stopped", "starting", "listening", "connected", "error"];
    for (const s of all) {
      for (const t of [ru, en]) {
        const text = stateLabel(s, t).text.toLowerCase();
        expect(text).not.toMatch(/работа|звонк|доступ|works|calls|reachable/);
      }
    }
    expect(stateLabel("listening", ru).text).toBe("ждёт подключения");
    expect(stateLabel("connected", en).text).toBe("Telegram connected");
  });

  it("offers only the actions valid in each state", () => {
    expect(actions(view("notInstalled", {}, false))).toEqual(["diagnostics", "settings"]);
    expect(actions(view("stopped"))).toContain("start");
    expect(actions(view("stopped"))).not.toContain("stop");
    expect(actions(view("error"))).toContain("start");
    expect(actions(view("starting"))).toEqual(["stop", "diagnostics", "settings"]);
    for (const s of ["listening", "connected"] as const) {
      const a = actions(view(s));
      expect(a).toContain("stop");
      expect(a).toContain("connect");
      expect(a).not.toContain("start");
    }
    expect(actions(null)).toEqual([]);
  });

  it("the link on screen never shows the secret", () => {
    const m = maskedLink(1443);
    expect(m.startsWith("tg://proxy?server=127.0.0.1&port=1443&secret=dd")).toBe(true);
    expect(m).not.toMatch(/[0-9a-f]{32}/);
  });

  it("route and client texts come only from engine counters", () => {
    expect(routeText(view("listening"), ru)).toBe("прямой WebSocket, без резервных узлов");
    expect(routeText(view("listening", {}, true, true), ru)).toContain("резерв через Cloudflare разрешён");
    expect(routeText(view("connected", { stats: { ...zero, connections_ws: 2, connections_cfproxy: 1 } }), ru)).toBe("WebSocket напрямую, через Cloudflare");
    expect(clientText(view("listening"), ru)).toBe("не подключён");
    expect(clientText(view("connected", { clientsActive: 3, relaysActive: 3, relayed: 3 }), ru)).toContain("3");
    expect(clientText(view("listening", { clientsActive: 1, stats: { ...zero, connections_bad: 2 } }), ru)).toContain("неверным секретом");
    expect(clientText(view("stopped"), ru)).toBe("—");
  });

  it("ports outside 1024–65535 are refused", () => {
    expect(validPort(1443)).toBe(true);
    for (const p of [0, 80, 1023, 65536, 1443.5, NaN]) expect(validPort(p)).toBe(false);
  });
});

describe("Telegram proxy regressions", () => {
  it("past successes and unauthorised clients never read as connected", () => {
    // Earlier relays, a client with a wrong secret open now, nothing relayed now.
    const v = view("listening", { clientsActive: 1, relaysActive: 0, relayed: 5, dataFromTelegram: true, stats: { ...zero, connections_ws: 5, connections_bad: 1, connections_active: 1 } });
    expect(clientText(v, ru)).not.toMatch(/^подключён/);
    expect(clientText(view("listening", { relaysActive: 0, relayed: 5 }), ru)).toBe("не подключён (ранее за сеанс: 5)");
    expect(clientText(view("listening", { clientsActive: 2, relaysActive: 0 }), ru)).toBe("не подключён");
  });

  it("a changed but not applied port: the running port is shown and explained", () => {
    const v = { ...view("listening"), pendingRestart: true, connectionPort: 1443, settings: { ...view("listening").settings, port: 2443 } };
    expect(portNote(v, ru)).toContain("работает на порту 1443");
    expect(portNote(v, ru)).toContain("2443");
    expect(portNote(view("listening"), ru)).toBeNull();
  });
});
