import { FileDown, Mic, Plus, Search, Shield, TriangleAlert, Users } from "lucide-react";
import { useMemo, useState } from "react";
import { strategyKind } from "../../catalog/strategies";
import type { Source, StrategyListItem } from "../../services/types";
import { useApp, useCount, useTr } from "../../stores/app";
import { Badge, Button, Menu } from "../ui";

export type StrategyTab = "builtin" | "community" | "mine" | "import";

export function noteText(n: string, tr: (a: string, b: string) => string) {
  return /not recommended/i.test(n) ? tr("не рекомендуется", "not recommended") : n;
}

export function sourceBadge(s: Source, tr: (a: string, b: string) => string) {
  if (s === "flowseal") return <Badge tone="accent">{tr("встроенная", "built-in")}</Badge>;
  if (s === "imported") return <Badge tone="warn">{tr("импорт", "imported")}</Badge>;
  return <Badge tone="ok">{tr("моя", "mine")}</Badge>;
}

export const inTab = (s: StrategyListItem, tab: StrategyTab) => (tab === "builtin" ? s.source === "flowseal" : tab === "mine" ? s.source !== "flowseal" : tab === "import" ? s.source === "imported" : false);

const KIND_TONE = { accent: "text-accent", ok: "text-ok", warn: "text-warn", neutral: "text-ink-faint" } as const;

