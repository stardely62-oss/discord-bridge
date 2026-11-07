import { save } from "@tauri-apps/plugin-dialog";
import { CheckCircle2, ChevronDown, CircleHelp, Eraser, FileOutput, MessageSquareWarning, MinusCircle, Mic, RefreshCw, Sparkles, Square, TriangleAlert, XCircle } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Badge, Button, Card, PageHeader, Segmented } from "../../components/ui";
import { confirmDialog } from "../../components/ui/confirm";
import { useAuto } from "../../hooks/useActivate";
import { api, errorText } from "../../services/api";
import type { CheckItem, CheckStatus, DiagnosticsReport } from "../../services/types";
import { useApp, useTr } from "../../stores/app";

function StatusIcon({ s }: { s: CheckStatus }) {
  const cls = "h-4 w-4 shrink-0";
  if (s === "ok") return <CheckCircle2 className={`${cls} text-ok`} />;
  if (s === "warning") return <TriangleAlert className={`${cls} text-warn`} />;
  if (s === "error") return <XCircle className={`${cls} text-bad`} />;
  if (s === "skipped") return <MinusCircle className={`${cls} text-ink-faint`} />;
  return <CircleHelp className={`${cls} text-ink-faint`} />;
}

export default function DiagnosticsPage() {
  const { overview, toast, refresh, auto } = useApp();
  const tr = useTr();
  const autoSel = useAuto();
  const [tab, setTab] = useState<"checks" | "fix" | "logs">("checks");
  const [details, setDetails] = useState(false);
  const [report, setReport] = useState<DiagnosticsReport | null>(overview?.lastDiagnostics ?? null);
  const [history, setHistory] = useState<DiagnosticsReport[]>([]);
  const [running, setRunning] = useState(false);
  const [logTab, setLogTab] = useState<"app" | "engine">("app");
  const [logs, setLogs] = useState<string[]>([]);

  const loadLogs = useCallback(async () => {
    try {
      setLogs(logTab === "app" ? await api.logs(300) : await api.engineLog());
    } catch (e) {
      setLogs([errorText(e)]);
    }
  }, [logTab]);

  useEffect(() => {
    loadLogs();
  }, [loadLogs]);
  useEffect(() => {
    api.diagnosticsHistory().then(setHistory).catch(() => undefined);
  }, [report]);

  const run = async () => {
    setRunning(true);
    try {
      const r = await api.runDiagnostics();
      setReport(r);
      refresh();
      loadLogs();
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setRunning(false);
    }
  };

  const enableTs = async () => {
    const ok = await confirmDialog({
      title: tr("Включить TCP timestamps?", "Enable TCP timestamps?"),
      body: tr("Это глобальная настройка TCP Windows (её же включает Flowseal). Исходное значение запоминается, вернуть его можно в «Настройки → Обход».", "A global Windows TCP setting (Flowseal enables it too). The original value is recorded; restore it in Settings → Bypass."),
      code: "netsh interface tcp set global timestamps=enabled",
      confirmLabel: tr("Включить", "Enable"),
    });
    if (!ok) return;
    try {
      const r = await api.setTcpTimestamps(true, true);
      toast("ok", r.message);
      run();
    } catch (e) {
      toast("error", errorText(e));
    }
  };

  const voice = async (works: boolean) => {
    const id = report?.strategyId ?? overview?.settings.activeStrategyId;
    if (!id) return;
    await api.confirmVoice(id, works).catch((e) => toast("error", errorText(e)));
    toast("ok", tr("Отметка сохранена", "Saved"));
    refresh();
    run();
  };

  const exportReport = async () => {
    const path = await save({ defaultPath: "discord-bridge-report.json", filters: [{ name: "JSON", extensions: ["json"] }] });
    if (!path) return;
    try {
      await api.exportReport(path);
      toast("ok", tr("Обезличенный отчёт сохранён", "Anonymised report saved"));
    } catch (e) {
      toast("error", errorText(e));
    }
  };

  const groups: { id: string; title: string }[] = [
    { id: "discord", title: "Discord" },
    { id: "zapret", title: "Zapret" },
    { id: "system", title: tr("Система", "System") },
    { id: "dns", title: "DNS" },
    { id: "https", title: "HTTPS" },
    { id: "udp", title: tr("UDP и голос", "UDP & voice") },
  ];
  const byGroup = (g: string): CheckItem[] => report?.items.filter((i) => i.group === g) ?? [];

  return (
    <div className="space-y-5">
      <PageHeader
        title={tr("Диагностика", "Diagnostics")}
        subtitle={tr("Что работает, что нет и как это исправить.", "What works, what doesn't and how to fix it.")}
        actions={
          <Button variant="primary" icon={<RefreshCw className="h-4 w-4" />} loading={running} onClick={run}>
            {report ? tr("Проверить снова", "Check again") : tr("Проверить", "Check")}
          </Button>
        }
      />
      <Segmented<"checks" | "fix" | "logs">
        ariaLabel={tr("Разделы диагностики", "Diagnostics sections")}
        value={tab}
        onChange={setTab}
        options={[
          { value: "checks", label: tr("Проверка", "Checks") },
          { value: "fix", label: tr("Подбор и голос", "Fix & voice") },
          { value: "logs", label: tr("Журнал", "Log") },
        ]}
      />

      {tab === "checks" && !report && <Card className="p-6 text-sm text-ink-muted">{tr("Проверка ещё не выполнялась — нажмите «Проверить».", "No check yet — press Check.")}</Card>}

      {tab === "checks" && report && (() => {
        const problems = report.items.filter((i) => i.status === "error" || i.status === "warning");
        const bad = problems.some((i) => i.status === "error");
        return (
          <Card className="p-5">
            <div className="flex items-start gap-3">
              {problems.length === 0 ? <CheckCircle2 className="mt-0.5 h-6 w-6 shrink-0 text-ok" /> : bad ? <XCircle className="mt-0.5 h-6 w-6 shrink-0 text-bad" /> : <TriangleAlert className="mt-0.5 h-6 w-6 shrink-0 text-warn" />}
              <div className="min-w-0 flex-1">
                <div className="text-base font-semibold">
                  {problems.length === 0 ? tr("Всё в порядке", "Everything is fine") : tr(`Нужно внимание: ${problems.length}`, `Needs attention: ${problems.length}`)}
                </div>
                <div className="mt-0.5 text-[13px] text-ink-muted">
                  {problems.length === 0
                    ? tr("Все проверки прошли. Если голос не работает — проверьте его на вкладке «Подбор и голос».", "All checks passed. If voice does not work, check it on the Fix & voice tab.")
                    : tr("Ниже — что именно не так и что можно сделать. Это не обязательно поломка: часть пунктов — просто предупреждения.", "Below is what is wrong and what you can do. Not always a failure: some are just warnings.")}
                </div>
                {problems.length > 0 && (
                  <ul className="mt-3 space-y-2">
                    {problems.map((i) => (
                      <li key={i.id} className="flex gap-2.5 rounded-md bg-surface-2 px-3 py-2">
                        <StatusIcon s={i.status} />
                        <div className="min-w-0">
                          <div className="text-[13px] font-medium">{i.title}</div>
                          <div className="text-xs text-ink-muted">{i.hint ?? i.detail}</div>
                          {i.id === "tcp-ts" && i.status === "warning" && (
                            <Button size="sm" className="mt-1.5" onClick={enableTs}>
                              {tr("Включить TCP timestamps…", "Enable TCP timestamps…")}
                            </Button>
                          )}
                        </div>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            </div>
            <button className="mt-4 flex items-center gap-1.5 text-xs text-ink-muted hover:text-ink" aria-expanded={details} onClick={() => setDetails(!details)}>
              <ChevronDown className={`h-3.5 w-3.5 transition-transform ${details ? "rotate-180" : ""}`} />
              {details ? tr("Скрыть технические подробности", "Hide technical details") : tr("Технические подробности", "Technical details")}
            </button>
          </Card>
        );
      })()}

      {tab === "checks" && report && details && (
        <div className="columns-2 gap-4 narrow:columns-1">
          {groups.map((g) => {
            const items = byGroup(g.id);
            if (items.length === 0) return null;
            return (
              <Card key={g.id} className="mb-4 break-inside-avoid p-4">
                <div className="label mb-2">{g.title}</div>
                <ul className="space-y-2">
                  {items.map((i) => (
                    <li key={i.id} className="flex gap-2.5">
                      <StatusIcon s={i.status} />
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center justify-between gap-2 text-sm">
                          <span className="truncate">{i.title}</span>
                          {i.durationMs != null && <span className="shrink-0 text-[11px] text-ink-faint">{i.durationMs} ms</span>}
                        </div>
                        <div className="selectable break-words text-xs text-ink-muted">{i.detail}</div>
                        {i.hint && <div className="mt-0.5 text-[11px] text-ink-faint">{i.hint}</div>}
                        {i.id === "tcp-ts" && i.status === "warning" && (
                          <Button size="sm" className="mt-1.5" onClick={enableTs}>
                            {tr("Включить TCP timestamps…", "Enable TCP timestamps…")}
                          </Button>
                        )}
                      </div>
                    </li>
                  ))}
                </ul>
              </Card>
            );
          })}
        </div>
      )}

      {/* Auto mode */}
      {tab === "fix" && (
      <Card className="p-5">
        <div className="flex items-start justify-between gap-4">
          <div className="flex items-start gap-3">
            <div className="rounded-md bg-accent-soft p-2.5 text-accent">
              <Sparkles className="h-5 w-5" />
            </div>
            <div>
              <div className="flex items-center gap-2 text-sm font-semibold">
                {tr("Автоподбор стратегии", "Automatic strategy selection")} <Badge tone="warn">{tr("ограниченный", "limited")}</Badge>
              </div>
              <p className="mt-1 max-w-xl text-xs text-ink-muted">
                {tr(
                  `Перебирает стратегии (максимум ${overview?.settings.autoMaxAttempts ?? 8} попыток, общий лимит 5 минут) и оставляет первую, при которой Discord отвечает по HTTPS. Голосовые каналы Auto проверить не может — после подбора проверьте голос вручную. При неудаче возвращается последняя рабочая стратегия.`,
                  `Tries strategies (max ${overview?.settings.autoMaxAttempts ?? 8} attempts, 5-minute limit) and keeps the first one with which Discord answers over HTTPS. Auto cannot test voice — check it manually afterwards. On failure the last working strategy is restored.`,
                )}
              </p>
            </div>
          </div>
          {auto?.running ? (
            <Button variant="danger" icon={<Square className="h-3.5 w-3.5" />} onClick={autoSel.cancel}>
              {tr("Отменить", "Cancel")}
            </Button>
          ) : (
            <Button variant="primary" icon={<Sparkles className="h-3.5 w-3.5" />} disabled={!overview?.helper.available} onClick={autoSel.start}>
              {tr("Подобрать автоматически", "Find automatically")}
            </Button>
          )}
        </div>
        {auto && (
          <div className="mt-4 rounded-md border border-line bg-sunken p-3">
            {auto.running && (
              <div className="mb-2 h-1 overflow-hidden rounded-full bg-surface-3">
                <div className="h-full bg-accent transition-all" style={{ width: `${auto.maxAttempts ? (auto.attempt / auto.maxAttempts) * 100 : 5}%` }} />
              </div>
            )}
            <ul className="selectable max-h-48 space-y-0.5 overflow-auto font-mono text-[11px] text-ink-muted">
              {auto.log.map((l, i) => (
                <li key={i}>{l}</li>
              ))}
            </ul>
            {auto.result && <div className={`mt-2 text-xs ${auto.successId ? "text-ok" : auto.restored === false ? "text-bad" : "text-warn"}`}>{auto.result}</div>}
          </div>
        )}
      </Card>

      )}

      {/* Manual voice test */}
      {tab === "fix" && (
      <Card className="p-5">
        <div className="flex items-start gap-3">
          <div className="rounded-md bg-surface-3 p-2.5 text-ink-muted">
            <Mic className="h-5 w-5" />
          </div>
          <div className="flex-1">
            <div className="text-sm font-semibold">{tr("Ручная проверка голосовых каналов", "Manual voice channel test")}</div>
            <ol className="mt-1.5 list-decimal space-y-0.5 pl-4 text-xs text-ink-muted">
              <li>{tr("Включите обход DPI и запустите Discord.", "Enable DPI bypass and launch Discord.")}</li>
              <li>{tr("Зайдите в любой голосовой канал; дождитесь статуса «Голосовая связь подключена».", "Join any voice channel; wait for “Voice Connected”.")}</li>
              <li>{tr("Если статус застрял на «Подключение» / «RTC Connecting» — голос не работает с этой стратегией.", "If it is stuck on “RTC Connecting”, voice does not work with this strategy.")}</li>
              <li>{tr("Отметьте результат — он сохранится для этой стратегии.", "Record the result — it is saved for this strategy.")}</li>
            </ol>
            <div className="mt-3 flex gap-2">
              <Button size="sm" icon={<CheckCircle2 className="h-3.5 w-3.5 text-ok" />} onClick={() => voice(true)}>
                {tr("Голос работает", "Voice works")}
              </Button>
              <Button size="sm" icon={<XCircle className="h-3.5 w-3.5 text-bad" />} onClick={() => voice(false)}>
                {tr("Голос не работает", "Voice doesn't work")}
              </Button>
            </div>
          </div>
        </div>
      </Card>

      )}

      {/* Logs */}
      {tab === "logs" && (
      <Card className="p-5">
        <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
          <Segmented<"app" | "engine">
            ariaLabel={tr("Журнал", "Log")}
            value={logTab}
            onChange={setLogTab}
            options={[
              { value: "app", label: tr("Журнал приложения", "App log") },
              { value: "engine", label: tr("Вывод winws", "winws output") },
            ]}
          />
          <div className="flex flex-wrap gap-2">
            <Button size="sm" variant="ghost" icon={<RefreshCw className="h-3.5 w-3.5" />} onClick={loadLogs}>
              {tr("Обновить", "Refresh")}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              icon={<Eraser className="h-3.5 w-3.5" />}
              onClick={async () => {
                await api.clearLogs();
                loadLogs();
              }}
            >
              {tr("Очистить", "Clear")}
            </Button>
            <Button size="sm" icon={<FileOutput className="h-3.5 w-3.5" />} onClick={exportReport}>
              {tr("Экспорт отчёта", "Export report")}
            </Button>
            <Button
              size="sm"
              icon={<MessageSquareWarning className="h-3.5 w-3.5" />}
              title={tr("Откроется GitHub. Отчёт не отправляется сам: при желании приложите экспортированный файл, предварительно проверив его.", "Opens GitHub. Nothing is sent automatically: attach the exported report yourself if you wish, after checking it.")}
              onClick={() => api.openExternal("report-issue").catch((e) => toast("error", errorText(e)))}
            >
              {tr("Сообщить о проблеме", "Report a problem")}
            </Button>
          </div>
        </div>
        <pre className="selectable max-h-72 overflow-auto rounded-md border border-line bg-sunken p-3 font-mono text-[11px] leading-5 text-ink-muted">
          {logs.length ? logs.join("\n") : tr("Журнал пуст", "Log is empty")}
        </pre>
        <p className="mt-2 text-[11px] text-ink-faint">
          {tr("Пути профиля и имя пользователя маскируются; учётные данные и содержимое трафика не записываются. Размер журнала ограничен 2 МБ.", "Profile paths and user name are masked; credentials and traffic content are never logged. Log size is capped at 2 MB.")}
        </p>
      </Card>

      )}

      {tab === "logs" && history.length > 1 && (
        <Card className="p-5">
          <div className="label mb-2">{tr("Последние проверки", "Recent checks")}</div>
          <table className="w-full text-xs">
            <tbody>
              {[...history].reverse().map((h) => (
                <tr key={h.at} className="border-b border-line last:border-0">
                  <td className="py-1.5 text-ink-muted">{new Date(h.at * 1000).toLocaleString()}</td>
                  <td className="py-1.5 font-mono">{h.strategyId ?? "—"}</td>
                  <td className="py-1.5">{h.engineRunning ? "Zapret ✓" : "Zapret —"}</td>
                  <td className="py-1.5">{h.httpsOk ? <span className="text-ok">HTTPS ✓</span> : <span className="text-bad">HTTPS ✗</span>}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      )}
    </div>
  );
}
