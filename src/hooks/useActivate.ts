import { useCallback } from "react";
import { confirmDialog } from "../components/ui/confirm";
import { api, errorText } from "../services/api";
import { useApp, useTr } from "../stores/app";

/** «Не включать» in the TCP timestamps question: remembered on this computer. */
const TS_DECLINED = "bridge.tcpTimestamps.declined";
const tsDeclined = () => {
  try {
    return localStorage.getItem(TS_DECLINED) === "1";
  } catch {
    return false;
  }
};
export const declineTs = () => {
  try {
    localStorage.setItem(TS_DECLINED, "1");
  } catch {
    // Not remembered: the question comes back next time, nothing else breaks.
  }
};

/**
 * After a successful start: the strategy uses fooling=ts but Windows TCP
 * timestamps are off (e.g. «Настрою сам» on the first run skipped the
 * question). Ask once; a «no» is remembered, Diagnostics still offers it.
 */
function useTimestampsReminder() {
  const { toast } = useApp();
  const tr = useTr();
  return useCallback(async () => {
    if (tsDeclined()) return;
    const needed = await api.tcpTimestampsNeeded().catch(() => false);
    if (!needed) return;
    const ok = await confirmDialog({
      title: tr("Включить TCP timestamps?", "Enable TCP timestamps?"),
      body: tr(
        "Эта стратегия использует fooling=ts, а TCP timestamps в Windows выключены — без них она может работать хуже. Это глобальная настройка TCP (оригинальный Flowseal включает её всегда). Исходное значение запоминается, вернуть его можно в «Настройки → Обход». Если откажетесь, больше не спросим: включить можно и в «Диагностике».",
        "This strategy uses fooling=ts, but Windows TCP timestamps are off — it may work worse without them. A global TCP setting (original Flowseal always enables it). The original value is recorded; restore it in Settings → Bypass. If you decline, we won't ask again: you can still enable it in Diagnostics.",
      ),
      code: "netsh interface tcp set global timestamps=enabled",
      confirmLabel: tr("Включить", "Enable"),
      cancelLabel: tr("Не включать", "Don't enable"),
    });
    if (!ok) return declineTs();
    try {
      toast("ok", (await api.setTcpTimestamps(true, true)).message);
    } catch (e) {
      toast("error", errorText(e));
    }
  }, [toast, tr]);
}

/** Activates a strategy and reports the real outcome (process + HTTPS probe). */
export function useActivate() {
  const { setBusy, toast, refresh, setActivation } = useApp();
  const tr = useTr();
  const remindTimestamps = useTimestampsReminder();
  return useCallback(
    async (id: string | null, ignoreConflicts = false) => {
      setBusy(true);
      try {
        const r = await api.bypassEnable(id, ignoreConflicts);
        if (r.ok) {
          if (r.httpsOk) {
            toast("ok", tr("Zapret запущен. Discord отвечает по HTTPS. Голос проверьте вручную.", "Zapret started. Discord responds over HTTPS. Check voice manually."));
          } else {
            toast(
              "info",
              tr(
                "Zapret запущен, но HTTPS-проверка Discord не пройдена — стратегия может не подходить вашему провайдеру.",
                "Zapret started, but the Discord HTTPS check failed — the strategy may not suit your ISP.",
              ),
            );
          }
          setActivation(null);
          void remindTimestamps();
        } else {
          setActivation(r);
        }
        await refresh();
        return r;
      } catch (e) {
        toast("error", errorText(e));
        return null;
      } finally {
        setBusy(false);
      }
    },
    [setBusy, toast, refresh, setActivation, tr, remindTimestamps],
  );
}

/** The single «Launch Discord» action (home, onboarding, tray share the backend command). */
export function useLaunch() {
  const { toast, refresh, setLaunchFailure } = useApp();
  return useCallback(
    async (mode: "preference" | "withBypass" | "withoutBypass" = "preference") => {
      try {
        const r = await api.launchDiscord(mode);
        if (r.launched) {
          toast("ok", r.message);
          setLaunchFailure(null);
        } else {
          setLaunchFailure(r);
        }
        setTimeout(refresh, 2000);
        return r;
      } catch (e) {
        toast("error", errorText(e));
        return null;
      }
    },
    [toast, refresh, setLaunchFailure],
  );
}

/** Automatic strategy selection (home and diagnostics share it). */
export function useAuto() {
  const { overview, setAuto, toast, refresh } = useApp();
  const max = overview?.settings.autoMaxAttempts ?? 0;
  const start = useCallback(async () => {
    setAuto({ running: true, attempt: 0, maxAttempts: max, log: [], result: null });
    try {
      const r = await api.autoStart();
      setAuto(r);
      refresh();
      return r;
    } catch (e) {
      toast("error", errorText(e));
      setAuto(null);
      return null;
    }
  }, [max, setAuto, toast, refresh]);
  const cancel = useCallback(() => api.autoCancel().catch((e) => toast("error", errorText(e))), [toast]);
  return { start, cancel };
}
