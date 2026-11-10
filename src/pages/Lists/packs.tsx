// Pieces shared by the tabs of the Apps page: pack card, details, update dialog, status notes.
import { ArrowUpCircle, CheckCircle2, CircleDashed, History, Info, Minus, Plus, RefreshCw, RotateCw, TriangleAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { review } from "../../catalog/confirm";
import { CATEGORY_EN, CATEGORY_RU, loc, packIpState, PackIpState, stateLabel } from "../../catalog/apps";
import { Button, Card, Modal } from "../../components/ui";
import { confirmDialog } from "../../components/ui/confirm";
import { useActivate } from "../../hooks/useActivate";
import { api, errorText } from "../../services/api";
import type { CatalogPack, CatalogView, IpSetStatus, IpsetMode, PackConfirm, PackDiff, PackState, PackView } from "../../services/types";
import { useApp, useCount, useTr } from "../../stores/app";

/** Real service icon from the verified catalog, or a neutral initial. */
export function AppIcon({ name, icon, size = "h-10 w-10 text-base" }: { name: string; icon?: string | null; size?: string }) {
  const icons = useApp((s) => s.catalog?.icons);
  const src = icon ? icons?.[icon] : undefined;
  if (src) return <img src={src} alt="" className={`${size} shrink-0 rounded-lg`} draggable={false} />;
  return <span className={`flex shrink-0 items-center justify-center rounded-lg bg-surface-3 font-semibold text-ink-muted ${size}`}>{name.slice(0, 1)}</span>;
}

export function StateChip({ state }: { state: PackState | "orphan" }) {
  const { lang } = useApp();
  const tr = useTr();
  if (state === "orphan")
    return (
      <span className="inline-flex items-center gap-1 text-xs text-warn">
        <TriangleAlert className="h-3.5 w-3.5" />
        {tr("нет в каталоге", "not in the catalog")}
      </span>
    );
  const cls: Record<PackState, string> = { flowseal: "text-ink-muted", available: "text-ink-faint", current: "text-ok", updateAvailable: "text-accent", differs: "text-warn" };
  return (
    <span className={`inline-flex items-center gap-1 text-xs ${cls[state]}`}>
      {state === "current" ? <CheckCircle2 className="h-3.5 w-3.5" /> : state === "updateAvailable" ? <ArrowUpCircle className="h-3.5 w-3.5" /> : state === "differs" ? <RefreshCw className="h-3.5 w-3.5" /> : null}
      {stateLabel(state, lang)}
    </span>
  );
}

export function CoverageNote({ pack }: { pack: CatalogPack }) {
  const { lang } = useApp();
  const tr = useTr();
  if (!pack.partialCoverage && !pack.notes) return null;
  return (
    <p className={`mt-2 flex gap-1.5 rounded-md px-2 py-1.5 text-[11px] leading-snug ${pack.partialCoverage ? "bg-warn-soft text-warn" : "bg-surface-2 text-ink-muted"}`}>
      <Info className="mt-px h-3.5 w-3.5 shrink-0" />
      <span>{loc(pack.notes, lang) || tr("Домены помогают частично: может понадобиться IP-правило и подходящая стратегия.", "Domains help only partly: an IP rule and a suitable strategy may be needed.")}</span>
    </p>
  );
}

/** Installed version: catalog versions are numbers; alpha.8 stored a date. */
export const versionText = (v: string | null | undefined, tr: (a: string, b: string) => string) => (!v ? "—" : /^\d+$/.test(v) ? v : tr(`до каталога (${v})`, `pre-catalog (${v})`));

export const counts = (count: ReturnType<typeof useCount>, d: number, i: number) =>
  [d ? count(d, ["домен", "домена", "доменов"], ["domain", "domains"]) : "", i ? count(i, ["IP-подсеть", "IP-подсети", "IP-подсетей"], ["IP subnet", "IP subnets"]) : ""].filter(Boolean).join(" · ");

export function ApplyBanner() {
  const { overview } = useApp();
  const tr = useTr();
  const activate = useActivate();
  if (overview?.ipSets?.error)
    return (
      <div className="flex items-start gap-3 rounded-lg border border-bad/30 bg-bad-soft px-4 py-2.5 text-[13px] text-bad">
        <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0" />
        <span className="selectable">{tr("Список IP нельзя собрать, поэтому обход не запустится: ", "The IP list cannot be built, so the bypass will not start: ")}{overview.ipSets.error}</span>
      </div>
    );
  if (!overview?.pendingApply) return null;
  return (
    <div className="flex items-center gap-3 rounded-lg border border-warn/30 bg-warn-soft px-4 py-2.5 text-[13px] text-warn">
      <TriangleAlert className="h-4 w-4 shrink-0" />
      <span className="flex-1">{tr("Изменения сохранены, но ещё не применены: обход работает со старыми списками.", "Changes are saved but not applied: the bypass still uses the old lists.")}</span>
      <Button size="sm" variant="secondary" icon={<RotateCw className="h-3.5 w-3.5" />} onClick={() => activate(overview.engine?.strategyId ?? null)}>
        {tr("Перезапустить обход", "Restart bypass")}
      </Button>
    </div>
  );
}

export const MODE_RU: Record<IpsetMode, string> = { none: "Без IP", selected: "Выбранные IP", flowseal: "Полный список Flowseal", flowsealPlusSelected: "Flowseal + выбранные", any: "Все адреса" };
export const MODE_EN: Record<IpsetMode, string> = { none: "No IPs", selected: "Selected IPs", flowseal: "Full Flowseal list", flowsealPlusSelected: "Flowseal + selected", any: "All addresses" };

/** One honest line about where IPs stand; "applied" only when the backend confirms it. */
export function IpStateNote({ state, ip, subject }: { state: PackIpState | null; ip: IpSetStatus | undefined; subject: "pack" | "list" }) {
  const tr = useTr();
  const { lang, setPage, strategies } = useApp();
  const activate = useActivate();
  if (!state || !ip) return null;
  const modeName = lang === "en" ? MODE_EN[ip.mode] : MODE_RU[ip.mode];
  const strategyName = strategies.find((s) => s.id === ip.strategyId)?.name ?? ip.strategyId ?? "—";
  const what = subject === "pack" ? tr("IP этого набора", "This set's IPs") : tr("Эти IP", "These IPs");
  const toSettings = (
    <button className="text-left underline underline-offset-2 hover:text-ink" onClick={() => setPage("settings")}>
      {tr("Настройки → Обход", "Settings → Bypass")}
    </button>
  );
  const toStrategies = (
    <button className="text-left underline underline-offset-2 hover:text-ink" onClick={() => setPage("strategies")}>
      {tr("выбрать другую стратегию", "choose another strategy")}
    </button>
  );
  const tone = { warn: "border-warn/30 text-warn", info: "border-line text-ink-muted", ok: "border-ok/30 text-ok" };
  const box = (kind: keyof typeof tone, body: React.ReactNode) => (
    <p className={`mt-2 flex gap-1.5 rounded-md border px-2 py-1.5 text-[11px] leading-snug ${tone[kind]}`}>
      {kind === "ok" ? <CheckCircle2 className="mt-px h-3.5 w-3.5 shrink-0" /> : kind === "warn" ? <TriangleAlert className="mt-px h-3.5 w-3.5 shrink-0" /> : <Info className="mt-px h-3.5 w-3.5 shrink-0" />}
      <span>{body}</span>
    </p>
  );
  switch (state) {
    case "notInstalledIgnored":
      return box("info", <>{tr(`${what} будут использоваться только в режимах «Выбранные IP» и «Flowseal + выбранные» (сейчас «${modeName}»). `, `${what} are used only in the “Selected IPs” and “Flowseal + selected” modes (now “${modeName}”). `)}{toSettings}</>);
    case "notInstalled":
      return null;
    case "modeIgnores":
      return box("warn", <>{tr(`${what} не используются: режим «${modeName}». Нужен «Выбранные IP» или «Flowseal + выбранные» — `, `${what} are not used: “${modeName}” mode. Needs “Selected IPs” or “Flowseal + selected” — `)}{toSettings}</>);
    case "allExcluded":
      return box("warn", tr(`${what} целиком попадают в ваши исключения «IP — не трогать».`, `${what} are entirely covered by your “IPs — leave alone” exclusions.`));
    case "strategyNoIpset":
      return box("warn", <>{tr(`Стратегия «${strategyName}» не использует IP-фильтры (нет правил с ipset-all.txt), поэтому IP на обход не влияют. Можно `, `Strategy “${strategyName}” has no IP filters (no rules with ipset-all.txt), so IPs do not affect the bypass. You can `)}{toStrategies}.</>);
    case "allAddresses":
      return box("info", tr(`Режим «${modeName}»: стратегия обрабатывает все адреса, отдельные IP не нужны.`, `“${modeName}” mode: the strategy processes every address, separate IPs are not needed.`));
    case "notApplied":
      return box(
        "info",
        ip.engineRunsStrategy ? (
          <>
            {tr("IP включены в подготовленную конфигурацию, но работающий обход их ещё не использует. ", "IPs are in the prepared configuration, but the running bypass does not use them yet. ")}
            <button className="text-left underline underline-offset-2 hover:text-ink" onClick={() => activate(ip.strategyId ?? null)}>
              {tr("Перезапустить обход", "Restart the bypass")}
            </button>
          </>
        ) : (
          tr("IP включены в подготовленную конфигурацию. Обход сейчас не запущен с этой стратегией — они начнут действовать после включения.", "IPs are in the prepared configuration. The bypass is not running this strategy now — they take effect once it is on.")
        ),
      );
    case "applied":
      return box("ok", tr(`${what} используются работающим обходом (правил с IP-фильтром в стратегии: ${ip.strategy.ipsetRules}).`, `${what} are used by the running bypass (strategy rules with an IP filter: ${ip.strategy.ipsetRules}).`));
  }
}

/** Shared actions for catalog packs (all go through Rust). */
export function usePackActions() {
  const { setCatalog, refresh, toast, lang, loadCatalog } = useApp();
  const tr = useTr();
  const [busy, setBusy] = useState(false);
  const run = async (fn: () => Promise<CatalogView>, ok: string) => {
    setBusy(true);
    try {
      setCatalog(await fn());
      await refresh();
      toast("ok", ok);
      return true;
    } catch (e) {
      toast("error", errorText(e));
      // Show what is current now (e.g. a stale confirmation was rejected).
      await loadCatalog();
      return false;
    } finally {
      setBusy(false);
    }
  };
  const name = (p: CatalogPack) => loc(p.name, lang);
  return {
    busy,
    install: (p: CatalogPack, c: PackConfirm) => run(() => api.packInstall(c), tr(`Набор «${name(p)}» установлен. Изменения начнут действовать после перезапуска обхода.`, `${name(p)} installed. Changes take effect after the bypass restarts.`)),
    update: (p: CatalogPack, c: PackConfirm) => run(() => api.packInstall(c), tr(`Набор «${name(p)}» обновлён до версии ${p.version}. Предыдущая версия сохранена для отката.`, `${name(p)} updated to version ${p.version}. The previous version is kept for rollback.`)),
    remove: async (id: string, title: string) => {
      const ok = await confirmDialog({
        title: tr(`Удалить набор «${title}»?`, `Remove “${title}”?`),
        body: tr("Удаляется только этот набор. Ваши ручные адреса и адреса других наборов остаются.", "Only this set is removed. Your own entries and other sets' addresses stay."),
        confirmLabel: tr("Удалить", "Remove"),
      });
      return ok && run(() => api.packRemove(id), tr(`Набор «${title}» удалён`, `${title} removed`));
    },
    rollback: async (id: string, title: string, to?: string | null) => {
      const ok = await confirmDialog({
        title: tr(`Откатить «${title}»?`, `Roll back “${title}”?`),
        body: tr(`Вернётся предыдущая проверенная версия${to ? ` (${to})` : ""}. Текущая станет точкой для повторного отката.`, `The previous verified version${to ? ` (${to})` : ""} is restored. The current one becomes the next rollback point.`),
        confirmLabel: tr("Откатить", "Roll back"),
      });
      return ok && run(() => api.packRollback(id), tr(`Набор «${title}» откатан`, `${title} rolled back`));
    },
  };
}

export function DiffList({ title, items, tone }: { title: string; items: string[]; tone: "ok" | "bad" }) {
  if (!items.length) return null;
  return (
    <div>
      <div className="label mb-1">{title}</div>
      <div className={`code-box selectable max-h-40 overflow-auto px-3 py-2 ${tone === "ok" ? "text-ok" : "text-bad"}`}>
        {items.map((x) => (
          <div key={x}>
            {tone === "ok" ? "+ " : "− "}
            {x}
          </div>
        ))}
      </div>
    </div>
  );
}

/** The view as it was when a window opened, plus the live one. The window
 * keeps showing the frozen snapshot; a newer catalog never replaces it
 * silently — the user reloads the review explicitly. */
export function useFrozen(live: PackView | null) {
  const [shown, setShown] = useState<PackView | null>(live);
  const id = live?.pack.id ?? null;
  useEffect(() => {
    setShown(live);
    // Freeze again only when another pack is opened (or the window closes).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);
  return { shown: shown && shown.pack.id === id ? shown : live, reload: () => setShown(live) };
}

export function StaleNote({ onReload }: { onReload: () => void }) {
  const tr = useTr();
  return (
    <div className="flex items-start gap-2 rounded-lg border border-warn/30 bg-warn/10 px-3 py-2 text-[13px]">
      <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0 text-warn" />
      <div className="min-w-0 flex-1">
        <p>{tr("Каталог или установленная копия изменились, пока это окно было открыто. Показанные данные устарели — подтвердить их нельзя.", "The catalog or the installed copy changed while this window was open. The shown data is outdated and cannot be confirmed.")}</p>
        <Button size="sm" variant="ghost" className="mt-1" icon={<RefreshCw className="h-3.5 w-3.5" />} onClick={onReload}>
          {tr("Показать актуальные данные", "Show the current data")}
        </Button>
      </div>
    </div>
  );
}

/** Exact address changes, confirmed before the update is saved. */
export function UpdateDialog({ view, onClose }: { view: PackView | null; onClose: () => void }) {
  const tr = useTr();
  const { lang } = useApp();
  const actions = usePackActions();
  const { shown, reload } = useFrozen(view);
  const [fetched, setFetched] = useState<PackDiff | null>(null);
  useEffect(() => {
    setFetched(null);
    // The view already carries the diff of the same snapshot; ask Rust only
    // if it does not. An answer for another snapshot is ignored by review().
    if (shown && !shown.diff) {
      let live = true;
      api
        .packDiff(shown.pack.id)
        .then((d) => live && setFetched(d))
        .catch(() => undefined);
      return () => {
        live = false;
      };
    }
  }, [shown]);
  if (!shown || !view) return null;
  const r = review(shown, view, fetched, true);
  const p = shown.pack;
  const diff = r.diff;
  const empty = diff && !diff.addedDomains.length && !diff.removedDomains.length && !diff.addedIps.length && !diff.removedIps.length;
  return (
    <Modal
      open
      onClose={onClose}
      title={tr(`Обновление «${loc(p.name, lang)}»`, `Update “${loc(p.name, lang)}”`)}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {tr("Отмена", "Cancel")}
          </Button>
          <Button variant="primary" loading={actions.busy} disabled={!r.confirm} onClick={async () => r.confirm && (await actions.update(p, r.confirm)) && onClose()}>
            {tr(`Обновить до версии ${p.version}`, `Update to version ${p.version}`)}
          </Button>
        </>
      }
    >
      <div className="space-y-3 text-sm">
        {r.stale && <StaleNote onReload={reload} />}
        <p className="text-ink-muted">
          {tr(`Версия ${versionText(diff?.fromVersion, tr)} → ${p.version} (${p.updated}). Перезапуска обхода не будет — вы примените изменения сами.`, `Version ${versionText(diff?.fromVersion, tr)} → ${p.version} (${p.updated}). The bypass is not restarted — you apply the changes yourself.`)}
        </p>
        {!diff && !r.stale && <p className="text-ink-faint">{tr("Загружаю изменения…", "Loading changes…")}</p>}
        {empty && <p className="text-ink-muted">{tr("Адреса не меняются, обновляются только описание и версия.", "Addresses do not change; only the description and version are updated.")}</p>}
        {diff && (
          <>
            <DiffList title={tr("Добавятся домены", "Domains added")} items={diff.addedDomains} tone="ok" />
            <DiffList title={tr("Уберутся домены", "Domains removed")} items={diff.removedDomains} tone="bad" />
            <DiffList title={tr("Добавятся IP", "IPs added")} items={diff.addedIps} tone="ok" />
            <DiffList title={tr("Уберутся IP", "IPs removed")} items={diff.removedIps} tone="bad" />
          </>
        )}
        <p className="text-xs text-ink-faint">{tr("Текущая версия сохранится — к ней можно будет откатиться.", "The current version is kept — you can roll back to it.")}</p>
      </div>
    </Modal>
  );
}

/** Separate, honest facts about one pack (never a single "works"). */
export function StatusLadder({ view }: { view: PackView }) {
  const tr = useTr();
  const { overview } = useApp();
  const ip = overview?.ipSets;
  const installed = view.installed;
  const rows: { ok: boolean | null; text: string }[] = [
    { ok: true, text: tr(`В каталоге: версия ${view.pack.version} от ${view.pack.updated}`, `In the catalog: version ${view.pack.version} of ${view.pack.updated}`) },
    view.state === "flowseal"
      ? { ok: null, text: tr(`Входит во встроенный список Flowseal (${view.pack.flowsealList}); отдельно не устанавливается`, `Part of the built-in Flowseal list (${view.pack.flowsealList}); not installed separately`) }
      : { ok: !!installed, text: installed ? tr(`Установлен: версия ${versionText(installed.version, tr)}`, `Installed: version ${versionText(installed.version, tr)}`) : tr("Не установлен", "Not installed") },
  ];
  if (view.state === "updateAvailable") rows.push({ ok: false, text: tr("Есть новая версия в каталоге", "A newer version is in the catalog") });
  if (view.state === "differs") rows.push({ ok: false, text: tr("Установленная копия отличается от версии каталога", "The installed copy differs from the catalog version") });
  if (installed) {
    if (overview?.pendingApply) rows.push({ ok: false, text: tr("Изменения сохранены, но ещё не применены к работающему обходу", "Changes are saved but not applied to the running bypass") });
    else if (ip?.engineUpToDate) rows.push({ ok: true, text: tr("Домены набора записаны в работающую конфигурацию обхода", "The set's domains are in the running bypass configuration") });
    else rows.push({ ok: null, text: tr("Обход сейчас не запущен с текущей конфигурацией", "The bypass is not running the current configuration now") });
    if (view.pack.ips.length && ip) {
      const st = packIpState(view.pack, view.state, ip);
      rows.push({ ok: st === "applied", text: st === "applied" ? tr("IP набора используются работающим обходом", "The set's IPs are used by the running bypass") : tr("IP набора сейчас не используются обходом — см. подсказку на карточке", "The set's IPs are not used by the bypass now — see the card note") });
    }
  }
  rows.push({ ok: null, text: tr("Доступность сервиса Bridge не проверял", "Bridge has not checked the service's availability") });
  return (
    <ul className="space-y-1.5">
      {rows.map((r, i) => (
        <li key={i} className="flex gap-2 text-[13px]">
          {r.ok === true ? <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0 text-ok" /> : r.ok === false ? <CircleDashed className="mt-0.5 h-4 w-4 shrink-0 text-warn" /> : <Info className="mt-0.5 h-4 w-4 shrink-0 text-ink-faint" />}
          <span className="text-ink-muted">{r.text}</span>
        </li>
      ))}
    </ul>
  );
}

export function PackDetails({ view, onClose, onUpdate }: { view: PackView | null; onClose: () => void; onUpdate: (v: PackView) => void }) {
  const tr = useTr();
  const count = useCount();
  const { lang } = useApp();
  const actions = usePackActions();
  const { shown, reload } = useFrozen(view);
  if (!view || !shown) return null;
  // Content and the install token come from the snapshot frozen at opening.
  const r = review(shown, view, null, false);
  const p = shown.pack;
  const name = loc(p.name, lang);
  const prov = (kind: string) => p.provenance.filter((x) => x.kind === kind);
  return (
    <Modal
      open
      wide
      onClose={onClose}
      title={name}
      footer={
        <>
          {view.installed?.previous && (
            <Button variant="ghost" icon={<History className="h-3.5 w-3.5" />} loading={actions.busy} onClick={async () => (await actions.rollback(p.id, name, view.installed?.previous?.version)) && onClose()}>
              {tr("Откатить", "Roll back")}
            </Button>
          )}
          {view.installed && (
            <Button variant="ghost" icon={<Minus className="h-3.5 w-3.5" />} loading={actions.busy} onClick={async () => (await actions.remove(p.id, name)) && onClose()}>
              {tr("Удалить", "Remove")}
            </Button>
          )}
          {(view.state === "updateAvailable" || view.state === "differs") && (
            <Button variant="primary" icon={<ArrowUpCircle className="h-3.5 w-3.5" />} onClick={() => onUpdate(view)}>
              {tr("Посмотреть изменения", "Review changes")}
            </Button>
          )}
          {shown.state === "available" && (
            <Button variant="primary" icon={<Plus className="h-3.5 w-3.5" />} loading={actions.busy} disabled={!r.confirm} onClick={async () => r.confirm && (await actions.install(p, r.confirm)) && onClose()}>
              {tr("Установить", "Install")}
            </Button>
          )}
        </>
      }
    >
      <div className="grid grid-cols-[1fr_280px] gap-5 narrow:grid-cols-1">
        <div className="min-w-0 space-y-3">
          {r.stale && <StaleNote onReload={reload} />}
          <div className="flex items-start gap-3">
            <AppIcon name={name} icon={p.icon} size="h-12 w-12 text-lg" />
            <div className="min-w-0">
              <div className="text-xs text-ink-faint">
                {lang === "en" ? CATEGORY_EN[p.category] : CATEGORY_RU[p.category]} · {p.source === "official" ? tr("официальный", "official") : tr("сообщество", "community")} · {tr("версия", "version")} {p.version} · {p.updated}
              </div>
              <p className="mt-1 text-[13px] text-ink-muted">{loc(p.description, lang)}</p>
            </div>
          </div>
          <CoverageNote pack={p} />
          <div>
            <div className="label mb-1">{tr("Домены", "Domains")} · {p.domains.length}</div>
            <div className="code-box selectable max-h-36 overflow-auto whitespace-pre px-3 py-2">{p.domains.length ? p.domains.join("\n") : "—"}</div>
          </div>
          <div>
            <div className="label mb-1">{tr("IP-подсети", "IP subnets")} · {p.ips.length}</div>
            <div className="code-box selectable max-h-36 overflow-auto whitespace-pre px-3 py-2">{p.ips.length ? p.ips.join("\n") : tr("нет", "none")}</div>
          </div>
          <div>
            <div className="label mb-1">{tr("Происхождение данных", "Data provenance")}</div>
            <ul className="space-y-1 text-xs text-ink-muted">
              {["domains", "ips", "icon"].flatMap((k) =>
                prov(k).map((x, i) => (
                  <li key={k + i} className="selectable break-all">
                    <span className="text-ink-faint">{k === "domains" ? tr("домены", "domains") : k === "ips" ? "IP" : tr("значок", "icon")}: </span>
                    {x.url}
                    {x.note && <span className="text-ink-faint"> — {x.note}</span>}
                  </li>
                )),
              )}
            </ul>
          </div>
        </div>
        <div className="space-y-3">
          <div className="label">{tr("Состояние", "State")}</div>
          <StatusLadder view={shown} />
          {shown.installed && <div className="text-xs text-ink-faint">{tr("Установлено", "Installed")}: {counts(count, shown.installed.domains.length, shown.installed.ips?.length ?? 0) || "—"}</div>}
        </div>
      </div>
    </Modal>
  );
}

export function PackCard({ view, onOpen, onUpdate }: { view: PackView; onOpen: () => void; onUpdate: () => void }) {
  const ip = useApp((s) => s.overview?.ipSets);
  const tr = useTr();
  const count = useCount();
  const { lang } = useApp();
  const actions = usePackActions();
  const p = view.pack;
  const name = loc(p.name, lang);
  return (
    <Card className={`flex flex-col p-4 transition-colors hover:border-line-strong ${view.state === "current" ? "border-ok/25" : view.state === "updateAvailable" ? "border-accent/30" : ""}`}>
      <button className="flex items-start gap-3 text-left" onClick={onOpen}>
        <AppIcon name={name} icon={p.icon} />
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-semibold hover:underline">{name}</div>
          <div className="text-xs text-ink-faint">
            {lang === "en" ? CATEGORY_EN[p.category] : CATEGORY_RU[p.category]} · {tr("версия", "version")} {p.version}
          </div>
        </div>
      </button>
      <p className="mt-2.5 text-[13px] leading-relaxed text-ink-muted">{loc(p.description, lang)}</p>
      {p.partialCoverage && <CoverageNote pack={p} />}
      <IpStateNote state={ip ? packIpState(p, view.state, ip) : null} ip={ip} subject="pack" />
      <div className="mt-auto flex items-center justify-between gap-2 pt-3">
        <div className="min-w-0">
          <StateChip state={view.state} />
          <div className="text-[11px] text-ink-faint">{counts(count, p.domains.length, p.ips.length)}</div>
        </div>
        <div className="flex gap-1.5">
          {view.state === "flowseal" && <span className="text-[11px] text-ink-faint">{p.flowsealList}</span>}
          {view.state === "available" && (
            <Button size="sm" variant="primary" icon={<Plus className="h-3.5 w-3.5" />} disabled={actions.busy} onClick={() => actions.install(p, view.confirm)}>
              {tr("Установить", "Install")}
            </Button>
          )}
          {(view.state === "updateAvailable" || view.state === "differs") && (
            <Button size="sm" variant="primary" icon={<ArrowUpCircle className="h-3.5 w-3.5" />} onClick={onUpdate}>
              {tr("Обновить…", "Update…")}
            </Button>
          )}
          {view.state !== "flowseal" && view.state !== "available" && (
            <Button size="sm" variant="ghost" onClick={onOpen}>
              {tr("Подробнее", "Details")}
            </Button>
          )}
        </div>
      </div>
    </Card>
  );
}
