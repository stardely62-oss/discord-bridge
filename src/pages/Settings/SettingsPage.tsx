import { open } from "@tauri-apps/plugin-dialog";
import { Download, ExternalLink, FolderOpen, RefreshCw, RotateCcw } from "lucide-react";
import { ReactNode, useEffect, useState } from "react";
import { Badge, Button, Card, NumberField, PageHeader, PageSkeleton, Segmented, Select, SettingRow, Switch } from "../../components/ui";
import { confirmDialog } from "../../components/ui/confirm";
import { api, errorText } from "../../services/api";
import type { CompatEntry, FlowsealAction, Settings, TcpInfo, UpdateInfo } from "../../services/types";
import { useApp, useTr, type SettingsTab } from "../../stores/app";
import { appReleaseTarget, checkState, type CheckState } from "../../updates/notices";

type Tab = SettingsTab;

/** Flowseal install/switch progress on this page. */
type FsPhase = { kind: "idle" } | { kind: "installing"; version: string } | { kind: "done"; result: FlowsealAction; incompatible: CompatEntry[] };

function StatusBadge({ state }: { state: CheckState }) {
  const tr = useTr();
  const m: Record<CheckState, [string, string, "neutral" | "accent" | "ok" | "warn" | "bad"]> = {
    notChecked: ["Не проверялось", "Not checked", "neutral"],
    checking: ["Проверяется…", "Checking…", "neutral"],
    upToDate: ["Обновлений нет", "No updates", "ok"],
    available: ["Доступна новая версия", "New version available", "accent"],
    unavailable: ["Обновление пока недоступно", "Update not available yet", "warn"],
    error: ["Ошибка проверки", "Check failed", "bad"],
  };
  const [ru, en, tone] = m[state];
  return <Badge tone={tone}>{tr(ru, en)}</Badge>;
}

function Facts({ rows }: { rows: [string, ReactNode][] }) {
  return (
    <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1.5 py-3 text-xs">
      {rows.map(([k, v]) => (
        <div key={k} className="contents">
          <dt className="text-ink-muted">{k}</dt>
          <dd className="min-w-0 break-words">{v}</dd>
        </div>
      ))}
    </dl>
  );
}

function checkedAt(info: UpdateInfo | null | undefined, lang: string): string {
  if (!info?.checkedAt) return "";
  return new Date(info.checkedAt * 1000).toLocaleTimeString(lang === "en" ? "en-GB" : "ru-RU", { hour: "2-digit", minute: "2-digit" });
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Card className="px-5 py-1">
      <div className="label pb-1 pt-3 uppercase tracking-wide text-ink-faint">{title}</div>
      <div className="divide-y divide-line">{children}</div>
    </Card>
  );
}

