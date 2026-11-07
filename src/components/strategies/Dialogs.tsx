import { open } from "@tauri-apps/plugin-dialog";
import { ArrowLeftRight, FileWarning, Info, ListOrdered } from "lucide-react";
import { useEffect, useState } from "react";
import { api, errorText } from "../../services/api";
import type { DiffLine, ImportPreview, StrategyDiff } from "../../services/types";
import { useApp, useCount, useTr } from "../../stores/app";
import { Badge, Button, Checkbox, Modal, Select } from "../ui";

export function PromptDialog({ open: isOpen, title, label, initial, onClose, onSubmit }: { open: boolean; title: string; label: string; initial: string; onClose: () => void; onSubmit: (v: string) => void }) {
  const [v, setV] = useState(initial);
  useEffect(() => setV(initial), [initial, isOpen]);
  const tr = useTr();
  return (
    <Modal
      open={isOpen}
      onClose={onClose}
      title={title}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {tr("Отмена", "Cancel")}
          </Button>
          <Button variant="primary" disabled={!v.trim()} onClick={() => onSubmit(v.trim())}>
            {tr("Готово", "Done")}
          </Button>
        </>
      }
    >
      <label className="block">
        <span className="label">{label}</span>
        <input
          autoFocus
          className="field mt-1"
          value={v}
          maxLength={100}
          onChange={(e) => setV(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && v.trim() && onSubmit(v.trim())}
        />
      </label>
    </Modal>
  );
}

function DiffLines({ lines }: { lines: DiffLine[] }) {
  return (
    <div className="selectable overflow-x-auto rounded-lg border border-line bg-sunken font-mono text-[11.5px] leading-5">
      {lines.map((l, i) => (
        <div key={i} className={l.op === "added" ? "bg-ok/10 text-ok" : l.op === "removed" ? "bg-bad/10 text-bad" : "text-ink-muted"}>
          <span className="inline-block w-5 select-none text-center opacity-60">{l.op === "added" ? "+" : l.op === "removed" ? "−" : " "}</span>
          {l.text}
        </div>
      ))}
      {lines.length === 0 && <div className="px-3 py-1 text-ink-faint">—</div>}
    </div>
  );
}

const FIELD_RU: Record<string, string> = {
  tcp: "TCP-порты",
  udp: "UDP-порты",
  l7: "Протоколы L7",
  lists: "Списки",
  domains: "Домены",
  fakes: "Fake-файлы",
  desync: "Desync",
  params: "Параметры",
  enabled: "Включено",
  name: "Название",
  description: "Описание",
  tcpTimestamps: "TCP timestamps",
};

