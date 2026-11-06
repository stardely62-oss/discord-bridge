// Service cards: compact card + details (Overview · Domains · IP · Technical).
// Facts come from Rust (bridge_core::services); see src/catalog/services.ts.
import { CheckCircle2, CircleDashed, Copy, FileText, Layers, Minus, Search, TriangleAlert } from "lucide-react";
import { ReactNode, useEffect, useMemo, useReducer, useRef, useState } from "react";
import { loc } from "../../catalog/apps";
import { applyLabel, groupDomains, ladder, matchText, rulePorts, ruleProtocols, sourceText, type Tone } from "../../catalog/services";
import { Badge, Button, Card, Modal, Segmented } from "../../components/ui";
import { api, errorText } from "../../services/api";
import type { CardDetails, CardSummary, CatalogPack, DomainInfo, IpsetMode, SourceFile } from "../../services/types";
import { useApp, useCount, useTr } from "../../stores/app";
import { access } from "../Home/status";

type DetailsTab = "overview" | "domains" | "ips" | "tech";

const MODE_RU: Record<IpsetMode, string> = { none: "без IP (заглушка Flowseal)", selected: "выбранные IP", flowseal: "полный список Flowseal", flowsealPlusSelected: "Flowseal + выбранные", any: "все адреса" };
const MODE_EN: Record<IpsetMode, string> = { none: "no IPs (Flowseal placeholder)", selected: "selected IPs", flowseal: "full Flowseal list", flowsealPlusSelected: "Flowseal + selected", any: "all addresses" };

async function copy(text: string, ok: string, toast: (k: "ok" | "error", t: string) => void) {
  try {
    await navigator.clipboard.writeText(text);
    toast("ok", ok);
  } catch (e) {
    toast("error", errorText(e));
  }
}

function ApplyChip({ summary }: { summary: CardSummary | undefined }) {
  const tr = useTr();
  if (!summary) return <Badge>{tr("состояние неизвестно", "state unknown")}</Badge>;
  const l = applyLabel(summary.state, tr);
  return (
    <span title={l.hint}>
      <Badge tone={l.tone}>{l.label}</Badge>
    </span>
  );
}

/** Compact card for a service covered by Flowseal (no install action). */
export function ServiceCard({ pack, icon, summary, onOpen }: { pack: CatalogPack; icon: ReactNode; summary: CardSummary | undefined; onOpen: () => void }) {
  const tr = useTr();
  const count = useCount();
  const { lang } = useApp();
  const name = loc(pack.name, lang);
  const direct = summary?.domains ?? pack.domains.length;
  return (
    <Card className="flex flex-col p-4 transition-colors hover:border-line-strong">
      <button className="flex items-start gap-3 text-left" onClick={onOpen} aria-label={tr(`${name}: подробности`, `${name}: details`)}>
        {icon}
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-semibold hover:underline">{name}</div>
          <div className="flex flex-wrap items-center gap-1.5 text-xs text-ink-faint">
            <Badge>{tr("Встроено в Flowseal", "Built into Flowseal")}</Badge>
            <span>{pack.flowsealList}</span>
          </div>
        </div>
      </button>
      <p className="mt-2.5 text-[13px] leading-relaxed text-ink-muted">{loc(pack.description, lang)}</p>
      <div className="mt-2 space-y-0.5 text-[11px] text-ink-faint">
        <div>
          {count(direct, ["домен сервиса", "домена сервиса", "доменов сервиса"], ["service domain", "service domains"])}
          {summary && summary.inFlowseal < direct && tr(` · в списках Flowseal ${summary.inFlowseal}`, ` · ${summary.inFlowseal} in Flowseal lists`)}
          {summary && summary.shared > 0 && tr(` · общих с другими ${summary.shared}`, ` · ${summary.shared} shared`)}
        </div>
        {summary && summary.extraRules > 0 && <div>{tr(`Дополнительные правила стратегии: ${summary.extraRules}`, `Additional strategy rules: ${summary.extraRules}`)}</div>}
      </div>
      <div className="mt-auto flex items-center justify-between gap-2 pt-3">
        <ApplyChip summary={summary} />
        <Button size="sm" variant="ghost" onClick={onOpen}>
          {tr("Подробнее", "Details")}
        </Button>
      </div>
    </Card>
  );
}

// ---------------------------------------------------------------- list viewer