export default function SettingsPage() {
  const { overview, toast, refresh, setLang, updates, setUpdates, settingsTab, clearSettingsTab, lang } = useApp();
  const tr = useTr();
  const [tab, setTab] = useState<Tab>(settingsTab ?? "general");
  const [s, setS] = useState<Settings | null>(null);
  const [saving, setSaving] = useState(false);
  const [checkingApp, setCheckingApp] = useState(false);
  const [checkingFs, setCheckingFs] = useState(false);
  const [fsPhase, setFsPhase] = useState<FsPhase>({ kind: "idle" });
  const [tcp, setTcp] = useState<TcpInfo | null>(null);

  useEffect(() => {
    api.getSettings().then(setS).catch((e) => toast("error", errorText(e)));
  }, [toast]);

  // "Go to update" from a notice opens this tab.
  useEffect(() => {
    if (settingsTab) {
      setTab(settingsTab);
      clearSettingsTab();
    }
  }, [settingsTab, clearSettingsTab]);

  if (!s) return <PageSkeleton rows={4} />;

  const persist = async (next: Settings) => {
    setSaving(true);
    try {
      const saved = await api.saveSettings(next);
      setS(saved);
      setLang(saved.language);
      refresh();
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setSaving(false);
    }
  };
  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => persist({ ...s, [k]: v });

  const browseDiscord = async () => {
    const p = await open({ multiple: false, filters: [{ name: "Discord", extensions: ["exe"] }] });
    if (!p || Array.isArray(p)) return;
    try {
      await api.setDiscordPath(p);
      setS(await api.getSettings());
      refresh();
      toast("ok", tr("Путь к Discord сохранён", "Discord path saved"));
    } catch (e) {
      toast("error", errorText(e));
    }
  };

  const appInfo = updates?.app;
  const fsInfo = updates?.flowseal;
  const reload = async () => {
    try {
      setUpdates(await api.updatesGet());
    } catch {
      // the check results below are still shown
    }
  };

  const checkApp = async () => {
    setCheckingApp(true);
    try {
      await api.appCheckUpdate();
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      await reload();
      setCheckingApp(false);
    }
  };

  const checkFlowseal = async () => {
    setCheckingFs(true);
    try {
      await api.flowsealCheck();
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      await reload();
      setCheckingFs(false);
    }
  };

  const finishAction = async (result: FlowsealAction) => {
    let incompatible: CompatEntry[] = [];
    if (result.ok) {
      incompatible = await api.flowsealCompat(result.version).catch(() => []);
    }
    setFsPhase({ kind: "done", result, incompatible });
    toast(result.ok ? "ok" : "error", result.message);
    refresh();
    await reload();
  };

  const installFlowseal = async () => {
    const version = fsInfo?.latest;
    if (!version || !fsInfo?.installable) return;
    const ok = await confirmDialog({
      title: tr(`Установить Flowseal ${version}?`, `Install Flowseal ${version}?`),
      body: tr("Перед установкой проверяются подпись манифеста и хэши всех файлов — в приложении и ещё раз в службе. Ваши стратегии не изменятся, прежняя версия останется установленной, и к ней можно будет вернуться. Если новая версия не запустится, прежняя восстановится автоматически.", "The manifest signature and all file hashes are verified first — in the app and again in the service. Your strategies are not changed; the previous version stays installed and can be restored. If the new version fails to start, the previous one is restored automatically."),
      confirmLabel: tr("Установить", "Install"),
    });
    if (!ok) return;
    setFsPhase({ kind: "installing", version });
    try {
      await finishAction(await api.flowsealInstall(version));
    } catch (e) {
      setFsPhase({ kind: "done", result: { ok: false, outcome: "refused", message: errorText(e), version }, incompatible: [] });
      toast("error", errorText(e));
    }
  };

  const switchVersion = async (version: string) => {
    try {
      const broken = await api.flowsealCompat(version);
      if (broken.length > 0) {
        const ok = await confirmDialog({
          title: tr(`Переключиться на Flowseal ${version}?`, `Switch to Flowseal ${version}?`),
          body: (
            <div className="space-y-2">
              <p>{tr("С этой версией не будут работать ваши стратегии (сами профили не изменятся):", "These strategies will not work with this version (profiles are not modified):")}</p>
              <ul className="space-y-1 text-xs">
                {broken.map((b) => (
                  <li key={b.id} className="rounded-md bg-surface-2 px-2.5 py-1.5">
                    <span className="font-medium text-ink">{b.name}</span> — {b.problems[0]}
                  </li>
                ))}
              </ul>
            </div>
          ),
          confirmLabel: tr("Всё равно переключить", "Switch anyway"),
        });
        if (!ok) return;
      }
      setFsPhase({ kind: "installing", version });
      await finishAction(await api.flowsealSetActive(version));
    } catch (e) {
      setFsPhase({ kind: "idle" });
      toast("error", errorText(e));
    }
  };

  const enableTs = async () => {
    const ok = await confirmDialog({
      title: tr("Включить TCP timestamps?", "Enable TCP timestamps?"),
      body: tr("Это глобальная настройка TCP Windows, она нужна части стратегий (оригинальный Flowseal включает её всегда). Исходное значение запоминается — его можно вернуть здесь же.", "A global Windows TCP setting needed by some strategies (original Flowseal always enables it). The original value is recorded and can be restored here."),
      code: "netsh interface tcp set global timestamps=enabled",
      confirmLabel: tr("Включить", "Enable"),
    });
    if (!ok) return;
    try {
      const r = await api.setTcpTimestamps(true, true);
      setTcp(r);
      toast("ok", r.message);
    } catch (e) {
      toast("error", errorText(e));
    }
  };

  const restoreTs = async () => {
    try {
      const r = await api.restoreTcpTimestamps();
      setTcp(r);
      toast("ok", r.message);
    } catch (e) {
      toast("error", errorText(e));
    }
  };

  const gf = s.gameFilter;
  return (
    <div className="space-y-4">
      <PageHeader
        title={tr("Настройки", "Settings")}
        subtitle={saving ? tr("Сохранение…", "Saving…") : tr("Изменения сохраняются сразу", "Changes are saved immediately")}
        actions={
          <Segmented<Tab>
            ariaLabel={tr("Разделы настроек", "Settings sections")}
            value={tab}
            onChange={setTab}
            options={[
              { value: "general", label: tr("Основные", "General") },
              { value: "bypass", label: tr("Обход", "Bypass") },
              { value: "updates", label: tr("Обновления", "Updates") },
            ]}
          />
        }
      />

      {tab === "general" && (
        <>
          <Section title={tr("Запуск", "Startup")}>
            <SettingRow
              title={
                <span className="flex items-center gap-2">
                  {tr("Запускать вместе с Windows", "Launch with Windows")} <Badge>Windows</Badge>
                </span>
              }
              description={tr("Тихо, свёрнутым в трей. Меняет автозагрузку Windows для вашего пользователя.", "Quietly, in the tray. Changes Windows startup for your user.")}
            >
              <Switch label="autostart" checked={overview?.autostartEnabled ?? s.launchAtStartup} onChange={(v) => set("launchAtStartup", v)} />
            </SettingRow>
            <SettingRow title={tr("Запускать свёрнутым", "Start minimized")}>
              <Switch label="minimized" checked={s.startMinimized} onChange={(v) => set("startMinimized", v)} />
            </SettingRow>
            <SettingRow title={tr("Сразу включать обход", "Turn on the bypass at start")} description={tr("При запуске приложения.", "When the app starts.")}>
              <Switch label="auto bypass" checked={s.autoEnableBypass} onChange={(v) => set("autoEnableBypass", v)} />
            </SettingRow>
            <SettingRow title={tr("Затем запускать Discord", "Then launch Discord")}>
              <Switch label="auto discord" checked={s.autoLaunchDiscord} onChange={(v) => set("autoLaunchDiscord", v)} />
            </SettingRow>
          </Section>

          <Section title="Discord">
            <SettingRow
              title={tr("Кнопка «Запустить Discord»", "«Launch Discord» button")}
              description={s.launchMode === "withBypass" ? tr("Сначала включает обход. Если он не запустится, предложит открыть Discord без него.", "Starts the bypass first. If it fails, offers to open Discord without it.") : tr("Просто открывает Discord, обход не трогает.", "Just opens Discord, leaves the bypass alone.")}
            >
              <Select
                ariaLabel="launch mode"
                className="w-44"
                value={s.launchMode}
                onChange={(v) => set("launchMode", v as Settings["launchMode"])}
                options={[
                  { value: "withBypass", label: tr("С обходом", "With bypass") },
                  { value: "withoutBypass", label: tr("Без обхода", "Without bypass") },
                ]}
              />
            </SettingRow>
            <SettingRow
              title={tr("Где установлен Discord", "Discord location")}
              description={<span className="selectable break-all">{s.discordPath ? s.discordPath : overview?.discord.install ? tr(`Найден автоматически: ${overview.discord.install.exe}`, `Found: ${overview.discord.install.exe}`) : tr("Не найден автоматически", "Not found")}</span>}
            >
              <div className="flex gap-2">
                <Button size="sm" icon={<FolderOpen className="h-3.5 w-3.5" />} onClick={browseDiscord}>
                  {tr("Выбрать…", "Browse…")}
                </Button>
                {s.discordPath && (
                  <Button
                    size="sm"
                    variant="ghost"
                    icon={<RotateCcw className="h-3.5 w-3.5" />}
                    onClick={async () => {
                      await api.setDiscordPath(null);
                      setS(await api.getSettings());
                      refresh();
                    }}
                  >
                    {tr("Авто", "Auto")}
                  </Button>
                )}
              </div>
            </SettingRow>
          </Section>

          <Section title={tr("Окно и трей", "Window & tray")}>
            <SettingRow title={tr("Крестик окна", "Window close button")}>
              <Select
                ariaLabel="close"
                className="w-48"
                value={s.closeBehavior}
                onChange={(v) => set("closeBehavior", v as Settings["closeBehavior"])}
                options={[
                  { value: "tray", label: tr("Сворачивает в трей", "Minimizes to tray") },
                  { value: "exit", label: tr("Закрывает приложение", "Quits the app") },
                ]}
              />
            </SettingRow>
            <SettingRow
              title={tr("Выключать обход при выходе", "Turn off the bypass on exit")}
              description={s.stopBypassOnExit ? tr("При выходе из приложения обход выключится.", "The bypass stops when you quit.") : tr("Обход продолжит работать после выхода.", "The bypass keeps running after you quit.")}
            >
              <Switch label="stop on exit" checked={s.stopBypassOnExit} onChange={(v) => set("stopBypassOnExit", v)} />
            </SettingRow>
          </Section>

          <Section title={tr("Внешний вид", "Appearance")}>
            <SettingRow title={tr("Тема", "Theme")}>
              <Segmented<Settings["theme"]>
                ariaLabel="theme"
                value={s.theme}
                onChange={(v) => set("theme", v)}
                options={[
                  { value: "system", label: tr("Как в Windows", "System") },
                  { value: "dark", label: tr("Тёмная", "Dark") },
                  { value: "light", label: tr("Светлая", "Light") },
                ]}
              />
            </SettingRow>
            <SettingRow title={tr("Язык", "Language")}>
              <Select
                ariaLabel="language"
                className="w-40"
                value={s.language}
                onChange={(v) => set("language", v as Settings["language"])}
                options={[
                  { value: "ru", label: "Русский" },
                  { value: "en", label: "English" },
                ]}
              />
            </SettingRow>
            <SettingRow title={tr("Вести журнал", "Keep a log")} description={tr("Обезличенный, до 2 МБ. Помогает разобраться с проблемами.", "Anonymised, up to 2 MB. Helps troubleshooting.")}>
              <Switch label="logs" checked={s.keepLogs} onChange={(v) => set("keepLogs", v)} />
            </SettingRow>
          </Section>
        </>
      )}

      {tab === "bypass" && (
        <>
          <div className="rounded-md border border-line bg-surface-2 px-4 py-2.5 text-xs text-ink-muted">
            {tr("Параметры для опытных пользователей. Если всё работает — менять ничего не нужно.", "Advanced options. If everything works, there is nothing to change.")}
          </div>
          <Section title="Zapret">
            <SettingRow title={tr("Игровой фильтр", "Game filter")} description={tr("Обрабатывать и порты игр, как GameFilter во Flowseal. Может затронуть трафик других программ.", "Also handle game ports, like Flowseal GameFilter. May affect other programs' traffic.")}>
              <Select
                ariaLabel="game filter"
                className="w-40"
                value={gf.mode}
                onChange={(v) => set("gameFilter", { ...gf, mode: v as Settings["gameFilter"]["mode"] })}
                options={[
                  { value: "disabled", label: tr("Выключен", "Off") },
                  { value: "all", label: "TCP + UDP" },
                  { value: "tcp", label: "TCP" },
                  { value: "udp", label: "UDP" },
                ]}
              />
            </SettingRow>
            {gf.mode !== "disabled" && (
              <div className="grid grid-cols-2 gap-3 py-3">
                <label>
                  <span className="label">{tr("Порты TCP", "TCP ports")}</span>
                  <input className="field mt-1 font-mono" defaultValue={gf.tcpRange} onBlur={(e) => e.target.value !== gf.tcpRange && set("gameFilter", { ...gf, tcpRange: e.target.value })} />
                </label>
                <label>
                  <span className="label">{tr("Порты UDP", "UDP ports")}</span>
                  <input className="field mt-1 font-mono" defaultValue={gf.udpRange} onBlur={(e) => e.target.value !== gf.udpRange && set("gameFilter", { ...gf, udpRange: e.target.value })} />
                </label>
              </div>
            )}
            <SettingRow
              title={
                <span className="flex items-center gap-2">
                  {tr("Какие IP идут через обход", "Which IPs go through the bypass")}
                  {s.ipsetMode === "any" && <Badge tone="warn">{tr("рискованно", "risky")}</Badge>}
                </span>
              }
              description={
                {
                  none: tr("Только сайты из списков, IP-правила стратегий ничего не захватывают (по умолчанию, как во Flowseal).", "Only listed sites; strategy IP rules match nothing (default, as in Flowseal)."),
                  selected: tr("Только ваши «IP через обход» и IP установленных наборов каталога.", "Only your “IPs via bypass” and the IPs of installed catalog sets."),
                  flowseal: tr("Полный список IP из релиза Flowseal. Ваши IP и IP наборов не используются.", "The full IP list from the Flowseal release. Your IPs and set IPs are not used."),
                  flowsealPlusSelected: tr("Полный список Flowseal плюс ваши IP и IP наборов.", "The full Flowseal list plus your IPs and set IPs."),
                  any: tr("Все IP-адреса на перехватываемых портах. Может сломать другие программы — используйте, только если понимаете зачем.", "Every IP address on the captured ports. May break other programs — use only if you know why."),
                }[s.ipsetMode]
              }
            >
              <Select
                ariaLabel="ipset"
                className="w-52"
                value={s.ipsetMode}
                onChange={(v) => set("ipsetMode", v as Settings["ipsetMode"])}
                options={[
                  { value: "none", label: tr("Без IP", "No IPs") },
                  { value: "selected", label: tr("Выбранные IP", "Selected IPs") },
                  { value: "flowseal", label: tr("Полный список Flowseal", "Full Flowseal list") },
                  { value: "flowsealPlusSelected", label: tr("Flowseal + выбранные", "Flowseal + selected") },
                  { value: "any", label: tr("Все адреса (рискованно)", "All addresses (risky)") },
                ]}
              />
            </SettingRow>
            <SettingRow
              title={
                <span className="flex items-center gap-2">
                  TCP timestamps <Badge tone="warn">{tr("системная настройка Windows", "Windows system setting")}</Badge>
                </span>
              }
              description={
                tcp
                  ? `${tr("Сейчас", "Now")}: ${tcp.current == null ? tr("неизвестно", "unknown") : tcp.current ? tr("включены", "on") : tr("выключены", "off")}${tcp.changedByUs ? tr(" · изменено Discord Bridge", " · changed by Discord Bridge") : ""}`
                  : tr("Нужны стратегиям с fooling=ts. Меняется только с вашего согласия.", "Needed by fooling=ts strategies. Changed only with your consent.")
              }
            >
              <div className="flex gap-2">
                <Button size="sm" onClick={enableTs}>
                  {tr("Включить…", "Enable…")}
                </Button>
                <Button size="sm" variant="ghost" onClick={restoreTs}>
                  {tr("Вернуть исходное", "Restore")}
                </Button>
              </div>
            </SettingRow>
            <SettingRow title={tr("Попыток при автоподборе", "Auto-selection attempts")} description={tr("От 1 до 50.", "From 1 to 50.")}>
              <NumberField ariaLabel="auto attempts" className="w-28" min={1} max={50} value={s.autoMaxAttempts} onCommit={(v) => set("autoMaxAttempts", Math.max(1, Math.min(50, Number(v) || 8)))} />
            </SettingRow>
          </Section>
        </>
      )}

      {tab === "updates" && (
        <>
          <Section title={tr("Автоматические проверки", "Automatic checks")}>
            <SettingRow
              title={tr("Автоматически проверять обновления при запуске", "Check for updates automatically at startup")}
              description={tr("Проверять новые версии Discord Bridge и Flowseal при запуске приложения. Загрузка и установка обновлений выполняются только с вашего согласия.", "Check for new Discord Bridge and Flowseal versions when the app starts. Updates are downloaded and installed only with your consent.")}
            >
              <Switch label="check-updates-on-start" checked={s.checkUpdatesOnStart} onChange={(v) => set("checkUpdatesOnStart", v)} />
            </SettingRow>
          </Section>

          <Section title="Discord Bridge">
            <Facts
              rows={[
                [tr("Установлена", "Installed"), overview?.appVersion ?? "—"],
                [tr("Последняя обнаруженная", "Latest found"), appInfo?.latest ?? "—"],
                [
                  tr("Статус", "Status"),
                  <span key="s" className="flex flex-wrap items-center gap-2">
                    <StatusBadge state={checkState(appInfo, checkingApp || !!updates?.checkingApp)} />
                    {appInfo && <span className="text-ink-faint">{tr(`проверено в ${checkedAt(appInfo, lang)}`, `checked at ${checkedAt(appInfo, lang)}`)}</span>}
                  </span>,
                ],
                ...(appInfo?.error ? ([[tr("Ошибка", "Error"), <span key="e" className="text-bad">{appInfo.error}</span>]] as [string, ReactNode][]) : []),
              ]}
            />
            <div className="flex flex-wrap items-center gap-2 py-3">
              <Button size="sm" icon={<RefreshCw className="h-3.5 w-3.5" />} loading={checkingApp} onClick={checkApp}>
                {tr("Проверить", "Check")}
              </Button>
              {appReleaseTarget(appInfo) && (
                <Button size="sm" variant="primary" icon={<ExternalLink className="h-3.5 w-3.5" />} onClick={() => api.openExternal(appReleaseTarget(appInfo)!).catch((e) => toast("error", errorText(e)))}>
                  {tr(`Открыть релиз ${appInfo?.latest}`, `Open release ${appInfo?.latest}`)}
                </Button>
              )}
              <span className="text-xs text-ink-faint">{tr("Установка — вручную, установщиком со страницы релиза.", "Install manually with the installer from the release page.")}</span>
            </div>
            {appInfo?.status === "available" && appInfo.changelog && <pre className="selectable mb-3 max-h-48 overflow-auto whitespace-pre-wrap rounded-md border border-line bg-surface-2 p-3 font-sans text-xs text-ink-muted">{appInfo.changelog}</pre>}
          </Section>

          <Section title={tr("Flowseal Zapret — стратегии и движок", "Flowseal Zapret — strategies & engine")}>
            <Facts
              rows={[
                [tr("Активная версия", "Active version"), `${overview?.bundle.version ?? "—"} · winws ${overview?.bundle.winwsVersion ?? "—"}`],
                [tr("Последняя официальная", "Latest official"), fsInfo?.latest ?? "—"],
                [
                  tr("Статус", "Status"),
                  <span key="s" className="flex flex-wrap items-center gap-2">
                    <StatusBadge state={checkState(fsInfo, checkingFs || !!updates?.checkingFlowseal)} />
                    {fsInfo && <span className="text-ink-faint">{tr(`проверено в ${checkedAt(fsInfo, lang)}`, `checked at ${checkedAt(fsInfo, lang)}`)}</span>}
                  </span>,
                ],
                [
                  tr("Подписанный манифест", "Signed manifest"),
                  !fsInfo || !fsInfo.newer ? (
                    <span key="m" className="text-ink-muted">{tr("не требуется — новой версии нет", "not needed — no newer version")}</span>
                  ) : fsInfo.installable ? (
                    <span key="m" className="text-ok">{tr("проверен: подпись закреплённым ключом, версия и совместимость с приложением", "verified: pinned-key signature, version and app compatibility")}</span>
                  ) : fsInfo.status === "error" ? (
                    <span key="m" className="text-bad">{tr("не удалось проверить", "could not be checked")}</span>
                  ) : (
                    <span key="m" className="text-warn">{fsInfo.unavailableReason ?? tr("не опубликован", "not published")}</span>
                  ),
                ],
                ...(fsInfo?.error ? ([[tr("Ошибка", "Error"), <span key="e" className="text-bad">{fsInfo.error}</span>]] as [string, ReactNode][]) : []),
              ]}
            />
            <div className="flex flex-wrap items-center gap-2 py-3">
              <Button size="sm" icon={<RefreshCw className="h-3.5 w-3.5" />} loading={checkingFs} onClick={checkFlowseal}>
                {tr("Проверить", "Check")}
              </Button>
              {fsInfo?.status === "available" && fsInfo.installable && (
                <Button size="sm" variant="primary" icon={<Download className="h-3.5 w-3.5" />} loading={fsPhase.kind === "installing"} disabled={fsPhase.kind === "installing"} onClick={installFlowseal}>
                  {tr(`Установить ${fsInfo.latest}`, `Install ${fsInfo.latest}`)}
                </Button>
              )}
              {fsInfo?.newer && fsInfo.latest && (
                <Button size="sm" variant="ghost" icon={<ExternalLink className="h-3.5 w-3.5" />} onClick={() => api.openExternal(`flowseal-release:${fsInfo.latest}`).catch((e) => toast("error", errorText(e)))}>
                  {tr("Релиз Flowseal", "Flowseal release")}
                </Button>
              )}
            </div>
            {fsPhase.kind === "installing" && <div className="py-3 text-xs text-ink-muted">{tr(`Flowseal ${fsPhase.version}: установка выполняется — проверка подписи, загрузка официального архива, проверка хешей, переключение…`, `Flowseal ${fsPhase.version}: installing — signature check, official archive download, hash check, switching…`)}</div>}
            {fsPhase.kind === "done" && (
              <div className="space-y-2 py-3 text-xs">
                <div className="flex flex-wrap items-center gap-2">
                  <Badge tone={fsPhase.result.ok ? "ok" : fsPhase.result.outcome === "rolledBack" || fsPhase.result.outcome === "refused" || fsPhase.result.outcome === "installFailed" ? "warn" : "bad"}>
                    {fsPhase.result.ok
                      ? tr("Установка завершена", "Installation complete")
                      : fsPhase.result.outcome === "rolledBack"
                        ? tr("Переключение не удалось, предыдущая версия восстановлена", "Switch failed, the previous version was restored")
                        : fsPhase.result.outcome === "rollbackFailed"
                          ? tr("Переключение не удалось, прежнюю версию вернуть не удалось", "Switch failed and the previous version could not be restored")
                          : tr("Ничего не изменено", "Nothing was changed")}
                  </Badge>
                </div>
                <div className="selectable text-ink-muted">{fsPhase.result.message}</div>
                {fsPhase.incompatible.length > 0 && (
                  <div className="space-y-1">
                    <div className="text-warn">{tr(`С Flowseal ${fsPhase.result.version} не будут работать ваши стратегии (профили не изменены, можно вернуться к прежней версии ниже):`, `These strategies will not work with Flowseal ${fsPhase.result.version} (profiles unchanged; you can go back below):`)}</div>
                    <ul className="space-y-1">
                      {fsPhase.incompatible.map((b) => (
                        <li key={b.id} className="rounded-md bg-surface-2 px-2.5 py-1.5">
                          <span className="font-medium text-ink">{b.name}</span> — {b.problems[0]}
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
              </div>
            )}
            {fsInfo?.newer && fsInfo.changelog && <pre className="selectable mb-3 max-h-48 overflow-auto whitespace-pre-wrap rounded-md border border-line bg-surface-2 p-3 font-sans text-xs text-ink-muted">{fsInfo.changelog}</pre>}
            <div className="py-3">
              <div className="label mb-2">{tr("Установленные версии — можно вернуться к прежней", "Installed versions — you can go back")}</div>
              {(overview?.bundle.bundles ?? []).map((b) => (
                <div key={b.version} className="flex items-center justify-between py-1 text-sm">
                  <span className="flex items-center gap-2">
                    {b.version} {b.builtin && <Badge>{tr("встроенная", "built-in")}</Badge>} {!b.ok && <Badge tone="bad">{tr("файлы повреждены", "files damaged")}</Badge>}
                  </span>
                  {b.active ? (
                    <Badge tone="ok">{tr("активна", "active")}</Badge>
                  ) : (
                    <Button size="sm" variant="ghost" disabled={!b.ok || fsPhase.kind === "installing"} onClick={() => switchVersion(b.version)}>
                      {tr("Использовать", "Use")}
                    </Button>
                  )}
                </div>
              ))}
              {(overview?.bundle.bundles.length ?? 0) <= 1 && <div className="text-xs text-ink-faint">{tr("Других версий пока нет.", "No other versions yet.")}</div>}
            </div>
          </Section>
        </>
      )}
    </div>
  );
}