export function DiffDialog({ open: isOpen, onClose, left: initialLeft, right: initialRight }: { open: boolean; onClose: () => void; left: string; right: string }) {
  const { strategies, toast } = useApp();
  const tr = useTr();
  const [left, setLeft] = useState(initialLeft);
  const [right, setRight] = useState(initialRight);
  const [diff, setDiff] = useState<StrategyDiff | null>(null);
  useEffect(() => {
    setLeft(initialLeft);
    setRight(initialRight);
  }, [initialLeft, initialRight, isOpen]);
  useEffect(() => {
    if (!isOpen || !left || !right) return;
    api.compare(left, right).then(setDiff).catch((e) => toast("error", errorText(e)));
  }, [isOpen, left, right, toast]);
  const opts = strategies.map((s) => ({ value: s.id, label: s.name, group: s.source === "flowseal" ? "Flowseal" : tr("Свои и импорт", "Custom & imported") }));
  return (
    <Modal open={isOpen} onClose={onClose} title={tr("Сравнение стратегий", "Compare strategies")} wide>
      <div className="mb-4 flex items-center gap-3">
        <Select ariaLabel={tr("Левая стратегия", "Left strategy")} value={left} onChange={setLeft} options={opts} />
        <ArrowLeftRight className="h-4 w-4 shrink-0 text-ink-faint" />
        <Select ariaLabel={tr("Правая стратегия", "Right strategy")} value={right} onChange={setRight} options={opts} />
      </div>
      {!diff ? (
        <div className="text-sm text-ink-muted">{tr("Загрузка…", "Loading…")}</div>
      ) : diff.identical ? (
        <div className="rounded-md border border-ok/30 bg-ok-soft px-4 py-3 text-sm text-ok">{tr("Конфигурации идентичны.", "Configurations are identical.")}</div>
      ) : (
        <div className="space-y-4">
          {diff.orderChanged && (
            <div className="flex items-center gap-2 rounded-md border border-warn/30 bg-warn-soft px-3 py-2 text-xs text-warn">
              <ListOrdered className="h-4 w-4" /> {tr("Изменён порядок правил — это влияет на то, какое правило сработает первым.", "Rule order changed — affects which rule matches first.")}
            </div>
          )}
          {diff.meta.length > 0 && (
            <table className="w-full text-xs">
              <tbody>
                {diff.meta.map((c) => (
                  <tr key={c.field} className="border-b border-line">
                    <td className="w-40 py-1.5 text-ink-faint">{FIELD_RU[c.field] ?? c.field}</td>
                    <td className="py-1.5 text-bad">{c.left}</td>
                    <td className="py-1.5 text-ok">{c.right}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <div>
            <div className="label mb-1.5">{tr("Перехват", "Capture")}</div>
            <DiffLines lines={diff.global} />
          </div>
          {diff.rules.map((r, i) => {
            const changed = r.changes.length > 0 || r.args.some((a) => a.op !== "same");
            return (
              <div key={i} className="rounded-md border border-line p-3">
                <div className="mb-2 flex items-center gap-2 text-xs">
                  <span className="font-medium">
                    {tr("Правило", "Rule")} {r.leftIndex ?? "—"} → {r.rightIndex ?? "—"}
                  </span>
                  {!r.leftIndex && <Badge tone="ok">{tr("добавлено", "added")}</Badge>}
                  {!r.rightIndex && <Badge tone="bad">{tr("удалено", "removed")}</Badge>}
                  {r.leftIndex && r.rightIndex && !changed && <Badge>{tr("без изменений", "unchanged")}</Badge>}
                </div>
                {r.changes.length > 0 && (
                  <table className="mb-2 w-full text-xs">
                    <tbody>
                      {r.changes.map((c) => (
                        <tr key={c.field}>
                          <td className="w-32 py-0.5 text-ink-faint">{FIELD_RU[c.field] ?? c.field}</td>
                          <td className="selectable py-0.5 font-mono text-bad">{c.left}</td>
                          <td className="selectable py-0.5 font-mono text-ok">{c.right}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
                {changed && <DiffLines lines={r.args} />}
              </div>
            );
          })}
        </div>
      )}
    </Modal>
  );
}

export function ImportDialog({ open: isOpen, onClose, onImported }: { open: boolean; onClose: () => void; onImported: (id: string) => void }) {
  const { toast } = useApp();
  const tr = useTr();
  const count = useCount();
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [accept, setAccept] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!isOpen) {
      setPreview(null);
      setAccept(false);
    }
  }, [isOpen]);

  const pick = async () => {
    const path = await open({ multiple: false, filters: [{ name: tr("Стратегии", "Strategies"), extensions: ["bat", "cmd", "json", "toml"] }] });
    if (!path || Array.isArray(path)) return;
    try {
      setPreview(await api.importPreview(path));
      setAccept(false);
    } catch (e) {
      toast("error", errorText(e));
    }
  };
  const commit = async () => {
    if (!preview) return;
    setBusy(true);
    try {
      const s = await api.importCommit(preview.token, preview.strategy.name, accept);
      toast("ok", tr(`Стратегия «${s.name}» импортирована`, `Strategy "${s.name}" imported`));
      onImported(s.id);
      onClose();
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const errors = preview?.issues.filter((i) => i.severity === "error") ?? [];
  return (
    <Modal
      open={isOpen}
      onClose={onClose}
      title={tr("Импорт стратегии", "Import strategy")}
      wide
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {tr("Отмена", "Cancel")}
          </Button>
          <Button variant="primary" loading={busy} disabled={!preview || (preview.unsupported.length > 0 && !accept)} onClick={commit}>
            {tr("Импортировать", "Import")}
          </Button>
        </>
      }
    >
      <div className="space-y-4 text-sm">
        <div className="flex items-start gap-3 rounded-md border border-line bg-surface-2 p-3 text-xs text-ink-muted">
          <Info className="mt-0.5 h-4 w-4 shrink-0 text-accent" />
          <div>
            {tr(
              "Поддерживаются BAT/CMD стратегии Flowseal/zapret, JSON и TOML Bridge. Файлы только анализируются: команды из BAT никогда не выполняются, пути к файлам вне ресурсов Flowseal отклоняются.",
              "Flowseal/zapret BAT/CMD, Bridge JSON and TOML are supported. Files are only parsed: BAT commands are never executed and paths outside Flowseal resources are rejected.",
            )}
          </div>
        </div>
        <Button onClick={pick}>{preview ? tr("Выбрать другой файл", "Choose another file") : tr("Выбрать файл…", "Choose file…")}</Button>
        {preview && (
          <div className="space-y-3">
            <div className="flex items-center gap-2">
              <Badge tone="accent">{preview.format.toUpperCase()}</Badge>
              <input className="field max-w-sm" value={preview.strategy.name} onChange={(e) => setPreview({ ...preview, strategy: { ...preview.strategy, name: e.target.value } })} aria-label={tr("Название", "Name")} />
              <span className="text-xs text-ink-faint">{count(preview.strategy.rules.length, ["правило", "правила", "правил"], ["rule", "rules"])}</span>
              {preview.migratedFrom != null && <Badge tone="warn">{tr(`схема v${preview.migratedFrom} → v1`, `schema v${preview.migratedFrom} → v1`)}</Badge>}
            </div>
            {preview.unsupported.length > 0 && (
              <div className="rounded-md border border-bad/30 bg-bad-soft p-3">
                <div className="mb-2 flex items-center gap-2 text-sm font-medium text-bad">
                  <FileWarning className="h-4 w-4" /> {tr("Неподдерживаемые команды (не будут импортированы и не выполнялись)", "Unsupported commands (not imported, never executed)")}
                </div>
                <ul className="selectable max-h-40 space-y-1 overflow-auto font-mono text-[11px]">
                  {preview.unsupported.map((u, i) => (
                    <li key={i}>
                      <span className="text-ink-faint">{tr("стр.", "line")} {u.line}:</span> {u.text} <span className="font-sans text-bad/80">— {u.reason}</span>
                    </li>
                  ))}
                </ul>
                <Checkbox className="mt-3" checked={accept} onChange={setAccept} title={tr("Импортировать только поддерживаемые части (параметры winws)", "Import only the supported parts (winws options)")} />
              </div>
            )}
            {preview.notes.length > 0 && (
              <ul className="space-y-1 text-xs text-ink-muted">
                {preview.notes.map((n, i) => (
                  <li key={i}>
                    {tr("стр.", "line")} {n.line}: {n.reason}
                  </li>
                ))}
              </ul>
            )}
            {preview.issues.length > 0 && (
              <div>
                <div className="label mb-1">
                  {tr("Проверка", "Validation")}: {count(errors.length, ["ошибка", "ошибки", "ошибок"], ["error", "errors"])}, {count(preview.issues.length - errors.length, ["предупреждение", "предупреждения", "предупреждений"], ["warning", "warnings"])}
                </div>
                <ul className="max-h-48 space-y-1 overflow-auto">
                  {preview.issues.map((i, n) => (
                    <li key={n} className={`rounded-lg px-3 py-1 text-xs ${i.severity === "error" ? "bg-bad-soft text-bad" : "bg-warn-soft text-warn"}`}>
                      <span className="font-mono opacity-70">{i.path}</span> — {i.message}
                    </li>
                  ))}
                </ul>
                {errors.length > 0 && <p className="mt-1 text-xs text-ink-muted">{tr("Стратегию можно сохранить и исправить в редакторе; запустить её до исправления нельзя.", "You can save and fix it in the editor; it cannot run until fixed.")}</p>}
              </div>
            )}
          </div>
        )}
      </div>
    </Modal>
  );
}