const ROW = 22;
const VIEW_H = 340;
const PAGE = 200;

/** Original file of the active Flowseal version: windowed, searchable, copyable. */
export function ListViewer({ name, onClose }: { name: string | null; onClose: () => void }) {
  const tr = useTr();
  const { toast } = useApp();
  const [q, setQ] = useState("");
  const [query, setQuery] = useState("");
  const [matched, setMatched] = useState<number | null>(null);
  const [total, setTotal] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [top, setTop] = useState(0);
  const pages = useRef(new Map<number, [number, string][]>());
  const gen = useRef(0);
  const [, redraw] = useReducer((x: number) => x + 1, 0);

  useEffect(() => {
    const t = setTimeout(() => setQuery(q), 200);
    return () => clearTimeout(t);
  }, [q]);

  useEffect(() => {
    gen.current += 1;
    pages.current = new Map();
    setMatched(null);
    setError(null);
    setTop(0);
    redraw();
  }, [name, query]);

  const first = Math.floor(top / ROW);
  const visible = Math.ceil(VIEW_H / ROW) + 4;
  useEffect(() => {
    if (!name) return;
    const g = gen.current;
    const want = new Set([Math.floor(first / PAGE), Math.floor((first + visible) / PAGE)]);
    for (const p of want) {
      if (pages.current.has(p)) continue;
      pages.current.set(p, []);
      api
        .flowsealListPage(name, query, p * PAGE, PAGE)
        .then((r) => {
          if (g !== gen.current) return;
          pages.current.set(p, r.lines);
          setMatched(r.matched);
          setTotal(r.total);
          redraw();
        })
        .catch((e) => g === gen.current && setError(errorText(e)));
    }
  }, [name, query, first, visible, matched]);

  const rows: ReactNode[] = [];
  if (matched) {
    for (let i = first; i < Math.min(matched, first + visible); i++) {
      const line = pages.current.get(Math.floor(i / PAGE))?.[i % PAGE];
      rows.push(
        <div key={i} className="absolute left-0 right-0 flex gap-3 px-2 font-mono text-[12px]" style={{ top: i * ROW, height: ROW, lineHeight: `${ROW}px` }}>
          <span className="w-14 shrink-0 select-none text-right text-ink-faint">{line ? line[0] : ""}</span>
          <span className="selectable whitespace-pre">{line ? line[1] || " " : "…"}</span>
        </div>,
      );
    }
  }
  return (
    <Modal open={name !== null} onClose={onClose} wide title={name ?? ""}>
      <div className="space-y-3">
        <div className="flex flex-wrap items-center gap-2">
          <label className="relative min-w-[200px] flex-1">
            <Search className="pointer-events-none absolute left-2.5 top-2 h-4 w-4 text-ink-faint" />
            <input className="field h-8 py-1 pl-8" value={q} onChange={(e) => setQ(e.target.value)} placeholder={tr("Поиск по строкам", "Search lines")} aria-label={tr("Поиск по строкам", "Search lines")} />
          </label>
          <span className="text-xs text-ink-muted">
            {total === null ? tr("загрузка…", "loading…") : query ? tr(`найдено ${matched ?? 0} из ${total} строк`, `${matched ?? 0} of ${total} lines`) : tr(`${total} строк`, `${total} lines`)}
          </span>
          <Button
            size="sm"
            icon={<Copy className="h-3.5 w-3.5" />}
            disabled={!name}
            onClick={async () => {
              try {
                const t = await api.flowsealListText(name!);
                await copy(t, tr("Файл скопирован полностью, без изменений", "The whole file was copied unchanged"), toast);
              } catch (e) {
                toast("error", errorText(e));
              }
            }}
          >
            {tr("Копировать весь файл", "Copy whole file")}
          </Button>
        </div>
        {error ? (
          <div className="text-sm text-bad">{error}</div>
        ) : (
          <div
            tabIndex={0}
            role="region"
            aria-label={tr(`Содержимое ${name}`, `Contents of ${name}`)}
            className="relative overflow-auto rounded-md border border-line bg-surface-2 outline-none focus-visible:ring-2 focus-visible:ring-accent/40"
            style={{ height: VIEW_H }}
            onScroll={(e) => setTop(e.currentTarget.scrollTop)}
          >
            <div style={{ height: (matched ?? 0) * ROW, position: "relative" }}>{rows}</div>
            {matched === 0 && <div className="p-3 text-xs text-ink-muted">{tr("Нет строк", "No lines")}</div>}
          </div>
        )}
        <p className="text-[11px] text-ink-faint">{tr("Оригинальный файл активной версии Flowseal, проверенный по хешу манифеста. Строки показаны без изменений и сокращений.", "The original file of the active Flowseal version, checked against the manifest hash. Lines are shown unchanged and in full.")}</p>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------- details

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="space-y-2">
      <h3 className="label uppercase tracking-wide text-ink-faint">{title}</h3>
      {children}
    </section>
  );
}

