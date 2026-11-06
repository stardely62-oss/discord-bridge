import { useCallback } from "react";
import { api, errorText } from "../services/api";
import { useApp, useTr } from "../stores/app";

/** Activates a strategy and reports the real outcome (process + HTTPS probe). */
export function useActivate() {
  const { setBusy, toast, refresh, setActivation } = useApp();
  const tr = useTr();
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
    [setBusy, toast, refresh, setActivation, tr],
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
