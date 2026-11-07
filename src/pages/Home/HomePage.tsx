import { motion, AnimatePresence, useReducedMotion } from "motion/react";
import {
  Activity,
  AppWindow,
  ArrowRight,
  CheckCircle2,
  ChevronDown,
  Copy,
  CircleHelp,
  Download,
  Headset,
  Layers,
  Loader2,
  Mic,
  Play,
  Power,
  RefreshCw,
  RotateCw,
  Route,
  Send,
  Settings as SettingsIcon,
  Sparkles,
  Square,
  TriangleAlert,
  XCircle,
} from "lucide-react";
import { ReactNode, useState } from "react";
import { Button, Card, Modal, Select, Skeleton, Switch } from "../../components/ui";
import { installedTitle, loc, ownEntries } from "../../catalog/apps";
import { strategyKind } from "../../catalog/strategies";
import { useActivate, useAuto, useLaunch } from "../../hooks/useActivate";
import { api, errorText } from "../../services/api";
import { useApp, useCount, useTr } from "../../stores/app";
import { access, type Level } from "./status";
import type { TelegramDiagItem } from "../../services/types";
import { actions, clientText, portNote, routeText, stateLabel } from "../../telegram/view";

const tone: Record<Level, string> = { ok: "text-ok", warn: "text-warn", bad: "text-bad", unknown: "text-ink-muted", busy: "text-accent" };

function LevelIcon({ level, className = "h-5 w-5" }: { level: Level; className?: string }) {
  const c = `${className} shrink-0`;
  if (level === "ok") return <CheckCircle2 className={`${c} text-ok`} />;
  if (level === "warn") return <TriangleAlert className={`${c} text-warn`} />;
  if (level === "bad") return <XCircle className={`${c} text-bad`} />;
  if (level === "busy") return <Loader2 className={`${c} animate-spin text-accent`} />;
  return <CircleHelp className={`${c} text-ink-faint`} />;
}

function Pill({ level, children }: { level: Level; children: ReactNode }) {
  const cls = {
    ok: "border-ok/30 bg-ok-soft text-ok",
    warn: "border-warn/30 bg-warn-soft text-warn",
    bad: "border-bad/30 bg-bad-soft text-bad",
    busy: "border-accent/30 bg-accent-soft text-accent",
    unknown: "border-line bg-surface-2 text-ink-muted",
  }[level];
  const dot = { ok: "bg-ok", warn: "bg-warn", bad: "bg-bad", busy: "bg-accent", unknown: "bg-ink-faint" }[level];
  return (
    <span className={`inline-flex shrink-0 items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs font-medium ${cls}`}>
      <span className={`h-1.5 w-1.5 rounded-full ${dot}`} />
      {children}
    </span>
  );
}

function ActionRow({ icon, title, hint, children, onClick, disabled }: { icon: ReactNode; title: string; hint?: string; children?: ReactNode; onClick?: () => void; disabled?: boolean }) {
  const body = (
    <>
      <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-surface-3 text-ink-muted">{icon}</span>
      <span className="min-w-0 flex-1 text-left">
        <span className="block text-[13px] font-medium text-ink">{title}</span>
        {hint && <span className="block truncate text-xs text-ink-faint">{hint}</span>}
      </span>
    </>
  );
  if (onClick)
    return (
      <button disabled={disabled} onClick={onClick} className="group flex w-full items-center gap-3 rounded-lg px-2.5 py-2 transition-colors hover:bg-surface-2 disabled:pointer-events-none disabled:opacity-50">
        {body}
        <ArrowRight className="h-4 w-4 shrink-0 text-ink-faint transition-transform group-hover:translate-x-0.5" />
      </button>
    );
  return (
    <div className="flex items-center gap-3 rounded-lg px-2.5 py-2">
      {body}
      {children}
    </div>
  );
}

