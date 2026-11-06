import { ArrowDown, ArrowUp, Copy, Plus, Trash2 } from "lucide-react";
import { useMemo } from "react";
import type { Arg, ArgValue, Issue, OptionSpec, Rule, Strategy } from "../../services/types";
import { useApp, useTr } from "../../stores/app";
import { Badge, Button, Checkbox, Menu, NumberField, Select, Switch, Tooltip } from "../ui";
import { CATEGORY_EN, CATEGORY_RU, clone, defaultValue, issuesFor, moveItem, valueText } from "./editorUtils";

interface Props {
  draft: Strategy;
  onChange: (s: Strategy) => void;
  specs: OptionSpec[];
  resources: { fakes: string[]; lists: string[] };
  issues: Issue[];
  readOnly: boolean;
}

function IssueText({ issues }: { issues: Issue[] }) {
  if (issues.length === 0) return null;
  return (
    <div className="mt-1 space-y-0.5">
      {issues.map((i, n) => (
        <div key={n} className={`text-[11px] ${i.severity === "error" ? "text-bad" : "text-warn"}`}>
          {i.message}
        </div>
      ))}
    </div>
  );
}

function ValueEditor({ arg, spec, resources, onChange, invalid, readOnly }: { arg: Arg; spec?: OptionSpec; resources: Props["resources"]; onChange: (v: ArgValue | undefined) => void; invalid: boolean; readOnly: boolean }) {
  const tr = useTr();
  const cls = `field ${invalid ? "field-error" : ""}`;
  if (!spec) return <input className={cls} value={valueText(arg.value)} disabled={readOnly} onChange={(e) => onChange(e.target.value)} aria-label={arg.opt} />;
  const k = spec.kind;
  switch (k.type) {
    case "flag":
      return <span className="text-xs text-ink-faint">{tr("без значения", "no value")}</span>;
    case "int":
      return <NumberField className="w-36" min={k.min} max={k.max} invalid={invalid} value={valueText(arg.value)} disabled={readOnly} onCommit={(v) => onChange(v)} ariaLabel={spec.titleRu} />;
    case "enum": {
      const cur = valueText(arg.value).split(",").filter(Boolean);
      if (!k.multi)
        return <Select ariaLabel={spec.titleRu} disabled={readOnly} value={cur[0] ?? ""} onChange={(v) => onChange(v)} options={k.values.map((v) => ({ value: v, label: v }))} className="w-48" />;
      return (
        <div className="flex flex-wrap gap-1" role="group" aria-label={spec.titleRu}>
          {k.values.map((v) => {
            const on = cur.includes(v);
            return (
              <button
                key={v}
                type="button"
                disabled={readOnly}
                aria-pressed={on}
                onClick={() => {
                  const next = on ? cur.filter((x) => x !== v) : [...cur, v];
                  onChange(next.join(","));
                }}
                className={`rounded-md border px-2 py-0.5 font-mono text-[11px] transition-colors ${on ? "border-accent/50 bg-accent-soft text-accent" : "border-line text-ink-muted hover:border-line-strong hover:text-ink"}`}
              >
                {v}
              </button>
            );
          })}
        </div>
      );
    }
    case "fake": {
      const isFile = typeof arg.value === "object" && arg.value !== null && "fake" in arg.value;
      const mode = isFile ? "file" : valueText(arg.value) === "!" ? "bang" : "hex";
      return (
        <div className="flex gap-2">
          <Select
            ariaLabel={spec.titleRu}
            disabled={readOnly}
            className="w-full max-w-sm"
            value={isFile ? `file:${valueText(arg.value)}` : mode}
            onChange={(v) => {
              if (v.startsWith("file:")) onChange({ fake: v.slice(5) });
              else if (v === "bang") onChange("!");
              else onChange("0x00");
            }}
            options={[
              ...resources.fakes.map((f) => ({ value: `file:${f}`, label: f, group: tr("Файлы bin", "bin files") })),
              { value: "hex", label: tr("HEX-значение…", "Hex value…"), group: tr("Другое", "Other") },
              ...(k.allowBang ? [{ value: "bang", label: tr("! — встроенный фейк winws", "! — winws built-in"), group: tr("Другое", "Other") }] : []),
            ]}
          />
          {mode === "hex" && <input className={`${cls} max-w-[12rem] font-mono`} value={valueText(arg.value)} disabled={readOnly} onChange={(e) => onChange(e.target.value)} placeholder="0x00" aria-label="hex" />}
        </div>
      );
    }
    case "list":
      return (
        <Select
          ariaLabel={spec.titleRu}
          disabled={readOnly}
          className="w-full max-w-sm"
          value={valueText(arg.value)}
          onChange={(v) => onChange({ list: v })}
          options={resources.lists.map((l) => ({ value: l, label: l }))}
        />
      );
    default:
      return (
        <input
          className={`${cls} font-mono`}
          value={valueText(arg.value)}
          disabled={readOnly}
          onChange={(e) => onChange(e.target.value)}
          aria-label={spec.titleRu}
          placeholder={k.type === "ports" ? "80,443,50000-50100" : k.type === "markers" ? "1,midsld" : k.type === "cutoff" ? "n3" : ""}
        />
      );
  }
}