export function StrategyList({ tab, onTab, onCreate, onImport, onDiscordOnly }: { tab: StrategyTab; onTab: (t: StrategyTab) => void; onCreate: () => void; onImport: () => void; onDiscordOnly: () => void }) {
  const { strategies, selectedStrategyId, selectStrategy, overview, lang } = useApp();
  const tr = useTr();
  const count = useCount();
  const [q, setQ] = useState("");
  const runningId = overview?.engine?.state === "running" ? overview.engine.strategyId : null;

  const items = useMemo(() => {
    const ql = q.trim().toLowerCase();
    return strategies.filter((s) => inTab(s, tab) && (!ql || s.name.toLowerCase().includes(ql) || s.id.includes(ql)));
  }, [strategies, q, tab]);

  const groups: { title: string; items: StrategyListItem[] }[] =
    tab === "mine"
      ? [
          { title: tr("Созданные и копии", "Created and copies"), items: items.filter((s) => s.source === "custom") },
          { title: tr("Импортированные", "Imported"), items: items.filter((s) => s.source === "imported") },
        ].filter((g) => g.items.length > 0)
      : tab === "builtin"
        ? [{ title: `Flowseal ${overview?.bundle.version ?? ""}`, items }]
        : tab === "import"
          ? [{ title: tr("Импортированные", "Imported"), items }].filter((g) => g.items.length > 0)
          : [];

  return (
    <aside className="flex w-80 shrink-0 flex-col border-r border-line bg-surface/40 narrow:w-72">
      <div className="space-y-2.5 border-b border-line p-3">
        <div className="flex items-center justify-between">
          <h1 className="text-base font-semibold">{tr("Стратегии", "Strategies")}</h1>
          <div className="flex items-center gap-1">
            <Button size="sm" variant="primary" icon={<Plus className="h-3.5 w-3.5" />} onClick={onCreate} title={tr("Создать стратегию", "New strategy")}>
              {tr("Создать", "New")}
            </Button>
            <Menu
              label={tr("Импорт и шаблоны", "Import & templates")}
              items={[
                { label: tr("Импорт из файла (BAT, JSON, TOML)…", "Import from file (BAT, JSON, TOML)…"), icon: <FileDown className="h-3.5 w-3.5" />, onClick: onImport },
                { label: tr("Шаблон «Только Discord»", "“Discord only” template"), icon: <Shield className="h-3.5 w-3.5" />, onClick: onDiscordOnly },
              ]}
            />
          </div>
        </div>
        <div role="tablist" aria-label={tr("Разделы стратегий", "Strategy sections")} className="-mx-1 flex gap-0.5 border-b border-line">
          {(
            [
              ["builtin", tr("Встроенные", "Built-in")],
              ["community", tr("Сообщество", "Community")],
              ["mine", tr("Мои", "Mine")],
              ["import", tr("Импорт", "Import")],
            ] as [StrategyTab, string][]
          ).map(([v, label]) => (
            <button
              key={v}
              role="tab"
              aria-selected={tab === v}
              onClick={() => onTab(v)}
              className={`relative -mb-px whitespace-nowrap border-b-2 px-1.5 pb-2 pt-1 text-[12.5px] transition-colors ${tab === v ? "border-accent font-medium text-ink" : "border-transparent text-ink-muted hover:text-ink"}`}
            >
              {label}
            </button>
          ))}
        </div>
        {(tab === "builtin" || tab === "mine") && (
          <label className="relative block">
            <Search className="pointer-events-none absolute left-2.5 top-2 h-4 w-4 text-ink-faint" />
            <input className="field h-8 py-1 pl-8" placeholder={tr("Поиск по названию", "Search by name")} value={q} onChange={(e) => setQ(e.target.value)} aria-label={tr("Поиск", "Search")} />
          </label>
        )}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {tab === "community" && (
          <div className="flex flex-col items-center gap-2 px-4 py-10 text-center text-xs text-ink-muted">
            <Users className="h-6 w-6 text-ink-faint" />
            {tr("Стратегий сообщества пока нет", "No community strategies yet")}
          </div>
        )}
        {tab !== "community" && groups.length === 0 && (
          <div className="p-6 text-center text-xs text-ink-muted">{tab === "mine" ? tr("Своих стратегий пока нет. Создайте новую или сделайте копию встроенной.", "No own strategies yet. Create one or copy a built-in one.") : tab === "import" ? tr("Импортированных стратегий пока нет", "Nothing imported yet") : tr("Ничего не найдено", "Nothing found")}</div>
        )}
        {groups.map((g) => (
          <div key={g.title} className="mb-3">
            <div className="label px-2 pb-1 pt-2 text-ink-faint">
              {g.title} · {g.items.length}
            </div>
            <ul className="space-y-0.5">
              {g.items.map((s) => {
                const sel = s.id === selectedStrategyId;
                const kind = s.source === "flowseal" ? strategyKind(s.name, s.notes) : null;
                return (
                  <li key={s.id}>
                    <button
                      onClick={() => selectStrategy(s.id)}
                      aria-current={sel ? "true" : undefined}
                      className={`group flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left transition-colors ${sel ? "bg-surface-3" : "hover:bg-surface-2"}`}
                    >
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-1.5">
                          {runningId === s.id && <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-ok" title={tr("работает сейчас", "running now")} />}
                          <span className={`truncate text-[13px] ${sel ? "font-medium text-ink" : "text-ink-muted group-hover:text-ink"}`}>{s.name}</span>
                        </div>
                        <div className="flex items-center gap-1.5 text-[11px] text-ink-faint">
                          {kind && <span className={KIND_TONE[kind.tone]}>{lang === "en" ? kind.en : kind.ru}</span>}
                          {kind && <span>·</span>}
                          {count(s.ruleCount, ["правило", "правила", "правил"], ["rule", "rules"])}
                          {s.hasVoiceRule && <Mic className="h-3 w-3" aria-label={tr("есть правило для голоса", "has voice rule")} />}
                        </div>
                      </div>
                      {s.draft && (
                        <span className="flex shrink-0 items-center gap-1 text-[10px] text-warn" title={tr("черновик: есть ошибки, запуск невозможен", "draft: has errors, cannot start")}>
                          <TriangleAlert className="h-3.5 w-3.5" />
                          {tr("черновик", "draft")}
                        </span>
                      )}
                    </button>
                  </li>
                );
              })}
            </ul>
          </div>
        ))}
      </div>
    </aside>
  );
}
