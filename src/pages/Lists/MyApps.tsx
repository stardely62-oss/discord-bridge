import { Globe } from "lucide-react";
import { useState } from "react";
import { installedTitle, loc, ownEntries } from "../../catalog/apps";
import { Button, Card } from "../../components/ui";
import type { CatalogView } from "../../services/types";
import { useApp, useCount, useTr } from "../../stores/app";
import { AppIcon, PackDetails, StateChip, UpdateDialog, counts, usePackActions, versionText } from "./packs";

export function MyApps({ view, manual, toManual }: { view: CatalogView; manual: string[]; toManual: () => void }) {
  const tr = useTr();
  const count = useCount();
  const { lang } = useApp();
  const actions = usePackActions();
  const [details, setDetails] = useState<string | null>(null);
  const [updating, setUpdating] = useState<string | null>(null);
  const mine = view.packs.filter((v) => v.installed || v.state === "flowseal");
  const own = ownEntries(manual);
  const find = (id: string | null) => view.packs.find((v) => v.pack.id === id) ?? null;
  return (
    <div className="space-y-3">
      <Card className="divide-y divide-line">
        {mine.map((v) => {
          const name = loc(v.pack.name, lang);
          return (
            <div key={v.pack.id} className="flex items-center gap-3 px-4 py-3">
              <AppIcon name={name} icon={v.pack.icon} size="h-8 w-8 text-sm" />
              <button className="min-w-0 flex-1 text-left" onClick={() => setDetails(v.pack.id)}>
                <div className="text-[13px] font-medium hover:underline">{name}</div>
                <div className="truncate text-xs text-ink-faint">
                  {v.state === "flowseal" ? tr(`входит в Flowseal — ${v.pack.flowsealList}`, `part of Flowseal — ${v.pack.flowsealList}`) : `${tr("версия", "version")} ${versionText(v.installed?.version, tr)} · ${counts(count, v.installed?.domains.length ?? 0, v.installed?.ips?.length ?? 0)}`}
                </div>
              </button>
              <StateChip state={v.state} />
              {(v.state === "updateAvailable" || v.state === "differs") && (
                <Button size="sm" variant="primary" onClick={() => setUpdating(v.pack.id)}>
                  {tr("Обновить…", "Update…")}
                </Button>
              )}
            </div>
          );
        })}
        {view.orphans.map((o) => {
          const name = installedTitle(o, lang);
          return (
            <div key={o.id} className="flex items-center gap-3 px-4 py-3">
              <AppIcon name={name} size="h-8 w-8 text-sm" />
              <div className="min-w-0 flex-1">
                <div className="text-[13px] font-medium">{name}</div>
                <div className="text-xs text-ink-faint">
                  {tr("Источник больше не публикуется. Локальная копия продолжает работать — её можно оставить или удалить.", "The source is no longer published. The local copy keeps working — keep it or remove it.")} {counts(count, o.domains.length, o.ips?.length ?? 0)}
                </div>
              </div>
              <StateChip state="orphan" />
              <Button size="sm" variant="ghost" disabled={actions.busy} onClick={() => actions.remove(o.id, name)}>
                {tr("Удалить", "Remove")}
              </Button>
            </div>
          );
        })}
        <div className="flex items-center gap-3 px-4 py-3">
          <span className="flex h-8 w-8 items-center justify-center rounded-lg bg-surface-3 text-ink-muted">
            <Globe className="h-4 w-4" />
          </span>
          <div className="min-w-0 flex-1">
            <div className="text-[13px] font-medium">{tr("Мои адреса", "My entries")}</div>
            <div className="truncate text-xs text-ink-faint">{own.length ? own.slice(0, 4).join(", ") + (own.length > 4 ? "…" : "") : tr("пока нет", "none yet")}</div>
          </div>
          <span className="text-xs text-ink-muted">{count(own.length, ["адрес", "адреса", "адресов"], ["entry", "entries"])}</span>
          <Button size="sm" variant="ghost" onClick={toManual}>
            {tr("Изменить", "Edit")}
          </Button>
        </div>
      </Card>
      <PackDetails
        view={find(details)}
        onClose={() => setDetails(null)}
        onUpdate={(v) => {
          setDetails(null);
          setUpdating(v.pack.id);
        }}
      />
      <UpdateDialog view={find(updating)} onClose={() => setUpdating(null)} />
    </div>
  );
}
