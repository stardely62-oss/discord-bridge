import { CheckCircle2, Circle, Loader2, Play, Sparkles, XCircle } from "lucide-react";
import { useState } from "react";
import { api, errorText } from "../../services/api";
import { useApp, useTr } from "../../stores/app";
import { declineTs, useLaunch } from "../../hooks/useActivate";
import { Button, Checkbox, Modal } from "../ui";

type StepState = "todo" | "run" | "ok" | "fail";

/**
 * One-click first-run setup: autostart, automatic bypass, TCP timestamps
 * (with explicit consent shown here) and automatic strategy selection.
 */
export function Onboarding() {
  const { overview, refresh, toast } = useApp();
  const tr = useTr();
  const launch = useLaunch();
  const [autostart, setAutostart] = useState(true);
  const [timestamps, setTimestamps] = useState(true);
  const [autoPick, setAutoPick] = useState(true);
  const [running, setRunning] = useState(false);
  const [steps, setSteps] = useState<Record<string, StepState>>({});
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);

  if (!overview || overview.settings.onboardingDone) return null;
  const helperOk = overview.helper.available;

  const mark = (k: string, v: StepState) => setSteps((s) => ({ ...s, [k]: v }));

  const finish = async (markDone = true) => {
    if (markDone) {
      try {
        const s = await api.getSettings();
        await api.saveSettings({ ...s, onboardingDone: true });
      } catch (e) {
        toast("error", errorText(e));
      }
    }
    await refresh();
  };

  const run = async () => {
    setRunning(true);
    try {
      mark("settings", "run");
      const s = await api.getSettings();
      await api.saveSettings({
        ...s,
        launchAtStartup: autostart,
        startMinimized: autostart,
        autoEnableBypass: true,
        closeBehavior: "tray",
      });
      mark("settings", "ok");

      // Unticked here is already an answer: no reminder after a later start.
      if (!timestamps) declineTs();
      if (timestamps) {
        mark("ts", "run");
        try {
          await api.setTcpTimestamps(true, true);
          mark("ts", "ok");
        } catch {
          mark("ts", "fail");
        }
      }

      mark("bypass", "run");
      let ok = false;
      let text = "";
      if (autoPick) {
        const r = await api.autoStart();
        ok = !!r.successId;
        text = r.result ?? "";
      } else {
        const r = await api.bypassEnable(null);
        ok = r.ok && !!r.httpsOk;
        text = r.ok ? (r.httpsOk ? tr("Discord отвечает по HTTPS.", "Discord answers over HTTPS.") : tr("Zapret запущен, но Discord по HTTPS не ответил.", "Zapret runs, but Discord did not answer over HTTPS.")) : (r.message ?? "");
      }
      mark("bypass", ok ? "ok" : "fail");
      setResult({ ok, text });
    } catch (e) {
      setResult({ ok: false, text: errorText(e) });
    } finally {
      setRunning(false);
    }
  };

  const Step = ({ k, label }: { k: string; label: string }) => {
    const st = steps[k] ?? "todo";
    return (
      <li className="flex items-center gap-2 text-sm">
        {st === "run" ? (
          <Loader2 className="h-4 w-4 animate-spin text-accent" />
        ) : st === "ok" ? (
          <CheckCircle2 className="h-4 w-4 text-ok" />
        ) : st === "fail" ? (
          <XCircle className="h-4 w-4 text-bad" />
        ) : (
          <Circle className="h-4 w-4 text-ink-faint" />
        )}
        {label}
      </li>
    );
  };

  const Check = ({ checked, onChange, title, text }: { checked: boolean; onChange: (v: boolean) => void; title: string; text: string }) => (
    <div className="rounded-md border border-line bg-surface-2 p-3">
      <Checkbox checked={checked} onChange={onChange} disabled={running || !!result} title={title} description={text} />
    </div>
  );

  const started = running || result !== null;

  return (
    <Modal
      open
      onClose={() => !running && finish(true)}
      title={tr("Добро пожаловать в Bridge", "Welcome to Bridge")}
      footer={
        result ? (
          <>
            <Button variant="ghost" onClick={() => finish(true)}>
              {tr("Готово", "Done")}
            </Button>
            <Button
              variant="primary"
              icon={<Play className="h-4 w-4" />}
              onClick={async () => {
                await finish(true);
                await launch("withBypass");
              }}
            >
              {tr("Запустить Discord", "Launch Discord")}
            </Button>
          </>
        ) : (
          <>
            <Button variant="ghost" disabled={running} onClick={() => finish(true)}>
              {tr("Настрою сам", "I'll set it up myself")}
            </Button>
            <Button variant="primary" icon={<Sparkles className="h-4 w-4" />} loading={running} disabled={!helperOk} onClick={run}>
              {tr("Настроить автоматически", "Set up automatically")}
            </Button>
          </>
        )
      }
    >
      <div className="space-y-3">
        <p className="text-sm text-ink-muted">
          {tr(
            "Один клик — и дальше всё работает само: при входе в Windows обход DPI включится в фоне, а Discord будет открываться как обычно.",
            "One click and everything works by itself: the DPI bypass starts in the background at Windows logon and Discord opens as usual.",
          )}
        </p>
        {!helperOk && (
          <div className="rounded-md border border-bad/30 bg-bad-soft px-3 py-2 text-xs text-bad">
            {tr("Служба Discord Bridge Helper недоступна — переустановите приложение.", "Discord Bridge Helper service is unavailable — reinstall the app.")} {overview.helper.error}
          </div>
        )}
        {!started ? (
          <div className="space-y-2">
            <Check checked={autostart} onChange={setAutostart} title={tr("Запускать вместе с Windows", "Start with Windows")} text={tr("Тихо, свёрнутым в трей.", "Quietly, minimized to the tray.")} />
            <Check checked={autoPick} onChange={setAutoPick} title={tr("Подобрать рабочую стратегию", "Find a working strategy")} text={tr("Перебор стратегий Flowseal до первой, с которой Discord открывается (до пары минут).", "Tries Flowseal strategies until Discord opens (up to a couple of minutes).")} />
            <Check
              checked={timestamps}
              onChange={setTimestamps}
              title={tr("Включить TCP timestamps в Windows", "Enable TCP timestamps in Windows")}
              text={tr(
                "Многим стратегиям это нужно; оригинальный Flowseal включает их всегда. Это глобальная настройка TCP, безопасна и обратима.",
                "Many strategies need it; original Flowseal always enables it. A global, safe and reversible TCP setting.",
              )}
            />
          </div>
        ) : (
          <ul className="space-y-2 rounded-md border border-line bg-surface-2 p-3">
            <Step k="settings" label={tr("Сохранение настроек", "Saving settings")} />
            {timestamps && <Step k="ts" label="TCP timestamps" />}
            <Step k="bypass" label={autoPick ? tr("Подбор стратегии и проверка Discord", "Selecting strategy and checking Discord") : tr("Включение обхода", "Enabling bypass")} />
          </ul>
        )}
        {result && (
          <div className={`rounded-md border px-3 py-2 text-sm ${result.ok ? "border-ok/30 bg-ok-soft text-ok" : "border-warn/30 bg-warn-soft text-warn"}`}>
            {result.ok
              ? tr("Готово! Discord доступен. Если голосовой канал не подключается — откройте «Диагностику».", "Done! Discord is reachable. If voice doesn't connect, open Diagnostics.")
              : tr("Автоматически подобрать стратегию не удалось. Попробуйте другую в разделе «Стратегии».", "Couldn't find a strategy automatically. Try another one in Strategies.")}
            {result.text && <div className="mt-1 text-xs opacity-80">{result.text}</div>}
          </div>
        )}
      </div>
    </Modal>
  );
}