function Step({ done, title, text }: { done: boolean | null; title: string; text: string }) {
  return (
    <li className="flex items-start gap-2.5">
      {done === true ? <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0 text-ok" /> : done === null ? <Minus className="mt-0.5 h-4 w-4 shrink-0 text-ink-faint" /> : <CircleDashed className="mt-0.5 h-4 w-4 shrink-0 text-ink-faint" />}
      <div>
        <div className={`text-[13px] ${done ? "font-medium" : "text-ink-muted"}`}>{title}</div>
        <div className="text-xs text-ink-faint">{text}</div>
      </div>
    </li>
  );
}

function Note({ tone, children }: { tone: Tone; children: ReactNode }) {
  const c = tone === "warn" ? "border-warn/30 text-warn" : tone === "bad" ? "border-bad/30 text-bad" : "border-line text-ink-muted";
  return (
    <div className={`flex items-start gap-2 rounded-md border px-3 py-2 text-xs ${c}`}>
      <TriangleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" />
      <div>{children}</div>
    </div>
  );
}

/** Name of the running strategy when it is the selected one, else its id. */
const runningName = (d: CardDetails) => (d.runningStrategy && d.strategy?.id === d.runningStrategy ? d.strategy.name : d.runningStrategy);

function Overview({ d, pack, onFile }: { d: CardDetails; pack: CatalogPack | null; onFile: (n: string) => void }) {
  const tr = useTr();
  const { lang, overview } = useApp();
  // A real check exists only for Discord (diagnostics HTTPS checks).
  const confirmed = useMemo(() => {
    if (d.summary.id !== "discord") return null;
    const running = overview?.engine?.state === "running";
    const a = access(overview?.lastDiagnostics, overview?.engine?.strategyId ?? overview?.settings.activeStrategyId ?? "", running, tr);
    return a.level === "ok";
  }, [d.summary.id, overview, tr]);
  const steps = ladder(d, confirmed);
  const rules = d.rules.map((r) => ({ r, info: d.strategy?.rules.find((x) => x.index === r.index) }));
  const text: Record<string, [string, string]> = {
    known: [tr("Адреса известны", "Addresses known"), tr(`${d.summary.inFlowseal} из ${d.summary.domains} доменов сервиса найдены в списках Flowseal ${d.bundleVersion ?? ""}`, `${d.summary.inFlowseal} of ${d.summary.domains} service domains found in the Flowseal ${d.bundleVersion ?? ""} lists`)],
    prepared: [tr("Правило подготовлено", "Rule prepared"), d.strategy ? tr(`Выбранная стратегия «${d.strategy.name}»: правил для сервиса — ${d.rules.length}`, `Selected strategy “${d.strategy.name}”: ${d.rules.length} rules for the service`) : tr("Стратегия не выбрана", "No strategy selected")],
    applied: [tr("Правило применяется", "Rule applied"), d.runningStrategy ? (d.pendingApply ? tr("Обход работает, но с прежней конфигурацией — изменения не применены", "The bypass runs with an older configuration — changes are not applied") : tr(`Работает «${runningName(d)}»`, `Running “${runningName(d)}”`)) : tr("Обход выключен", "The bypass is off")],
    confirmed: [tr("Доступность подтверждена", "Reachability confirmed"), confirmed === null ? tr("Для этого сервиса реальной проверки нет — подтверждённого статуса не бывает", "There is no real check for this service — it is never shown as confirmed") : confirmed ? tr("Последняя диагностика: все проверки HTTPS Discord прошли при этой конфигурации", "Last diagnostics: all Discord HTTPS checks passed with this configuration") : tr("Нет свежей успешной диагностики при этой конфигурации", "No recent successful diagnostics with this configuration")],
  };
  return (
    <div className="space-y-5">
      {pack && (
        <Section title={tr("Назначение", "Purpose")}>
          <p className="text-[13px] text-ink-muted">{loc(pack.description, lang)}</p>
          {pack.notes && <p className="text-xs text-ink-faint">{loc(pack.notes, lang)}</p>}
        </Section>
      )}
      <Section title={tr("Состояние", "State")}>
        <ul className="space-y-2">
          {steps.map((s) => (
            <Step key={s.key} done={s.done} title={text[s.key][0]} text={text[s.key][1]} />
          ))}
        </ul>
      </Section>
      <Section title={tr("Что обрабатывает выбранная стратегия", "What the selected strategy handles")}>
        {rules.length === 0 ? (
          <p className="text-xs text-ink-muted">{tr("Правил для адресов этого сервиса нет.", "No rules for this service's addresses.")}</p>
        ) : (
          <ul className="space-y-1.5 text-xs">
            {rules.map(({ r, info }) => (
              <li key={r.index} className="flex flex-wrap items-center gap-2">
                <Badge>#{r.index}</Badge>
                {info && ruleProtocols(info).map((p) => <Badge key={p} tone="accent">{p}</Badge>)}
                <span>{info ? rulePorts(info, tr) : ""}</span>
                <span className="text-ink-faint">— {matchText(r.matched, tr)}</span>
                {r.matched.by === "protocol" ? (
                  <span className="text-ink-faint">{tr("· по протоколу, не по адресам", "· by protocol, not by address")}</span>
                ) : r.covers > 0 ? (
                  <span className="text-ink-faint">{tr(`· адресов сервиса: ${r.covers}`, `· service addresses: ${r.covers}`)}</span>
                ) : (
                  <span className="text-warn">{tr("· ни одного адреса сервиса (исключены)", "· no service address (excluded)")}</span>
                )}
              </li>
            ))}
          </ul>
        )}
      </Section>
      <div className="space-y-2">
        {d.pendingApply && <Note tone="warn">{tr("Изменения сохранены, но работающий обход их ещё не использует — перезапустите обход.", "Changes are saved but the running bypass does not use them yet — restart the bypass.")}</Note>}
        {d.summary.excluded > 0 && <Note tone="warn">{tr(`${d.summary.excluded} домен(ов) сервиса исключены списком исключений — к ним обход не применяется.`, `${d.summary.excluded} service domain(s) are excluded — the bypass is not applied to them.`)}</Note>}
        {d.sources.some((f) => f.kind === "hostlist" && !f.present && f.name === pack?.flowsealList) && <Note tone="bad">{tr(`Файл ${pack?.flowsealList} в активной версии Flowseal отсутствует или не прошёл проверку.`, `${pack?.flowsealList} is missing from the active Flowseal version or failed verification.`)}</Note>}
        <Note tone="neutral">
          {tr("Списки Flowseal общие: правило одно на весь файл. Домены сервиса — часть файла, а не отдельный набор; повторно добавлять их в свои списки не нужно.", "Flowseal lists are shared: one rule per file. The service's domains are part of the file, not a separate set; there is no need to add them to your lists again.")}{" "}
          {pack?.flowsealList && (
            <button className="text-accent underline" onClick={() => onFile(pack.flowsealList!)}>
              {tr(`Открыть ${pack.flowsealList}`, `Open ${pack.flowsealList}`)}
            </button>
          )}
        </Note>
      </div>
    </div>
  );
}

function Domains({ d, names }: { d: CardDetails; names: (id: string) => string }) {
  const tr = useTr();
  const { toast } = useApp();
  const [q, setQ] = useState("");
  const groups = groupDomains(d.domains, q);
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <label className="relative min-w-[200px] flex-1">
          <Search className="pointer-events-none absolute left-2.5 top-2 h-4 w-4 text-ink-faint" />
          <input className="field h-8 py-1 pl-8" value={q} onChange={(e) => setQ(e.target.value)} placeholder={tr("Поиск домена", "Search domain")} aria-label={tr("Поиск домена", "Search domain")} />
        </label>
        <Button size="sm" icon={<Copy className="h-3.5 w-3.5" />} onClick={() => copy(d.domains.map((x) => x.domain).join("\n"), tr("Домены сервиса скопированы", "Service domains copied"), toast)}>
          {tr("Копировать домены", "Copy domains")}
        </Button>
      </div>
      {groups.map((g) => (
        <div key={g.group} className="rounded-md border border-line">
          <div className="border-b border-line bg-surface-2 px-3 py-1.5 text-xs font-medium">
            {g.group} <span className="text-ink-faint">· {g.items.length}</span>
          </div>
          <ul className="divide-y divide-line">
            {g.items.map((x) => (
              <DomainRow key={x.domain} x={x} names={names} />
            ))}
          </ul>
        </div>
      ))}
      {groups.length === 0 && <p className="text-xs text-ink-muted">{tr("Ничего не найдено", "Nothing found")}</p>}
    </div>
  );
}

