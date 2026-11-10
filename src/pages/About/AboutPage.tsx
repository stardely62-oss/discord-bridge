import { ExternalLink, RefreshCw } from "lucide-react";
import { useState } from "react";
import { Button, Card, PageHeader } from "../../components/ui";
import { api, errorText } from "../../services/api";
import { useApp, useTr } from "../../stores/app";
import { useAppUpdate } from "../../updates/install";
import { appReleaseTarget } from "../../updates/notices";

export default function AboutPage() {
  const { overview, updates, setUpdates, toast } = useApp();
  const tr = useTr();
  const appUpdate = useAppUpdate();
  const upd = updates?.app ?? null;
  const [checking, setChecking] = useState(false);
  const links: [string, string, string][] = [
    ["repo", "Bridge", tr("Исходный код, MIT", "Source code, MIT")],
    ["flowseal", "Flowseal zapret-discord-youtube", tr("Стратегии и ресурсы, MIT", "Strategies and resources, MIT")],
    ["zapret", "bol-van/zapret", tr("Движок winws, MIT", "winws engine, MIT")],
    ["windivert", "WinDivert", tr("Драйвер перехвата пакетов, LGPLv3 / GPLv2", "Packet capture driver, LGPLv3 / GPLv2")],
  ];
  const check = async () => {
    setChecking(true);
    try {
      await api.appCheckUpdate();
      setUpdates(await api.updatesGet());
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setChecking(false);
    }
  };
  return (
    <div className="space-y-4">
      <PageHeader title={tr("О программе", "About")} />
      <Card className="flex items-center gap-5 p-6">
        <img src="/app-icon.png" alt="" className="h-16 w-16 rounded-lg" />
        <div className="flex-1">
          <div className="text-xl font-semibold">Bridge</div>
          <div className="text-sm text-ink-muted">
            {tr("Версия", "Version")} {overview?.appVersion} · Flowseal {overview?.bundle.version ?? "—"} · winws {overview?.bundle.winwsVersion ?? "—"} · {tr("служба", "service")} {overview?.helper.version ?? "—"}
          </div>
          {upd && (
            <div className="mt-1 text-xs">
              {upd.status === "error" ? <span className="text-warn">{tr("Проверить обновления не удалось", "Update check failed")}: {upd.error}</span> : upd.status === "available" ? <span className="text-accent">{tr(`Доступна версия ${upd.latest}`, `Version ${upd.latest} available`)}</span> : <span className="text-ok">{tr("Установлена последняя версия", "Up to date")}</span>}
            </div>
          )}
        </div>
        <div className="flex gap-2">
          <Button size="sm" icon={<RefreshCw className="h-3.5 w-3.5" />} loading={checking} onClick={check}>
            {tr("Проверить обновления", "Check for updates")}
          </Button>
          {appUpdate.canInstall && (
            <Button size="sm" variant="primary" loading={appUpdate.installing} onClick={appUpdate.install}>
              {tr("Установить", "Install")}
            </Button>
          )}
          {appReleaseTarget(upd) && (
            <Button size="sm" variant={appUpdate.canInstall ? "secondary" : "primary"} onClick={() => api.openExternal(appReleaseTarget(upd)!).catch((e) => toast("error", errorText(e)))}>
              {tr("Открыть релиз", "Open release")}
            </Button>
          )}
        </div>
      </Card>
      <Card className="divide-y divide-line">
        {links.map(([id, title, sub]) => (
          <button key={id} onClick={() => api.openExternal(id)} className="flex w-full items-center justify-between px-5 py-3 text-left transition-colors hover:bg-surface-2">
            <div>
              <div className="text-sm">{title}</div>
              <div className="text-xs text-ink-muted">{sub}</div>
            </div>
            <ExternalLink className="h-4 w-4 text-ink-faint" />
          </button>
        ))}
      </Card>
      <div className="flex justify-end">
        <Button size="sm" variant="ghost" icon={<ExternalLink className="h-3.5 w-3.5" />} onClick={() => api.openExternal("author-projects").catch((e) => toast("error", errorText(e)))}>
          {tr("Другие проекты автора", "More projects by the author")}
        </Button>
      </div>
      <Card className="space-y-2 p-5 text-xs leading-relaxed text-ink-muted">
        <p>
          {tr(
            "Bridge не является официальным продуктом Discord Inc. и не связан с ней. Приложение запускает немодифицированный официальный клиент Discord и не читает сообщения, токены или учётные данные.",
            "Bridge is not an official Discord Inc. product and is not affiliated with it. It launches the unmodified official Discord client and never reads messages, tokens or credentials.",
          )}
        </p>
        <p>
          {tr(
            "В комплект входят оригинальные файлы Flowseal (MIT), winws из проекта zapret (MIT, © bol-van), WinDivert (LGPLv3/GPLv2, © basil00) и cygwin1.dll (LGPLv3). Полные тексты лицензий — в файле THIRD_PARTY_NOTICES.md в папке установки.",
            "Bundles original Flowseal files (MIT), winws from zapret (MIT, © bol-van), WinDivert (LGPLv3/GPLv2, © basil00) and cygwin1.dll (LGPLv3). Full license texts are in THIRD_PARTY_NOTICES.md in the install folder.",
          )}
        </p>
      </Card>
    </div>
  );
}