function DiscordHero() {
  const { overview, strategies, busy, toast, refresh, auto, setPage } = useApp();
  const tr = useTr();
  const activate = useActivate();
  const launchDiscord = useLaunch();
  const autoSel = useAuto();
  const reduce = useReducedMotion();
  const [launching, setLaunching] = useState(false);
  const [checking, setChecking] = useState(false);
  const [more, setMore] = useState(false);
  if (!overview) return null;

  const engine = overview.engine;
  const running = engine?.state === "running";
  const failed = engine?.state === "failed";
  const helperOk = overview.helper.available;
  const autoRunning = !!auto?.running;
  const activeId = (running ? engine?.strategyId : null) ?? overview.settings.activeStrategyId ?? "";
  const d = overview.discord;
  const acc = access(overview.lastDiagnostics, activeId, running, tr);

  const launch = async () => {
    setLaunching(true);
    try {
      await launchDiscord("preference");
    } finally {
      setLaunching(false);
    }
  };
  const toggle = async () => {
    if (!running) return void (await activate(activeId || null));
    if (engine?.canControl === false) return toast("error", tr("Обходом управляет другой пользователь Windows", "Another Windows user controls the bypass"));
    await api.bypassDisable().catch((e) => toast("error", errorText(e)));
    refresh();
  };
  const changeStrategy = async (id: string) => {
    if (running) await activate(id);
    else {
      await api.selectStrategy(id).catch((e) => toast("error", errorText(e)));
      refresh();
    }
  };
  const check = async () => {
    setChecking(true);
    try {
      await api.runDiagnostics();
      await refresh();
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setChecking(false);
    }
  };
  const confirmVoice = async (works: boolean) => {
    if (!activeId) return;
    await api.confirmVoice(activeId, works).catch((e) => toast("error", errorText(e)));
    toast("ok", tr("Отметка сохранена для этой стратегии", "Saved for this strategy"));
    refresh();
  };

  type Hero = { level: Level; pill: string; title: string; text: string };
  const hero: Hero = autoRunning
    ? {
        level: "busy",
        pill: tr("Идёт подбор", "Searching"),
        title: tr("Подбираем способ обхода", "Finding a working method"),
        text: auto?.maxAttempts
          ? tr(`Попытка ${auto.attempt} из ${auto.maxAttempts}. Обычно это занимает пару минут.`, `Attempt ${auto.attempt} of ${auto.maxAttempts}. Usually takes a couple of minutes.`)
          : tr("Обычно это занимает пару минут.", "Usually takes a couple of minutes."),
      }
    : busy || checking
      ? { level: "busy", pill: tr("Проверка", "Checking"), title: tr("Идёт проверка", "Checking"), text: tr("Запускаем обход и проверяем, открывается ли Discord.", "Starting the bypass and checking that Discord opens.") }
      : !helperOk
        ? { level: "bad", pill: tr("Служба не отвечает", "No service"), title: tr("Служба не отвечает", "The service is not responding"), text: tr("Обход работает через системную службу Bridge. Обычно помогает переустановка приложения.", "The bypass runs in the Bridge system service. Reinstalling the app usually helps.") }
        : failed
          ? { level: "bad", pill: tr("Ошибка", "Error"), title: tr("Ошибка подключения", "Connection error"), text: engine?.lastError || tr("Обход остановился. Попробуйте подобрать другой способ.", "The bypass stopped. Try finding another method.") }
          : running && acc.level === "bad"
            ? { level: "bad", pill: tr("Не открывается", "Unreachable"), title: tr("Ошибка подключения", "Connection error"), text: tr("Обход включён, но Discord не открывается — этот способ не подходит вашему провайдеру. Откройте «Ещё» → «Подобрать автоматически».", "The bypass is on, but Discord does not open — this method does not suit your ISP. Open “More” → “Find automatically”.") }
            : running && acc.level === "warn"
              ? { level: "warn", pill: tr("Частично", "Partly"), title: tr("Discord доступен не полностью", "Discord is partly reachable"), text: tr("Часть адресов Discord не отвечает: возможны проблемы с загрузкой картинок или обновлений.", "Some Discord addresses fail: images or updates may not load.") }
              : running
                ? { level: "ok", pill: tr("Доступен", "Reachable"), title: tr("Discord доступен", "Discord is reachable"), text: tr("Обход включён. Запускайте Discord как обычно.", "The bypass is on. Launch Discord as usual.") }
                : {
                    level: "unknown",
                    pill: tr("Обход выключен", "Bypass off"),
                    title: tr("Обход выключен", "The bypass is off"),
                    text:
                      overview.settings.launchMode === "withBypass"
                        ? tr("Нажмите «Запустить Discord» — обход включится сам.", "Press “Launch Discord” — the bypass starts by itself.")
                        : tr("Без обхода Discord может не открываться.", "Discord may not open without the bypass."),
                  };

  const voice: { level: Level; text: string } =
    overview.voice === "userConfirmed"
      ? { level: "ok", text: tr("Голос подтверждён", "Voice confirmed") }
      : overview.voice === "userReportedBroken"
        ? { level: "bad", text: tr("Голос не работает", "Voice does not work") }
        : overview.voice === "noUdpRules"
          ? { level: "warn", text: tr("Голос не поддерживается этой стратегией", "Voice not supported by this strategy") }
          : { level: "unknown", text: tr("Голос не проверен", "Voice not checked") };

  return (
    <Card className="relative overflow-hidden">
      <div aria-hidden className="pointer-events-none absolute inset-0 bg-[radial-gradient(ellipse_at_top_left,rgb(var(--c-accent)/0.14),transparent_55%)]" />
      <div className="relative px-7 pb-6 pt-6">
        <div className="flex items-center gap-3">
          <div className="flex h-11 w-11 items-center justify-center rounded-xl bg-accent text-white shadow-[0_6px_20px_-6px_rgb(var(--c-accent)/0.6)]">
            <Headset className="h-6 w-6" aria-hidden />
          </div>
          <div className="min-w-0 flex-1">
            <div className="text-[15px] font-semibold leading-tight">Discord</div>
            <div className="truncate text-xs text-ink-faint">{d.installed ? `${d.install?.channel ?? ""} ${d.install?.version ?? ""}${d.running ? tr(" · открыт", " · running") : ""}` : tr("не установлен на этом компьютере", "not installed on this PC")}</div>
          </div>
          <Pill level={hero.level}>{hero.pill}</Pill>
        </div>

        <div className="mt-6 flex items-start gap-3">
          <LevelIcon level={hero.level} className="mt-1 h-6 w-6" />
          <div className="min-w-0">
            <h1 className="text-[22px] font-semibold leading-tight tracking-[-0.01em]">{hero.title}</h1>
            <p className="selectable mt-1.5 max-w-xl text-sm leading-relaxed text-ink-muted">{hero.text}</p>
          </div>
        </div>

        {autoRunning && (
          <div className="mt-4 h-1.5 overflow-hidden rounded-full bg-surface-3">
            <div className="h-full rounded-full bg-accent transition-all duration-500" style={{ width: `${auto?.maxAttempts ? Math.max(4, (auto.attempt / auto.maxAttempts) * 100) : 4}%` }} />
          </div>
        )}
        {!autoRunning && auto?.result && <div className={`mt-4 rounded-md border px-3 py-2 text-xs ${auto.successId ? "border-ok/30 bg-ok-soft text-ok" : "border-warn/30 bg-warn-soft text-warn"}`}>{auto.result}</div>}

        <div className="mt-6 flex flex-wrap items-center gap-3">
          {autoRunning ? (
            <Button size="lg" variant="danger" icon={<Square className="h-4 w-4" />} onClick={autoSel.cancel}>
              {tr("Остановить подбор", "Stop searching")}
            </Button>
          ) : d.installed ? (
            <Button size="lg" variant="primary" className="h-11 px-6 text-[15px]" icon={<Play className="h-[18px] w-[18px]" />} loading={launching} disabled={busy} onClick={launch}>
              {tr("Запустить Discord", "Launch Discord")}
            </Button>
          ) : (
            <Button size="lg" variant="primary" className="h-11 px-6 text-[15px]" icon={<Download className="h-[18px] w-[18px]" />} onClick={() => api.openExternal("discord-download")}>
              {tr("Скачать Discord", "Get Discord")}
            </Button>
          )}
          <Button variant="ghost" aria-expanded={more} onClick={() => setMore(!more)} icon={<ChevronDown className={`h-4 w-4 transition-transform ${more ? "rotate-180" : ""}`} />}>
            {tr("Ещё", "More")}
          </Button>
          <div className="ml-auto flex items-center gap-2 text-xs">
            <Mic className={`h-3.5 w-3.5 ${tone[voice.level]}`} />
            <span className={tone[voice.level]}>{voice.text}</span>
            {running && overview.voice !== "noUdpRules" && overview.voice !== "userConfirmed" && (
              <span className="flex gap-1">
                <button className="rounded px-1.5 py-0.5 text-ink-muted hover:bg-surface-3 hover:text-ok" onClick={() => confirmVoice(true)}>
                  {tr("работает", "works")}
                </button>
                <button className="rounded px-1.5 py-0.5 text-ink-muted hover:bg-surface-3 hover:text-bad" onClick={() => confirmVoice(false)}>
                  {tr("нет", "no")}
                </button>
              </span>
            )}
          </div>
        </div>
      </div>

      <AnimatePresence initial={false}>
        {more && (
          <motion.div initial={reduce ? false : { height: 0, opacity: 0 }} animate={{ height: "auto", opacity: 1 }} exit={reduce ? undefined : { height: 0, opacity: 0 }} transition={{ duration: 0.18, ease: "easeOut" }} className="relative overflow-hidden border-t border-line bg-surface-2/40">
            <div className="grid grid-cols-2 gap-x-4 gap-y-0.5 p-3 narrow:grid-cols-1">
              <ActionRow icon={<Sparkles className="h-4 w-4" />} title={tr("Подобрать автоматически", "Find automatically")} hint={tr("Переберём стратегии и оставим рабочую", "Try strategies and keep a working one")} onClick={autoSel.start} disabled={!helperOk || autoRunning || busy} />
              <ActionRow icon={<RefreshCw className={`h-4 w-4 ${checking ? "animate-spin" : ""}`} />} title={tr("Проверить доступ", "Check access")} hint={acc.hint ?? acc.value} onClick={check} disabled={checking || autoRunning} />
              <ActionRow icon={<Layers className="h-4 w-4" />} title={tr("Стратегия", "Strategy")}>
                <Select
                  ariaLabel={tr("Стратегия", "Strategy")}
                  className="h-8 w-48 py-1 text-[13px]"
                  value={activeId}
                  disabled={busy || autoRunning}
                  onChange={changeStrategy}
                  options={strategies.map((s) => ({ value: s.id, label: s.name, group: s.source === "flowseal" ? tr("Встроенные", "Built-in") : tr("Мои", "Mine") }))}
                />
              </ActionRow>
              <ActionRow icon={<Power className="h-4 w-4" />} title={running ? tr("Выключить обход", "Turn off the bypass") : tr("Включить обход", "Turn on the bypass")} hint={running ? tr("Discord может перестать открываться", "Discord may stop loading") : tr("Без запуска Discord", "Without launching Discord")} onClick={toggle} disabled={busy || !helperOk || autoRunning} />
              <ActionRow icon={<Activity className="h-4 w-4" />} title={tr("Подробная диагностика", "Detailed diagnostics")} hint={tr("Что именно не работает", "What exactly fails")} onClick={() => setPage("diagnostics")} />
              <ActionRow icon={<AppWindow className="h-4 w-4" />} title={tr("Каталог приложений", "App catalog")} hint={tr("Обход для игр и сервисов", "Bypass for games and services")} onClick={() => setPage("lists")} />
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </Card>
  );
}

function Stat({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-3 py-1.5 text-[13px]">
      <span className="shrink-0 text-ink-faint">{label}</span>
      <span className="min-w-0 truncate text-right text-ink">{children}</span>
    </div>
  );
}

function ZapretCard() {
  const { overview, strategies, setPage, lang, catalog } = useApp();
  const tr = useTr();
  const count = useCount();
  const activate = useActivate();
  if (!overview) return null;
  const running = overview.engine?.state === "running";
  const activeId = (running ? overview.engine?.strategyId : null) ?? overview.settings.activeStrategyId ?? "";
  const st = strategies.find((s) => s.id === activeId);
  const kind = st ? strategyKind(st.name, st.notes) : null;
  const installed = overview.settings.installedPacks ?? [];
  const flowseal = (catalog?.packs ?? []).filter((v) => v.state === "flowseal").map((v) => loc(v.pack.name, lang));
  const apps = [...flowseal, ...installed.map((i) => installedTitle(i, lang))];
  const own = ownEntries(overview.settings.userLists.general).length;
  return (
    <Card className="flex flex-col p-5">
      <div className="flex items-center gap-3">
        <span className="flex h-9 w-9 items-center justify-center rounded-lg bg-surface-3 text-ink-muted">
          <Route className="h-[18px] w-[18px]" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-sm font-semibold">Zapret</div>
          <div className="text-xs text-ink-faint">{tr("движок обхода", "bypass engine")}</div>
        </div>
        <Pill level={running ? "ok" : "unknown"}>{running ? tr("включён", "on") : tr("выключен", "off")}</Pill>
      </div>
      <p className="mt-3 text-[13px] leading-relaxed text-ink-muted">{tr("Работает для Discord и выбранных приложений — игр, лаунчеров, сервисов.", "Works for Discord and the apps you choose — games, launchers, services.")}</p>
      <div className="mt-3 divide-y divide-line">
        <Stat label={tr("Стратегия", "Strategy")}>
          {st?.name ?? "—"}
          {kind && <span className="ml-1.5 text-ink-faint">· {tr(kind.ru, kind.en)}</span>}
        </Stat>
        <Stat label={tr("Приложения", "Apps")}>{apps.join(", ") || "—"}</Stat>
        <Stat label={tr("Мои адреса", "My entries")}>{own ? count(own, ["адрес", "адреса", "адресов"], ["entry", "entries"]) : tr("нет", "none")}</Stat>
      </div>
      {overview.pendingApply && (
        <div className="mt-3 flex items-center gap-2 rounded-md border border-warn/30 bg-warn-soft px-3 py-2 text-xs text-warn">
          <TriangleAlert className="h-3.5 w-3.5 shrink-0" />
          <span className="flex-1">{tr("Изменения ещё не применены", "Changes are not applied yet")}</span>
          <button className="inline-flex items-center gap-1 font-medium hover:underline" onClick={() => activate(activeId || null)}>
            <RotateCw className="h-3 w-3" /> {tr("Применить", "Apply")}
          </button>
        </div>
      )}
      <div className="mt-auto flex gap-2 pt-4">
        <Button size="sm" variant="secondary" onClick={() => setPage("strategies")}>
          {tr("Стратегии", "Strategies")}
        </Button>
        <Button size="sm" variant="secondary" onClick={() => setPage("lists")}>
          {tr("Приложения", "Apps")}
        </Button>
      </div>
    </Card>
  );
}

/** Telegram WS Proxy: a local MTProto proxy (Flowseal tg-ws-proxy engine) run
 * by Bridge as its own process, independent of Zapret. The state comes
 * from the real process; nothing claims that Telegram, media or calls work. */
function TelegramCard() {
  const { telegram, setTelegram, toast, openSettings, overview, refresh } = useApp();
  const reduce = useReducedMotion();
  const [more, setMore] = useState(false);
  const tr = useTr();
  const [busy, setBusy] = useState<string | null>(null);
  const [diag, setDiag] = useState<TelegramDiagItem[] | null>(null);
  const [help, setHelp] = useState<"connect" | "noTelegram" | "disconnect" | null>(null);
  const v = telegram;
  const st = v?.status;
  const label = st ? stateLabel(st.state, tr) : { level: "unknown" as Level, text: "…" };
  const can = actions(v);
  const run = async (key: string, f: () => Promise<void>) => {
    setBusy(key);
    try {
      await f();
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setBusy(null);
    }
  };
  const start = () => run("start", async () => setTelegram(await api.telegramStart()));
  const stop = () =>
    run("stop", async () => {
      setTelegram(await api.telegramStop());
      setHelp("disconnect");
    });
  const restart = () => run("restart", async () => setTelegram(await api.telegramRestart()));
  const connect = () =>
    run("connect", async () => {
      const r = await api.telegramOpen();
      if (r.opened) {
        toast("info", tr("Ссылка передана Telegram Desktop — подтвердите подключение прокси в окне Telegram", "The link was passed to Telegram Desktop — confirm the proxy in Telegram"));
        setHelp("connect");
      } else setHelp("noTelegram");
    });
  const copyLink = () =>
    run("copy", async () => {
      const link = await api.telegramLink();
      await navigator.clipboard.writeText(link);
      toast("ok", tr("Ссылка скопирована. В ней секрет прокси — не публикуйте её", "Link copied. It contains the proxy secret — do not share it"));
    });
  const diagnostics = () => run("diag", async () => setDiag(await api.telegramDiagnostics()));
  // The same setting as Settings → Telegram Proxy (telegramProxy.autostart).
  const autostart = overview?.settings.telegramProxy?.autostart ?? false;
  // Not disabled while saving (a disabled control drops keyboard focus);
  // repeated presses during the save are ignored instead.
  const setAutostart = (on: boolean) =>
    busy !== "autostart" &&
    run("autostart", async () => {
      const cur = await api.getSettings();
      await api.saveSettings({ ...cur, telegramProxy: { ...cur.telegramProxy, autostart: on } });
      await refresh();
      setTelegram(await api.telegramGet());
    });
  return (
    <Card className="flex flex-col overflow-hidden">
      <div className="flex flex-1 flex-col p-5">
        <div className="flex items-center gap-3">
          <span className="flex h-9 w-9 items-center justify-center rounded-lg bg-[#2aabee]/15 text-[#2aabee]">
            <Send className="h-[18px] w-[18px]" />
          </span>
          <div className="min-w-0 flex-1">
            <div className="text-sm font-semibold">Telegram</div>
            <div className="text-xs text-ink-faint">WS Proxy · MTProto</div>
          </div>
          <Pill level={label.level}>{label.text}</Pill>
        </div>
        <p className="mt-3 text-[13px] leading-relaxed text-ink-muted">
          {tr("Локальный прокси для Telegram Desktop. Работает отдельно от Zapret: каждый можно включать и выключать независимо.", "A local proxy for Telegram Desktop. Independent of Zapret: each can be turned on and off separately.")}
        </p>
        {st?.lastError && (st.state === "error" || st.state === "notInstalled") && <div className="selectable mt-3 rounded-md border border-bad/30 bg-bad-soft px-3 py-2 text-xs text-bad">{st.lastError}</div>}
        {v?.component.problem && <div className="mt-3 rounded-md border border-bad/30 bg-bad-soft px-3 py-2 text-xs text-bad">{v.component.problem}</div>}
        {v?.pendingRestart && (
          <div className="mt-3 flex items-center gap-2 rounded-md border border-warn/30 bg-warn-soft px-3 py-2 text-xs text-warn">
            <TriangleAlert className="h-3.5 w-3.5 shrink-0" />
            <span className="flex-1">{portNote(v, tr) ?? tr("Настройки изменены — перезапустите прокси", "Settings changed — restart the proxy")}</span>
          </div>
        )}
        <div className="mt-auto flex flex-wrap items-center gap-1.5 pt-4">
          {can.includes("start") && (
            <Button size="sm" variant="primary" icon={<Play className="h-3.5 w-3.5" />} loading={busy === "start"} disabled={!!busy} onClick={start}>
              {tr("Запустить", "Start")}
            </Button>
          )}
          {can.includes("connect") && (
            <Button size="sm" variant="primary" icon={<Send className="h-3.5 w-3.5" />} loading={busy === "connect"} disabled={!!busy} onClick={connect}>
              {tr("Подключить Telegram", "Connect Telegram")}
            </Button>
          )}
          {can.includes("stop") && (
            <Button size="sm" icon={<Square className="h-3.5 w-3.5" />} loading={busy === "stop"} disabled={!!busy} onClick={stop}>
              {tr("Остановить", "Stop")}
            </Button>
          )}
          <Button size="sm" variant="ghost" className="ml-auto px-2" aria-expanded={more} aria-controls="telegram-more" onClick={() => setMore(!more)} icon={<ChevronDown className={`h-4 w-4 transition-transform ${more ? "rotate-180" : ""}`} />}>
            {tr("Ещё", "More")}
          </Button>
        </div>
      </div>

      <AnimatePresence initial={false}>
        {more && (
          <motion.div
            id="telegram-more"
            initial={reduce ? false : { height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={reduce ? undefined : { height: 0, opacity: 0 }}
            transition={{ duration: 0.18, ease: "easeOut" }}
            className="relative overflow-hidden border-t border-line bg-surface-2/40"
          >
            {v && (
              <div className="divide-y divide-line px-5 pt-2">
                <Stat label={tr("Адрес", "Address")}>127.0.0.1:{v.connectionPort}</Stat>
                <Stat label={tr("Маршрут", "Route")}>{routeText(v, tr)}</Stat>
                <Stat label={tr("Клиент", "Client")}>{clientText(v, tr)}</Stat>
                <Stat label={tr("Версия", "Version")}>{v.component.installed ? `tg-ws-proxy ${v.component.version}` : tr("не установлен", "not installed")}</Stat>
              </div>
            )}
            {/* One column in the half-width card; two when the cards stack (narrow window). */}
            <div className="grid grid-cols-1 gap-x-4 gap-y-0.5 p-3 narrow:grid-cols-2">
              {can.includes("restart") && <ActionRow icon={<RotateCw className="h-4 w-4" />} title={tr("Перезапустить", "Restart")} hint={tr("Применить изменённые настройки", "Apply changed settings")} onClick={restart} disabled={!!busy} />}
              {can.includes("copyLink") && <ActionRow icon={<Copy className="h-4 w-4" />} title={tr("Копировать ссылку", "Copy link")} hint={tr("Содержит секрет — не публикуйте", "Contains the secret — do not share")} onClick={copyLink} disabled={!!busy} />}
              {can.includes("diagnostics") && <ActionRow icon={<Activity className="h-4 w-4" />} title={tr("Диагностика", "Diagnostics")} hint={tr("Процесс, порт, клиент, маршрут", "Process, port, client, route")} onClick={diagnostics} disabled={!!busy} />}
              {can.includes("settings") && <ActionRow icon={<SettingsIcon className="h-4 w-4" />} title={tr("Настройки", "Settings")} hint={tr("Порт, резервный маршрут, секрет", "Port, fallback route, secret")} onClick={() => openSettings("telegram")} />}
              <ActionRow icon={<Power className="h-4 w-4" />} title={tr("Запускать вместе с Bridge", "Start with Bridge")} hint={tr("Не зависит от автовключения обхода", "Independent of the bypass autostart")}>
                <Switch label={tr("Запускать Telegram Proxy вместе с Bridge", "Start Telegram Proxy with Bridge")} checked={autostart} disabled={!v?.component.installed} onChange={(on) => void setAutostart(on)} />
              </ActionRow>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
      <Modal open={diag !== null} onClose={() => setDiag(null)} title={tr("Диагностика Telegram Proxy", "Telegram Proxy diagnostics")}>
        <ul className="space-y-2.5">
          {(diag ?? []).map((d) => (
            <li key={d.id} className="flex items-start gap-2.5 text-[13px]">
              <LevelIcon level={d.status} className="mt-0.5 h-4 w-4" />
              <div className="min-w-0">
                <div className="font-medium">{d.title}</div>
                <div className="text-xs text-ink-muted">{d.status === "unknown" && !d.detail.includes("не проверено") ? `${tr("Не проверено", "Not checked")}: ${d.detail}` : d.detail}</div>
              </div>
            </li>
          ))}
        </ul>
        <p className="mt-3 text-[11px] text-ink-faint">{tr("Открытый локальный порт не доказывает, что Telegram работает: это подтверждают только реальные подключения клиента и данные от серверов Telegram.", "An open local port does not prove that Telegram works: only real client connections and data from Telegram servers do.")}</p>
      </Modal>
      <Modal open={help !== null} onClose={() => setHelp(null)} title={help === "disconnect" ? tr("Прокси остановлен", "Proxy stopped") : tr("Подключение Telegram Desktop", "Connecting Telegram Desktop")}>
        <TelegramHelp kind={help} port={v?.connectionPort ?? 1443} onCopy={copyLink} />
      </Modal>
    </Card>
  );
}

function TelegramHelp({ kind, port, onCopy }: { kind: "connect" | "noTelegram" | "disconnect" | null; port: number; onCopy: () => void }) {
  const tr = useTr();
  if (kind === "disconnect")
    return (
      <div className="space-y-2 text-[13px] text-ink-muted">
        <p>{tr("Если Telegram был подключён через этот прокси, он продолжит пытаться использовать 127.0.0.1 и не сможет соединиться.", "If Telegram was connected through this proxy, it keeps trying 127.0.0.1 and cannot connect.")}</p>
        <p>{tr("Чтобы вернуть прямое соединение: Telegram Desktop → Настройки → Продвинутые настройки → Тип подключения → «Без прокси» (или отключите прокси 127.0.0.1 в списке).", "To go back to a direct connection: Telegram Desktop → Settings → Advanced → Connection type → “Disable proxy” (or turn off the 127.0.0.1 proxy in the list).")}</p>
        <p>{tr("Zapret эта остановка не затрагивает.", "Zapret is not affected by this stop.")}</p>
      </div>
    );
  return (
    <div className="space-y-2 text-[13px] text-ink-muted">
      {kind === "noTelegram" && <p className="text-warn">{tr("Telegram Desktop не найден: ссылки tg:// некому открыть. Установите Telegram Desktop или подключите прокси вручную.", "Telegram Desktop was not found: nothing handles tg:// links. Install Telegram Desktop or add the proxy manually.")}</p>}
      {kind === "connect" && <p>{tr("В открывшемся окне Telegram нажмите «Подключиться» / «Включить».", "In the Telegram window that opens, press “Connect” / “Enable”.")}</p>}
      <p className="font-medium text-ink">{tr("Вручную:", "Manually:")}</p>
      <ol className="list-decimal space-y-1 pl-5">
        <li>{tr("Telegram Desktop → Настройки → Продвинутые настройки → Тип подключения.", "Telegram Desktop → Settings → Advanced → Connection type.")}</li>
        <li>{tr("«Использовать собственный прокси» → «Добавить прокси» → MTPROTO.", "“Use custom proxy” → “Add proxy” → MTPROTO.")}</li>
        <li>{tr(`Сервер 127.0.0.1, порт ${port}, секрет — из ссылки (вставьте ссылку целиком, Telegram заполнит поля сам).`, `Server 127.0.0.1, port ${port}, secret from the link (pasting the whole link fills the fields).`)}</li>
      </ol>
      <Button size="sm" icon={<Copy className="h-3.5 w-3.5" />} onClick={onCopy}>
        {tr("Копировать ссылку (содержит секрет)", "Copy link (contains the secret)")}
      </Button>
      <p className="text-xs">{tr("Прокси работает только на этом компьютере (127.0.0.1). После его остановки переключите Telegram обратно на прямое соединение.", "The proxy works on this computer only (127.0.0.1). After stopping it, switch Telegram back to a direct connection.")}</p>
    </div>
  );
}

export default function HomePage() {
  const { overview, overviewError } = useApp();
  const tr = useTr();

  if (!overview) {
    return overviewError ? (
      <Card className="flex items-start gap-3 p-5 text-sm">
        <XCircle className="h-5 w-5 shrink-0 text-bad" />
        <div>
          <div className="font-medium">{tr("Не удалось получить состояние", "Could not load the state")}</div>
          <div className="selectable mt-1 text-xs text-ink-muted">{overviewError}</div>
        </div>
      </Card>
    ) : (
      <div className="space-y-4" aria-busy="true">
        <Skeleton className="h-[290px] w-full rounded-lg" />
        <div className="grid grid-cols-2 gap-4">
          <Skeleton className="h-[260px] rounded-lg" />
          <Skeleton className="h-[260px] rounded-lg" />
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <DiscordHero />
      <div className="grid grid-cols-2 gap-4 narrow:grid-cols-1">
        <ZapretCard />
        <TelegramCard />
      </div>
      {(overview.settingsRecovered || overview.strategyLoadErrors.length > 0 || overview.catalogErrors.length > 0) && (
        <div className="rounded-md border border-warn/30 bg-warn-soft px-4 py-3 text-xs text-warn">
          {overview.settingsRecovered && <div>{tr("Файл настроек был повреждён и восстановлен из резервной копии.", "Settings were corrupted and restored from backup.")}</div>}
          {overview.strategyLoadErrors.map((e) => (
            <div key={e}>{e}</div>
          ))}
          {overview.catalogErrors.map((e) => (
            <div key={e}>Flowseal: {e}</div>
          ))}
        </div>
      )}
    </div>
  );
}
