import { save } from "@tauri-apps/plugin-dialog";
import { Braces, Clock, Copy, Download, GitCompare, Pencil, Play, RotateCcw, Save, Sparkles, Trash2, Undo2, Wand2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useActivate } from "../../hooks/useActivate";
import { api, errorText } from "../../services/api";
import type { OptionSpec, Strategy, StrategyDetails } from "../../services/types";
import { useApp, useCount, useTr } from "../../stores/app";
import { Badge, Button, Card, Menu, Segmented, Skeleton, Tooltip } from "../ui";
import { confirmDialog } from "../ui/confirm";
import { CodeEditor } from "./CodeEditor";
import { DiffDialog, PromptDialog } from "./Dialogs";
import { clone, sameStrategy } from "./editorUtils";
import { noteText, sourceBadge } from "./StrategyList";
import { strategyKind } from "../../catalog/strategies";
import { VisualEditor } from "./VisualEditor";

type Tab = "overview" | "editor" | "args";
type Mode = "visual" | "code";

export function StrategyView({ id, specs, resources }: { id: string; specs: OptionSpec[]; resources: { fakes: string[]; lists: string[] } }) {
  const { toast, overview, refreshStrategies, selectStrategy, busy } = useApp();
  const tr = useTr();
  const count = useCount();
  const activate = useActivate();
  const [details, setDetails] = useState<StrategyDetails | null>(null);
  const [draft, setDraft] = useState<Strategy | null>(null);
  const [preview, setPreview] = useState<StrategyDetails | null>(null);
  const [tab, setTab] = useState<Tab>("overview");
  const [mode, setMode] = useState<Mode>("visual");
  const [code, setCode] = useState("");
  const [codeError, setCodeError] = useState<string | null>(null);
  const [diffOpen, setDiffOpen] = useState<{ l: string; r: string } | null>(null);
  const [prompt, setPrompt] = useState<null | "duplicate" | "rename">(null);
  const [saving, setSaving] = useState(false);
  const [showArgs, setShowArgs] = useState(() => window.innerWidth >= 1280);
  const timer = useRef<number | undefined>(undefined);

  const load = useCallback(async () => {
    try {
      const d = await api.getStrategy(id);
      setDetails(d);
      setDraft(clone(d.strategy));
      setPreview(d);
      setCode(JSON.stringify(d.strategy, null, 2));
      setCodeError(null);
    } catch (e) {
      toast("error", errorText(e));
    }
  }, [id, toast]);

  useEffect(() => {
    load();
  }, [load]);

  // Live validation through the Rust validator (debounced).
  useEffect(() => {
    if (!draft || mode !== "visual") return;
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      api.previewStrategy(draft).then(setPreview).catch(() => undefined);
    }, 250);
    return () => window.clearTimeout(timer.current);
  }, [draft, mode]);

  const s = details?.strategy;
  const readOnly = s?.source === "flowseal";
  const dirty = useMemo(() => !!(s && draft && !sameStrategy(s, draft)) || (mode === "code" && !!s && code !== JSON.stringify(s, null, 2)), [s, draft, code, mode]);
  const running = overview?.engine?.state === "running" && overview.engine.strategyId === id;
  const errorCount = (preview?.issues ?? []).filter((i) => i.severity === "error").length;

  if (!s || !draft)
    return (
      <div className="flex-1 space-y-3 p-5" aria-busy="true">
        <Skeleton className="h-6 w-64" />
        <Skeleton className="h-4 w-96" />
        <Skeleton className="h-8 w-80" />
        <Skeleton className="mt-6 h-20 w-full" />
        <Skeleton className="h-20 w-full" />
      </div>
    );

  const switchMode = async (m: Mode) => {
    if (m === mode) return;
    if (m === "code") {
      setCode(JSON.stringify(draft, null, 2));
      setMode("code");
      return;
    }
    const r = await api.parseStrategyJson(code);
    if (!r.ok || !r.details) {
      setCodeError(`${r.error}${r.line ? ` (${tr("строка", "line")} ${r.line})` : ""}`);
      toast("error", tr("Исправьте ошибку в JSON перед переключением", "Fix the JSON error before switching"));
      return;
    }
    setDraft(r.details.strategy);
    setPreview(r.details);
    setCodeError(null);
    setMode("visual");
  };

  const doSave = async () => {
    setSaving(true);
    try {
      let toSave = draft;
      if (mode === "code") {
        const r = await api.parseStrategyJson(code);
        if (!r.ok || !r.details) {
          setCodeError(`${r.error}${r.line ? ` (${tr("строка", "line")} ${r.line})` : ""}`);
          return;
        }
        toSave = r.details.strategy;
      }
      if (toSave.id !== s.id) {
        toast("error", tr("Идентификатор стратегии менять нельзя", "Strategy id cannot be changed"));
        return;
      }
      const d = await api.saveStrategy(toSave);
      setDetails(d);
      setDraft(clone(d.strategy));
      setPreview(d);
      setCode(JSON.stringify(d.strategy, null, 2));
      toast("ok", tr("Сохранено", "Saved"));
      await refreshStrategies();
      if (running) toast("info", tr("Стратегия активна — нажмите «Активировать», чтобы применить изменения.", "Strategy is active — press Activate to apply changes."));
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setSaving(false);
    }
  };

  const reset = () => {
    setDraft(clone(s));
    setCode(JSON.stringify(s, null, 2));
    setCodeError(null);
    setPreview(details);
  };

  const duplicate = async (name: string) => {
    setPrompt(null);
    try {
      const c = await api.duplicateStrategy(s.id, name);
      await refreshStrategies();
      selectStrategy(c.id);
      toast("ok", tr(`Создана копия «${c.name}»`, `Copy "${c.name}" created`));
    } catch (e) {
      toast("error", errorText(e));
    }
  };

  const rename = async (name: string) => {
    setPrompt(null);
    try {
      await api.renameStrategy(s.id, name);
      await refreshStrategies();
      load();
    } catch (e) {
      toast("error", errorText(e));
    }
  };

  const remove = async () => {
    if (!(await confirmDialog({ title: tr(`Удалить стратегию «${s.name}»?`, `Delete strategy "${s.name}"?`), danger: true, confirmLabel: tr("Удалить", "Delete") }))) return;
    try {
      await api.deleteStrategy(s.id);
      selectStrategy(null);
      await refreshStrategies();
      toast("ok", tr("Стратегия удалена", "Strategy deleted"));
    } catch (e) {
      toast("error", errorText(e));
    }
  };

  const restore = async () => {
    if (
      !(await confirmDialog({
        title: tr("Восстановить оригинал?", "Restore the original?"),
        body: tr("Правила этой копии заменятся оригинальными правилами Flowseal. Название сохранится.", "This copy's rules are replaced with the Flowseal original. The name is kept."),
        confirmLabel: tr("Восстановить", "Restore"),
      }))
    )
      return;
    try {
      await api.restoreOriginal(s.id);
      await load();
      await refreshStrategies();
      toast("ok", tr("Оригинальная конфигурация восстановлена", "Original configuration restored"));
    } catch (e) {
      toast("error", errorText(e));
    }
  };

  const doExport = async (format: "json" | "toml") => {
    const path = await save({ defaultPath: `${s.id}.${format}`, filters: [{ name: format.toUpperCase(), extensions: [format] }] });
    if (!path) return;
    try {
      await api.exportStrategy(s.id, format, path);
      toast("ok", tr("Экспортировано", "Exported"));
    } catch (e) {
      toast("error", errorText(e));
    }
  };

  const formatCode = () => {
    try {
      setCode(JSON.stringify(JSON.parse(code), null, 2));
      setCodeError(null);
    } catch (e) {
      setCodeError(errorText(e));
    }
  };

  const issues = preview?.issues ?? [];
  const kind = strategyKind(s.name, s.notes ?? []);

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col">
      {/* Header */}
      <div className="border-b border-line px-5 pb-3 pt-4">
        <div className="flex min-w-0 items-center gap-2">
          <h2 className="truncate text-base font-semibold" title={s.name}>
            {s.name}
          </h2>
          {sourceBadge(s.source, tr)}
          {running && <Badge tone="ok">{tr("активна", "active")}</Badge>}
          {s.requirements.tcpTimestamps && (
            <span className="inline-flex items-center gap-1">
              <Badge>
                <Clock className="h-3 w-3" /> TCP timestamps
              </Badge>
              <Tooltip
                text={tr(
                  "Стратегии нужны TCP timestamps Windows. Оригинальный Flowseal включает их сам, Discord Bridge — только с вашего согласия (Настройки → Обход).",
                  "This strategy needs Windows TCP timestamps. Flowseal enables them itself; Discord Bridge only with your consent (Settings → Bypass).",
                )}
              />
            </span>
          )}
          {s.notes?.map((n) => (
            <Badge key={n} tone="warn">
              {noteText(n, tr)}
            </Badge>
          ))}
        </div>
        {readOnly && (
          <p className="mt-1.5 text-[13px] text-ink">
            <span className={kind.tone === "warn" ? "text-warn" : kind.tone === "ok" ? "text-ok" : "text-accent"}>{tr(kind.ru, kind.en)}</span>
            <span className="text-ink-muted"> — {tr(kind.hintRu, kind.hintEn)}</span>
          </p>
        )}
        <p className="mt-1 line-clamp-2 text-xs text-ink-muted">{s.description || tr("Без описания", "No description")}</p>
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <Button
            variant="primary"
            size="sm"
            icon={<Play className="h-3.5 w-3.5" />}
            disabled={busy || errorCount > 0 || dirty}
            onClick={() => activate(s.id)}
            title={dirty ? tr("Сначала сохраните изменения", "Save changes first") : undefined}
          >
            {running ? tr("Перезапустить", "Restart") : tr("Включить", "Activate")}
          </Button>
          <Button size="sm" icon={<Copy className="h-3.5 w-3.5" />} onClick={() => setPrompt("duplicate")}>
            {readOnly ? tr("Копия для правки", "Copy to edit") : tr("Дублировать", "Duplicate")}
          </Button>
          <Button size="sm" icon={<GitCompare className="h-3.5 w-3.5" />} onClick={() => setDiffOpen({ l: s.origin.baseId ?? s.id, r: s.id })}>
            {s.origin.baseId ? tr("С оригиналом", "Vs original") : tr("Сравнить", "Compare")}
          </Button>
          <Menu
            label={tr("Другие действия", "More actions")}
            items={[
              { label: tr("Экспорт в JSON", "Export JSON"), icon: <Download className="h-3.5 w-3.5" />, onClick: () => doExport("json") },
              { label: tr("Экспорт в TOML", "Export TOML"), icon: <Download className="h-3.5 w-3.5" />, onClick: () => doExport("toml") },
              { label: tr("Переименовать", "Rename"), icon: <Pencil className="h-3.5 w-3.5" />, onClick: () => setPrompt("rename"), hidden: readOnly },
              { label: tr("Восстановить оригинал", "Restore original"), icon: <RotateCcw className="h-3.5 w-3.5" />, onClick: restore, hidden: readOnly || !s.origin.baseId },
              { label: tr("Удалить", "Delete"), icon: <Trash2 className="h-3.5 w-3.5" />, onClick: remove, danger: true, hidden: readOnly },
            ]}
          />
        </div>
        <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
          <Segmented<Tab>
            ariaLabel={tr("Вкладки", "Tabs")}
            value={tab}
            onChange={setTab}
            options={[
              { value: "overview", label: tr("Обзор", "Overview") },
              { value: "editor", label: readOnly ? tr("Параметры", "Parameters") : tr("Редактор", "Editor") },
              { value: "args", label: tr("Команда", "Command") },
            ]}
          />
          {tab === "editor" && (
            <Segmented<Mode>
              ariaLabel={tr("Режим редактора", "Editor mode")}
              value={mode}
              onChange={switchMode}
              options={[
                { value: "visual", label: tr("Форма", "Form"), icon: <Wand2 className="h-3.5 w-3.5" /> },
                { value: "code", label: "JSON", icon: <Braces className="h-3.5 w-3.5" /> },
              ]}
            />
          )}
        </div>
      </div>

      {/* Body */}
      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
        {readOnly && tab === "editor" && (
          <div className="mb-4 flex flex-wrap items-center justify-between gap-2 rounded-md border border-line bg-surface-2 px-3 py-2 text-xs">
            <span className="text-ink-muted">
              {tr("Встроенные стратегии Flowseal защищены от изменений. Создайте копию, чтобы редактировать.", "Built-in Flowseal strategies are protected. Make a copy to edit.")}
            </span>
            <Button size="sm" variant="primary" icon={<Sparkles className="h-3.5 w-3.5" />} onClick={() => setPrompt("duplicate")}>
              {tr("Создать копию", "Create copy")}
            </Button>
          </div>
        )}

        {tab === "overview" && (
          <div className="space-y-3">
            <div className="text-xs text-ink-muted">
              {count(s.rules.length, ["правило", "правила", "правил"], ["rule", "rules"])} · Flowseal {s.origin.flowsealVersion ?? "—"} ·{" "}
              {errorCount ? <span className="text-bad">{count(errorCount, ["ошибка", "ошибки", "ошибок"], ["error", "errors"])}</span> : tr("ошибок нет", "no errors")}
            </div>
            <div className="overflow-hidden rounded-lg border border-line">
              {(details?.summaries ?? []).map((r, i) => {
                const targets = [...r.lists, ...r.domains];
                const shown = targets.slice(0, 4);
                const label = s.rules[i]?.label;
                return (
                  <div key={i} className={`flex gap-3 border-t border-line bg-surface px-4 py-3 first:border-t-0 ${s.rules[i]?.enabled === false ? "opacity-50" : ""}`}>
                    <span className="mt-0.5 w-5 shrink-0 font-mono text-xs text-ink-faint">{i + 1}</span>
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
                        <Badge tone={r.protocol.includes("udp") ? "warn" : "accent"}>{r.protocol.toUpperCase()}</Badge>
                        {r.tcpPorts && <span className="font-mono text-ink">TCP {r.tcpPorts}</span>}
                        {r.udpPorts && <span className="font-mono text-ink">UDP {r.udpPorts}</span>}
                        {r.l7 && <span className="text-ok">L7: {r.l7}</span>}
                        {label && <span className="text-ink-muted">— {label}</span>}
                        {r.desync && <span className="ml-auto font-mono text-accent">{r.desync}</span>}
                      </div>
                      {(targets.length > 0 || r.fakes.length > 0) && (
                        <div className="mt-1.5 space-y-0.5 text-[11px] text-ink-muted">
                          {targets.length > 0 && (
                            <div className="truncate" title={targets.join(", ")}>
                              <span className="text-ink-faint">{tr("Списки: ", "Lists: ")}</span>
                              <span className="font-mono">{shown.join(", ")}</span>
                              {targets.length > shown.length && <span className="text-ink-faint"> {tr(`и ещё ${targets.length - shown.length}`, `+${targets.length - shown.length} more`)}</span>}
                            </div>
                          )}
                          {r.fakes.length > 0 && (
                            <div className="truncate" title={r.fakes.join(", ")}>
                              <span className="text-ink-faint">{tr("Фейки: ", "Fakes: ")}</span>
                              <span className="font-mono">{r.fakes.join(", ")}</span>
                            </div>
                          )}
                        </div>
                      )}
                      {r.other.length > 0 && (
                        <div className="selectable mt-1 truncate font-mono text-[11px] text-ink-faint" title={r.other.join(" ")}>
                          {r.other.join("  ")}
                        </div>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        )}
        {tab === "editor" &&
          (mode === "visual" ? (
            <VisualEditor draft={draft} onChange={setDraft} specs={specs} resources={resources} issues={issues} readOnly={readOnly} />
          ) : (
            <div className="flex h-[calc(100vh-250px)] min-h-[360px] gap-4">
              <div className="flex min-w-0 flex-1 flex-col overflow-hidden rounded-md border border-line">
                <div className="flex h-9 shrink-0 items-center gap-2 border-b border-line bg-surface-2 px-2">
                  <span className="px-1 font-mono text-[11px] text-ink-faint">strategy.json</span>
                  <div className={`min-w-0 flex-1 truncate text-xs ${codeError ? "text-bad" : "text-ink-faint"}`} title={codeError ?? ""}>
                    {codeError ?? ""}
                  </div>
                  {!readOnly && (
                    <Button size="sm" variant="ghost" icon={<Braces className="h-3.5 w-3.5" />} onClick={formatCode}>
                      {tr("Форматировать", "Format")}
                    </Button>
                  )}
                  <Button size="sm" variant={showArgs ? "secondary" : "ghost"} aria-pressed={showArgs} onClick={() => setShowArgs(!showArgs)}>
                    {tr("Предпросмотр", "Preview")}
                  </Button>
                </div>
                <div className="cm-flat min-h-0 flex-1">
                  <CodeEditor value={code} onChange={setCode} specs={specs} readOnly={readOnly} />
                </div>
              </div>
              {showArgs && (
              <div className="flex w-72 shrink-0 flex-col overflow-hidden rounded-md border border-line">
                <div className="flex h-9 shrink-0 items-center border-b border-line bg-surface-2 px-3 text-xs font-medium text-ink-muted">{tr("Что получит winws", "What winws gets")}</div>
                <div className="min-h-0 flex-1 overflow-auto bg-sunken">
                  {(preview?.linesPreview ?? []).map((line, i) => {
                    const m = /^(# \(выключено\) )?\[(\d+)\] (.*)$/.exec(line);
                    const title = i === 0 ? tr("Перехват", "Capture") : m ? `${tr("Правило", "Rule")} ${m[2]}${m[1] ? tr(" (выключено)", " (off)") : ""}` : "";
                    const body = i === 0 ? line : m ? m[3] : line;
                    return (
                      <div key={i} className={`border-b border-line px-3 py-2 last:border-0 ${m?.[1] ? "opacity-50" : ""}`}>
                        <div className="mb-1 text-[10px] font-medium uppercase tracking-wide text-ink-faint">{title}</div>
                        <div className="selectable space-y-px font-mono text-[11px] leading-[18px] text-ink-muted">
                          {body
                            .split(" ")
                            .filter(Boolean)
                            .map((a, j) => (
                              <div key={j} className="break-all">
                                {a}
                              </div>
                            ))}
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
              )}
            </div>
          ))}

        {tab === "args" && (
          <Card className="p-4">
            <div className="label mb-2">{tr("Итоговая командная строка (ресурсы показаны как %BIN% и %LISTS%)", "Final command line (resources shown as %BIN% and %LISTS%)")}</div>
            <pre className="selectable whitespace-pre-wrap break-all font-mono text-[11.5px] leading-5 text-ink-muted">
              winws.exe {(preview?.argsPreview ?? []).map((a) => (a === "--new" ? "--new\n " : a)).join(" ")}
            </pre>
          </Card>
        )}
      </div>

      {/* Save bar */}
      {!readOnly && tab === "editor" && (
        <div className="flex items-center justify-between border-t border-line bg-surface px-5 py-2.5">
          <div className="text-xs text-ink-muted">
            {dirty ? tr("Есть несохранённые изменения", "Unsaved changes") : tr("Изменений нет", "No changes")}
            {errorCount > 0 && <span className="ml-2 text-bad">· {tr(`ошибок: ${errorCount}`, `errors: ${errorCount}`)}</span>}
          </div>
          <div className="flex gap-2">
            <Button size="sm" variant="ghost" icon={<Undo2 className="h-3.5 w-3.5" />} disabled={!dirty} onClick={reset}>
              {tr("Отменить изменения", "Discard changes")}
            </Button>
            <Button size="sm" variant="primary" icon={<Save className="h-3.5 w-3.5" />} disabled={!dirty} loading={saving} onClick={doSave}>
              {tr("Сохранить", "Save")}
            </Button>
          </div>
        </div>
      )}

      <PromptDialog
        open={prompt !== null}
        title={prompt === "rename" ? tr("Переименовать стратегию", "Rename strategy") : tr("Копия стратегии", "Copy strategy")}
        label={tr("Название", "Name")}
        initial={prompt === "rename" ? s.name : tr(`${s.name} (копия)`, `${s.name} (copy)`)}
        onClose={() => setPrompt(null)}
        onSubmit={(v) => (prompt === "rename" ? rename(v) : duplicate(v))}
      />
      <DiffDialog open={diffOpen !== null} onClose={() => setDiffOpen(null)} left={diffOpen?.l ?? s.id} right={diffOpen?.r ?? s.id} />
    </div>
  );
}