function ArgRows({ args, base, scope, specs, specMap, resources, issues, readOnly, onChange }: {
  args: Arg[];
  base: string;
  scope: "global" | "rule";
  specs: OptionSpec[];
  specMap: Map<string, OptionSpec>;
  resources: Props["resources"];
  issues: Issue[];
  readOnly: boolean;
  onChange: (a: Arg[]) => void;
}) {
  const { lang } = useApp();
  const tr = useTr();
  const options = specs
    .filter((s) => s.scope === scope)
    .map((s) => ({ value: s.name, label: lang === "en" ? s.titleEn : s.titleRu, group: lang === "en" ? CATEGORY_EN[s.category] : CATEGORY_RU[s.category] }));
  const addArg = () => {
    const used = new Set(args.map((a) => a.opt));
    const spec = specs.find((s) => s.scope === scope && (s.repeatable || !used.has(s.name)));
    if (!spec) return;
    onChange([...args, { opt: spec.name, value: defaultValue(spec.kind, resources) }]);
  };
  return (
    <div className="space-y-1.5">
      {args.map((a, i) => {
        const spec = specMap.get(a.opt);
        const path = `${base}[${i}]`;
        const iss = issuesFor(issues, path);
        const err = iss.some((x) => x.severity === "error");
        return (
          <div key={i} className={`group rounded-md border px-3 py-2 ${err ? "border-bad/40 bg-bad-soft" : "border-line bg-surface-2"}`}>
            <div className="flex items-center gap-1.5">
              <Select
                ariaLabel={tr("Параметр", "Option")}
                disabled={readOnly}
                className="h-7 w-auto max-w-full flex-1 border-transparent bg-transparent px-1 py-0 text-[13px] font-medium hover:border-line"
                value={a.opt}
                onChange={(opt) => {
                  const sp = specMap.get(opt);
                  const next = args.slice();
                  next[i] = { opt, value: sp ? defaultValue(sp.kind, resources) : a.value };
                  onChange(next);
                }}
                options={specMap.has(a.opt) ? options : [{ value: a.opt, label: `${a.opt} (${tr("не поддерживается", "unsupported")})` }, ...options]}
              />
              {spec && <Tooltip text={`${a.opt}: ${lang === "en" ? spec.helpEn : spec.helpRu}`} />}
              <span className="ml-auto hidden font-mono text-[11px] text-ink-faint sm:inline">{a.opt}</span>
              {!readOnly && (
                <div className="flex shrink-0 gap-0.5 text-ink-muted">
                  <button className="rounded p-1 hover:bg-surface-3 hover:text-ink" aria-label={tr("Выше", "Up")} onClick={() => onChange(moveItem(args, i, i - 1))}>
                    <ArrowUp className="h-3.5 w-3.5" />
                  </button>
                  <button className="rounded p-1 hover:bg-surface-3 hover:text-ink" aria-label={tr("Ниже", "Down")} onClick={() => onChange(moveItem(args, i, i + 1))}>
                    <ArrowDown className="h-3.5 w-3.5" />
                  </button>
                  <button className="rounded p-1 hover:bg-bad-soft hover:text-bad" aria-label={tr("Удалить параметр", "Remove option")} onClick={() => onChange(args.filter((_, j) => j !== i))}>
                    <Trash2 className="h-3.5 w-3.5" />
                  </button>
                </div>
              )}
            </div>
            <div className="mt-1.5">
              <ValueEditor
                arg={a}
                spec={spec}
                resources={resources}
                invalid={err}
                readOnly={readOnly}
                onChange={(v) => {
                  const next = args.slice();
                  next[i] = v === undefined ? { opt: a.opt } : { opt: a.opt, value: v };
                  onChange(next);
                }}
              />
              <IssueText issues={iss} />
            </div>
          </div>
        );
      })}
      {!readOnly && (
        <Button size="sm" variant="ghost" icon={<Plus className="h-3.5 w-3.5" />} onClick={addArg}>
          {tr("Добавить параметр", "Add option")}
        </Button>
      )}
    </div>
  );
}