function DomainRow({ x, names }: { x: DomainInfo; names: (id: string) => string }) {
  const tr = useTr();
  const { toast } = useApp();
  return (
    <li className="flex flex-wrap items-start gap-x-3 gap-y-1 px-3 py-2 text-xs">
      <span className="selectable min-w-[180px] font-mono text-[12px] text-ink">{x.domain}</span>
      <span className="flex min-w-0 flex-1 flex-wrap gap-1.5">
        {x.sources.length === 0 ? <Badge tone="warn">{tr("нет в списках Flowseal этой версии", "not in this version's Flowseal lists")}</Badge> : x.sources.map((s) => <Badge key={`${s.list}:${s.line}`}>{sourceText(s, tr)}</Badge>)}
        {x.excludedBy.map((s) => (
          <Badge key={`x${s.list}:${s.line}`} tone="bad">
            {tr("исключён", "excluded")}: {sourceText(s, tr)}
          </Badge>
        ))}
        {x.sharedWith.length > 0 && <Badge tone="accent">{tr(`также: ${x.sharedWith.map(names).join(", ")} — правило общее, повторно не добавляется`, `also: ${x.sharedWith.map(names).join(", ")} — one shared rule, not added twice`)}</Badge>}
      </span>
      <button aria-label={tr(`Копировать ${x.domain}`, `Copy ${x.domain}`)} className="text-ink-faint hover:text-ink" onClick={() => copy(x.domain, tr("Скопировано", "Copied"), toast)}>
        <Copy className="h-3.5 w-3.5" />
      </button>
    </li>
  );
}

