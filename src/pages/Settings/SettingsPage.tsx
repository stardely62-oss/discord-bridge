import { open } from "@tauri-apps/plugin-dialog";
import { Download, FolderOpen, RefreshCw, RotateCcw } from "lucide-react";
import { ReactNode, useEffect, useState } from "react";
import { Badge, Button, Card, NumberField, PageHeader, PageSkeleton, Segmented, Select, SettingRow, Switch } from "../../components/ui";
import { confirmDialog } from "../../components/ui/confirm";
import { api, errorText } from "../../services/api";
import type { Settings, TcpInfo, UpdateInfo } from "../../services/types";
import { useApp, useTr } from "../../stores/app";

type Tab = "general" | "bypass" | "updates";

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Card className="px-5 py-1">
      <div className="label pb-1 pt-3 uppercase tracking-wide text-ink-faint">{title}</div>
      <div className="divide-y divide-line">{children}</div>
    </Card>
  );
}

export default function SettingsPage() {
  const { overview, toast, refresh, setLang } = useApp();
  const tr = useTr();
  const [tab, setTab] = useState<Tab>("general");
  const [s, setS] = useState<Settings | null>(null);
  const [saving, setSaving] = useState(false);
  const [fsUpdate, setFsUpdate] = useState<UpdateInfo | null>(null);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [tcp, setTcp] = useState<TcpInfo | null>(null);

  useEffect(() => {
    api.getSettings().then(setS).catch((e) => toast("error", errorText(e)));
  }, [toast]);

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

  const checkFlowseal = async () => {
    setChecking(true);
    try {
      setFsUpdate(await api.flowsealCheck());
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setChecking(false);
    }
  };

  const installFlowseal = async () => {
    if (!fsUpdate?.latest) return;
    const ok = await confirmDialog({
      title: tr(`Установить Flowseal ${fsUpdate.latest}?`, `Install Flowseal ${fsUpdate.latest}?`),
      body: tr("Перед установкой проверяются подпись манифеста и хэши всех файлов. Ваши стратегии не изменятся, а прежнюю версию можно будет вернуть.", "The manifest signature and all file hashes are verified first. Your strategies are not changed and the previous version can be restored."),
      confirmLabel: tr("Установить", "Install"),
    });
    if (!ok) return;
    setInstalling(true);
    try {
      toast("ok", await api.flowsealInstall(fsUpdate.latest));
      refresh();
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setInstalling(false);
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
      toast("ok", await api.flowsealSetActive(version));
    } catch (e) {
      toast("error", errorText(e));
    }
    refresh();
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
        <Section title={tr("Flowseal — стратегии и движок", "Flowseal — strategies & engine")}>
          <SettingRow
            title={tr(`Установлена версия ${overview?.bundle.version ?? "—"}`, `Installed ${overview?.bundle.version ?? "—"}`)}
            description={tr(`winws ${overview?.bundle.winwsVersion ?? "—"}. Ставятся только версии с подписанным манифестом, файлы берутся с официального GitHub Flowseal.`, `winws ${overview?.bundle.winwsVersion ?? "—"}. Only versions with a signed manifest are installed; files come from the official Flowseal GitHub.`)}
          >
            <Button size="sm" icon={<RefreshCw className="h-3.5 w-3.5" />} loading={checking} onClick={checkFlowseal}>
              {tr("Проверить", "Check")}
            </Button>
          </SettingRow>
          {fsUpdate && (
            <div className="space-y-2 py-3 text-xs">
              {fsUpdate.error ? (
                <div className="text-bad">{fsUpdate.error}</div>
              ) : fsUpdate.newer ? (
                <>
                  <div className="flex items-center gap-2">
                    <Badge tone="accent">{tr(`доступна ${fsUpdate.latest}`, `${fsUpdate.latest} available`)}</Badge>
                    {fsUpdate.installable ? (
                      <Button size="sm" variant="primary" icon={<Download className="h-3.5 w-3.5" />} loading={installing} onClick={installFlowseal}>
                        {tr("Установить", "Install")}
                      </Button>
                    ) : (
                      <span className="text-ink-muted">{fsUpdate.unavailableReason ?? tr("Установка недоступна.", "Installation unavailable.")}</span>
                    )}
                  </div>
                  {fsUpdate.changelog && <pre className="selectable max-h-48 overflow-auto whitespace-pre-wrap rounded-md border border-line bg-surface-2 p-3 font-sans text-ink-muted">{fsUpdate.changelog}</pre>}
                </>
              ) : (
                <div className="text-ok">{tr(`Установлена последняя версия (${fsUpdate.latest})`, `Up to date (${fsUpdate.latest})`)}</div>
              )}
            </div>
          )}
          {(overview?.bundle.bundles.length ?? 0) > 1 && (
            <div className="py-3">
              <div className="label mb-2">{tr("Установленные версии — можно вернуться к прежней", "Installed versions — you can go back")}</div>
              {overview?.bundle.bundles.map((b) => (
                <div key={b.version} className="flex items-center justify-between py-1 text-sm">
                  <span className="flex items-center gap-2">
                    {b.version} {b.builtin && <Badge>{tr("встроенная", "built-in")}</Badge>} {!b.ok && <Badge tone="bad">{tr("повреждена", "damaged")}</Badge>}
                  </span>
                  {b.active ? (
                    <Badge tone="ok">{tr("используется", "in use")}</Badge>
                  ) : (
                    <Button size="sm" variant="ghost" disabled={!b.ok} onClick={() => switchVersion(b.version)}>
                      {tr("Использовать", "Use")}
                    </Button>
                  )}
                </div>
              ))}
            </div>
          )}
        </Section>
      )}
    </div>
  );
}