export function VisualEditor({ draft, onChange, specs, resources, issues, readOnly }: Props) {
  const tr = useTr();
  const specMap = useMemo(() => new Map(specs.map((s) => [s.name, s])), [specs]);
  const set = (f: (s: Strategy) => void) => {
    const next = clone(draft);
    f(next);
    onChange(next);
  };
  const setRule = (i: number, r: Rule) => set((s) => (s.rules[i] = r));

  return (
    <div className="space-y-5">
      <section className="grid grid-cols-2 gap-x-4 gap-y-3">
        <label className="block">
          <span className="label">{tr("Название", "Name")}</span>
          <input className={`field mt-1 ${issuesFor(issues, "name").length ? "field-error" : ""}`} value={draft.name} disabled={readOnly} onChange={(e) => set((s) => (s.name = e.target.value))} />
          <IssueText issues={issuesFor(issues, "name")} />
        </label>
        <label className="block">
          <span className="label">{tr("Описание", "Description")}</span>
          <input className="field mt-1" value={draft.description} disabled={readOnly} onChange={(e) => set((s) => (s.description = e.target.value))} />
        </label>
        <div className="col-span-2 flex items-center gap-2">
          <Checkbox
            checked={draft.requirements.tcpTimestamps}
            disabled={readOnly}
            onChange={(v) => set((s) => (s.requirements.tcpTimestamps = v))}
            title={<span className="text-[13px] font-normal">{tr("Стратегии нужны TCP timestamps Windows", "Strategy needs Windows TCP timestamps")}</span>}
          />
          <Tooltip text={tr("Отметьте для стратегий с --dpi-desync-fooling=ts. Сама настройка Windows меняется только с вашего согласия.", "Tick for strategies using --dpi-desync-fooling=ts. The Windows setting itself is only changed with your consent.")} />
        </div>
      </section>

      <section className="rounded-lg border border-line bg-surface p-4">
        <div className="mb-3 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <h3 className="text-sm font-semibold">{tr("Перехват трафика (WinDivert)", "Traffic capture (WinDivert)")}</h3>
            <Tooltip text={tr("Глобальные параметры: какие порты вообще передаются движку. Всё, что не перехвачено, Zapret не трогает — это ограничивает влияние на другие приложения.", "Global options: which ports reach the engine at all. Anything not captured is untouched, limiting impact on other apps.")} />
          </div>
        </div>
        <ArgRows args={draft.global} base="global" scope="global" specs={specs} specMap={specMap} resources={resources} issues={issues} readOnly={readOnly} onChange={(a) => set((s) => (s.global = a))} />
        <IssueText issues={issuesFor(issues, "global")} />
      </section>

      <section className="space-y-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <h3 className="text-sm font-semibold">{tr("Правила", "Rules")}</h3>
            <Tooltip text={tr("Правила проверяются по порядку сверху вниз; пакет обрабатывает первое подходящее правило. Порядок важен.", "Rules are evaluated top to bottom; the first matching rule handles a packet. Order matters.")} />
          </div>
          {!readOnly && (
            <Button
              size="sm"
              icon={<Plus className="h-3.5 w-3.5" />}
              onClick={() =>
                set((s) =>
                  s.rules.push({ enabled: true, args: [{ opt: "--filter-tcp", value: "443" }, { opt: "--dpi-desync", value: "multisplit" }, { opt: "--dpi-desync-split-pos", value: "1" }] }),
                )
              }
            >
              {tr("Добавить правило", "Add rule")}
            </Button>
          )}
        </div>
        <IssueText issues={issuesFor(issues, "rules")} />
        {draft.rules.map((r, i) => {
          const rIssues = issuesFor(issues, `rules[${i}]`, true);
          const proto = r.args.some((a) => a.opt === "--filter-udp") ? "UDP" : r.args.some((a) => a.opt === "--filter-tcp") ? "TCP" : tr("любой", "any");
          const ports = valueText(r.args.find((a) => a.opt === "--filter-udp" || a.opt === "--filter-tcp")?.value);
          return (
            <div key={i} className={`rounded-lg border bg-surface p-4 ${r.enabled ? "border-line" : "border-dashed border-line opacity-60"}`}>
              <div className="mb-3 flex items-center gap-2.5">
                <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md bg-surface-3 font-mono text-xs text-ink-muted">{i + 1}</span>
                <Badge tone={proto === "UDP" ? "warn" : "accent"}>{proto}</Badge>
                <span className="max-w-[140px] truncate font-mono text-xs text-ink-muted" title={ports}>
                  {ports}
                </span>
                <input
                  className="field h-7 min-w-0 max-w-[240px] flex-1 border-transparent bg-transparent py-0.5 text-xs hover:border-line focus:bg-surface-2"
                  placeholder={readOnly ? "" : tr("Добавить метку…", "Add a label…")}
                  value={r.label ?? ""}
                  disabled={readOnly}
                  onChange={(e) => setRule(i, { ...r, label: e.target.value || undefined })}
                  aria-label={tr("Метка правила", "Rule label")}
                />
                <div className="ml-auto flex shrink-0 items-center gap-1.5">
                  <Switch label={tr("Правило включено", "Rule enabled")} checked={r.enabled} disabled={readOnly} onChange={(v) => setRule(i, { ...r, enabled: v })} />
                  {!readOnly && (
                    <Menu
                      label={tr("Действия с правилом", "Rule actions")}
                      items={[
                        { label: tr("Переместить выше", "Move up"), icon: <ArrowUp className="h-3.5 w-3.5" />, onClick: () => set((s) => (s.rules = moveItem(s.rules, i, i - 1))), hidden: i === 0 },
                        { label: tr("Переместить ниже", "Move down"), icon: <ArrowDown className="h-3.5 w-3.5" />, onClick: () => set((s) => (s.rules = moveItem(s.rules, i, i + 1))), hidden: i === draft.rules.length - 1 },
                        { label: tr("Дублировать", "Duplicate"), icon: <Copy className="h-3.5 w-3.5" />, onClick: () => set((s) => s.rules.splice(i + 1, 0, clone(r))) },
                        { label: tr("Удалить правило", "Delete rule"), icon: <Trash2 className="h-3.5 w-3.5" />, onClick: () => set((s) => s.rules.splice(i, 1)), danger: true },
                      ]}
                    />
                  )}
                </div>
              </div>
              <IssueText issues={rIssues} />
              <ArgRows args={r.args} base={`rules[${i}].args`} scope="rule" specs={specs} specMap={specMap} resources={resources} issues={issues} readOnly={readOnly} onChange={(a) => setRule(i, { ...r, args: a })} />
            </div>
          );
        })}
      </section>
    </div>
  );
}