function Ips({ d, onFile }: { d: CardDetails; onFile: (n: string) => void }) {
  const tr = useTr();
  const { lang } = useApp();
  const ip = d.ipset;
  const backup = d.sources.find((f) => f.name === "ipset-all.txt.backup");
  return (
    <div className="space-y-5">
      <Section title={tr("IP самого сервиса", "The service's own IPs")}>
        {d.serviceIps.length === 0 ? (
          <p className="text-xs text-ink-muted">{tr("В каталоге нет подтверждённых IP-адресов этого сервиса. Его трафик определяется по доменам и протоколам.", "The catalog has no confirmed IP addresses for this service. Its traffic is identified by domains and protocols.")}</p>
        ) : (
          <pre className="selectable max-h-40 overflow-auto rounded-md border border-line bg-surface-2 p-2 font-mono text-[12px]">{d.serviceIps.join("\n")}</pre>
        )}
      </Section>
      <Section title={tr("Статические записи hosts", "Static hosts entries")}>
        <p className="text-xs text-ink-muted">{tr("Файл .service/hosts в установленную версию Flowseal не входит: Discord Bridge не меняет hosts Windows.", "The .service/hosts file is not part of the installed Flowseal version: Discord Bridge does not change the Windows hosts file.")}</p>
      </Section>
      <Section title={tr("Общий IPSet Flowseal", "Shared Flowseal IPSet")}>
        <p className="text-xs text-ink-muted">{tr("Это общий список сетей, а не адреса конкретного сервиса. Правила с ним действуют на любые соединения с этими сетями на своих портах.", "This is a shared list of networks, not the addresses of one service. Rules with it apply to any connection to these networks on their ports.")}</p>
        <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1 text-xs">
          <dt className="text-ink-muted">{tr("Режим", "Mode")}</dt>
          <dd>{lang === "en" ? MODE_EN[ip.mode] : MODE_RU[ip.mode]}</dd>
          <dt className="text-ink-muted">{tr("ipset-all.txt при запуске", "ipset-all.txt at start")}</dt>
          <dd>{ip.runtimeAll ? tr("пустой — совпадает любой адрес", "empty — any address matches") : ip.runtimePlaceholder ? tr("заглушка 203.0.113.113/32 — ни один реальный адрес не совпадает", "placeholder 203.0.113.113/32 — no real address matches") : ip.runtimeEntries != null ? tr(`${ip.runtimeEntries} сетей`, `${ip.runtimeEntries} networks`) : tr("не удалось построить", "could not be built")}</dd>
          <dt className="text-ink-muted">{tr("Полный список Flowseal", "Full Flowseal list")}</dt>
          <dd>{backup?.present ? tr(`${backup.entries} сетей (ipset-all.txt.backup)`, `${backup.entries} networks (ipset-all.txt.backup)`) : tr("файл отсутствует", "file missing")}</dd>
          <dt className="text-ink-muted">{tr("Правила стратегии с IPSet", "Strategy rules with the IPSet")}</dt>
          <dd>{ip.rules.length ? ip.rules.map((i) => `#${i}`).join(", ") : tr("нет", "none")}</dd>
        </dl>
        <div className="flex flex-wrap gap-2">
          {backup?.present && (
            <Button size="sm" icon={<FileText className="h-3.5 w-3.5" />} onClick={() => onFile("ipset-all.txt.backup")}>
              ipset-all.txt.backup
            </Button>
          )}
          <Button size="sm" icon={<FileText className="h-3.5 w-3.5" />} onClick={() => onFile("ipset-exclude.txt")}>
            ipset-exclude.txt
          </Button>
        </div>
      </Section>
    </div>
  );
}

const KIND_RU: Record<SourceFile["kind"], string> = { hostlist: "домены для обхода", hostlistExclude: "исключения доменов", ipset: "сети для обхода", ipsetExclude: "исключения сетей", ipsetSource: "полный список сетей", other: "другое" };
const KIND_EN: Record<SourceFile["kind"], string> = { hostlist: "domains to bypass", hostlistExclude: "domain exclusions", ipset: "networks to bypass", ipsetExclude: "network exclusions", ipsetSource: "full network list", other: "other" };

function Tech({ d, onFile }: { d: CardDetails; onFile: (n: string) => void }) {
  const tr = useTr();
  const { lang } = useApp();
  const mine = new Set(d.rules.map((r) => r.index));
  return (
    <div className="space-y-5">
      <Section title={tr("Оригинальные файлы Flowseal", "Original Flowseal files")}>
        <p className="text-xs text-ink-muted">{tr(`Активная версия Flowseal: ${d.bundleVersion ?? "неизвестна"}. Числа посчитаны по самим файлам.`, `Active Flowseal version: ${d.bundleVersion ?? "unknown"}. Counts come from the files themselves.`)}</p>
        <table className="w-full text-xs">
          <thead className="text-left text-ink-faint">
            <tr>
              <th className="py-1 font-normal">{tr("Файл", "File")}</th>
              <th className="py-1 font-normal">{tr("Назначение", "Role")}</th>
              <th className="py-1 text-right font-normal">{tr("Строк", "Lines")}</th>
              <th className="py-1" />
            </tr>
          </thead>
          <tbody className="divide-y divide-line">
            {d.sources.map((f) => (
              <tr key={f.name}>
                <td className="py-1.5 font-mono">{f.name}</td>
                <td className="py-1.5 text-ink-muted">{lang === "en" ? KIND_EN[f.kind] : KIND_RU[f.kind]}</td>
                <td className="py-1.5 text-right">{f.present ? f.lines : <span className="text-warn">{tr("нет файла", "missing")}</span>}</td>
                <td className="py-1.5 text-right">
                  {f.present && (
                    <Button size="sm" variant="ghost" onClick={() => onFile(f.name)}>
                      {tr("Открыть", "Open")}
                    </Button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Section>
      <Section title={tr("Выбранная стратегия", "Selected strategy")}>
        {!d.strategy ? (
          <p className="text-xs text-ink-muted">{tr("Стратегия не выбрана.", "No strategy selected.")}</p>
        ) : (
          <>
            <p className="text-xs text-ink-muted">
              «{d.strategy.name}» · {tr("перехват", "capture")}: {[d.strategy.wfTcp ? `TCP ${d.strategy.wfTcp}` : null, d.strategy.wfUdp ? `UDP ${d.strategy.wfUdp}` : null].filter(Boolean).join(" · ") || "—"}
            </p>
            <div className="overflow-auto">
              <table className="w-full text-xs">
                <thead className="text-left text-ink-faint">
                  <tr>
                    <th className="py-1 font-normal">#</th>
                    <th className="py-1 font-normal">{tr("Порты", "Ports")}</th>
                    <th className="py-1 font-normal">{tr("Цели", "Targets")}</th>
                    <th className="py-1 font-normal">{tr("Исключения", "Exclusions")}</th>
                    <th className="py-1 font-normal">{tr("Метод", "Method")}</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-line">
                  {d.strategy.rules.map((r) => (
                    <tr key={r.index} className={mine.has(r.index) ? "bg-accent-soft" : r.enabled ? "" : "opacity-50"}>
                      <td className="py-1.5 pr-2 align-top">{r.index}</td>
                      <td className="py-1.5 pr-2 align-top">{rulePorts(r, tr)}</td>
                      <td className="py-1.5 pr-2 align-top font-mono">{[...r.hostlists, ...r.hostlistDomains.map((x) => `domains: ${x}`), ...r.ipsets.map((x) => `ipset: ${x}`), ...r.l7.map((x) => `l7: ${x}`)].join(", ") || "—"}</td>
                      <td className="py-1.5 pr-2 align-top font-mono text-ink-muted">{[...r.hostlistExcludes, ...r.ipsetExcludes].join(", ") || "—"}</td>
                      <td className="py-1.5 align-top">{r.desync ?? "—"}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <p className="text-[11px] text-ink-faint">{tr("Подсвечены правила этого сервиса. Исключения не являются целями обхода: адреса из них обход не трогает.", "Rules of this service are highlighted. Exclusions are not bypass targets: the bypass leaves their addresses alone.")}</p>
          </>
        )}
      </Section>
      <Section title={tr("Состояние конфигурации", "Configuration state")}>
        <p className="text-xs text-ink-muted">
          {d.runningStrategy ? tr(`Работает «${runningName(d)}».`, `Running “${runningName(d)}”.`) : tr("Обход выключен.", "The bypass is off.")} {d.pendingApply ? tr("Сохранённые изменения ещё не применены.", "Saved changes are not applied yet.") : ""}
        </p>
      </Section>
    </div>
  );
}

export function ServiceDetails({ id, pack, names, onClose }: { id: string | null; pack: CatalogPack | null; names: (id: string) => string; onClose: () => void }) {
  const tr = useTr();
  const { lang } = useApp();
  const [tab, setTab] = useState<DetailsTab>("overview");
  const [d, setD] = useState<CardDetails | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [file, setFile] = useState<string | null>(null);
  useEffect(() => {
    setD(null);
    setError(null);
    setTab("overview");
    if (!id) return;
    api.serviceDetails(id).then(setD, (e) => setError(errorText(e)));
  }, [id]);
  const title = pack ? loc(pack.name, lang) : id ?? "";
  return (
    <>
      <Modal open={id !== null && file === null} onClose={onClose} wide title={title}>
        <div className="space-y-4">
          <Segmented<DetailsTab>
            ariaLabel={tr("Разделы карточки", "Card sections")}
            value={tab}
            onChange={setTab}
            options={[
              { value: "overview", label: tr("Обзор", "Overview") },
              { value: "domains", label: d ? tr(`Домены · ${d.domains.length}`, `Domains · ${d.domains.length}`) : tr("Домены", "Domains") },
              { value: "ips", label: tr("IP-адреса", "IP addresses") },
              { value: "tech", label: tr("Технические подробности", "Technical details"), icon: <Layers className="h-3.5 w-3.5" /> },
            ]}
          />
          {error && <div className="text-sm text-bad">{error}</div>}
          {!d && !error && <div className="text-sm text-ink-muted">{tr("Загрузка…", "Loading…")}</div>}
          {d && tab === "overview" && <Overview d={d} pack={pack} onFile={setFile} />}
          {d && tab === "domains" && <Domains d={d} names={names} />}
          {d && tab === "ips" && <Ips d={d} onFile={setFile} />}
          {d && tab === "tech" && <Tech d={d} onFile={setFile} />}
        </div>
      </Modal>
      <ListViewer name={file} onClose={() => setFile(null)} />
    </>
  );
}
