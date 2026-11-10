import { open, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { FileDown, FileUp, Globe, Network, Save, ShieldOff, Undo2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { manualIpState } from "../../catalog/apps";
import { ListKey, MANUAL_LISTS } from "../../catalog/lists";
import { Button, Card } from "../../components/ui";
import { api, errorText } from "../../services/api";
import type { Settings } from "../../services/types";
import { useApp, useCount, useTr } from "../../stores/app";
import { IpStateNote } from "./packs";

const toText = (a: string[]) => a.join("\n");
const toLines = (s: string) =>
  s
    .split("\n")
    .map((x) => x.trim())
    .filter(Boolean);
const entries = (s: string) => toLines(s).filter((l) => !l.startsWith("#")).length;

const LIST_ICON: Record<ListKey, JSX.Element> = {
  general: <Globe className="h-4 w-4" />,
  ipsetInclude: <Network className="h-4 w-4" />,
  exclude: <ShieldOff className="h-4 w-4" />,
  ipsetExclude: <ShieldOff className="h-4 w-4" />,
};

export function ManualLists({ settings, saving, save }: { settings: Settings; saving: boolean; save: (patch: Partial<Settings>, ok: string) => Promise<boolean> }) {
  const tr = useTr();
  const count = useCount();
  const { toast } = useApp();
  const fromSettings = (s: Settings): Record<ListKey, string> => ({
    general: toText(s.userLists.general),
    exclude: toText(s.userLists.exclude),
    ipsetExclude: toText(s.userLists.ipsetExclude),
    ipsetInclude: toText(s.userLists.ipsetInclude ?? []),
  });
  const [lists, setLists] = useState<Record<ListKey, string>>(fromSettings(settings));
  const [tab, setTab] = useState<ListKey>("general");
  useEffect(() => setLists(fromSettings(settings)), [settings]);
  const dirty = useMemo(() => (Object.keys(lists) as ListKey[]).some((k) => toText(toLines(lists[k])) !== toText(settings.userLists[k] ?? [])), [lists, settings]);

  const { lang } = useApp();
  const meta = Object.fromEntries(
    MANUAL_LISTS.map((l) => [l.key, { title: lang === "en" ? l.titleEn : l.titleRu, text: lang === "en" ? l.textEn : l.textRu, icon: LIST_ICON[l.key], placeholder: l.placeholder, kind: l.kind }]),
  ) as Record<ListKey, { title: string; text: string; icon: JSX.Element; placeholder: string; kind: "domain" | "ip" }>;
  const ipStatus = useApp((s) => s.overview?.ipSets);

  const importFile = async () => {
    const path = await open({ multiple: false, filters: [{ name: tr("Текстовый список", "Text list"), extensions: ["txt", "list"] }] });
    if (!path || Array.isArray(path)) return;
    try {
      const add = await api.readListFile(path);
      const have = new Set(toLines(lists[tab]).map((x) => x.toLowerCase()));
      const fresh = add.filter((x) => !have.has(x.toLowerCase()));
      setLists({ ...lists, [tab]: toText([...toLines(lists[tab]), ...fresh]) });
      toast("info", tr(`Добавлено строк: ${fresh.length}. Проверьте и сохраните.`, `${fresh.length} lines added. Review and save.`));
    } catch (e) {
      toast("error", errorText(e));
    }
  };
  const exportFile = async () => {
    const path = await saveDialog({ defaultPath: `discord-bridge-${tab}.txt`, filters: [{ name: "TXT", extensions: ["txt"] }] });
    if (!path) return;
    try {
      await api.writeListFile(path, toLines(lists[tab]));
      toast("ok", tr("Список сохранён в файл", "List saved to file"));
    } catch (e) {
      toast("error", errorText(e));
    }
  };

  const m = meta[tab];
  return (
    <Card className="overflow-hidden">
      <div className="grid grid-cols-[220px_1fr] narrow:grid-cols-1">
        <div role="tablist" aria-label={tr("Списки", "Lists")} className="space-y-1 border-r border-line p-2 narrow:flex narrow:gap-1 narrow:space-y-0 narrow:border-b narrow:border-r-0">
          {MANUAL_LISTS.map(({ key: k }) => (
            <button
              key={k}
              role="tab"
              aria-selected={tab === k}
              onClick={() => setTab(k)}
              className={`flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left transition-colors narrow:w-auto narrow:flex-1 ${tab === k ? "bg-surface-3 text-ink" : "text-ink-muted hover:bg-surface-2 hover:text-ink"}`}
            >
              <span className={tab === k ? "text-accent" : ""}>{meta[k].icon}</span>
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[13px] font-medium">{meta[k].title}</span>
                <span className="block text-[11px] text-ink-faint">{count(entries(lists[k]), ["запись", "записи", "записей"], ["entry", "entries"])}</span>
              </span>
            </button>
          ))}
        </div>
        <div className="flex min-w-0 flex-col p-4">
          <div className="mb-2 flex items-start justify-between gap-3">
            <p className="text-xs leading-relaxed text-ink-muted">{m.text}</p>
            <div className="flex shrink-0 gap-1">
              <Button size="sm" variant="ghost" icon={<FileDown className="h-3.5 w-3.5" />} onClick={importFile}>
                {tr("Импорт", "Import")}
              </Button>
              <Button size="sm" variant="ghost" icon={<FileUp className="h-3.5 w-3.5" />} onClick={exportFile}>
                {tr("Экспорт", "Export")}
              </Button>
            </div>
          </div>
          {tab === "ipsetInclude" && <IpStateNote state={ipStatus ? manualIpState(entries(toText(settings.userLists.ipsetInclude ?? [])), ipStatus) : null} ip={ipStatus} subject="list" />}
          {tab === "ipsetInclude" && dirty && <p className="mb-2 text-[11px] text-ink-faint">{tr("Статус выше — для сохранённого списка.", "The status above is for the saved list.")}</p>}
          <textarea className="field h-64 resize-none font-mono text-xs leading-5" value={lists[tab]} placeholder={m.placeholder} onChange={(e) => setLists({ ...lists, [tab]: e.target.value })} spellCheck={false} aria-label={m.title} />
          <p className="mt-1.5 text-[11px] text-ink-faint">{m.kind === "ip" ? tr("По одному IP или подсети в строке (IPv4 и IPv6, маска через /), # — комментарий.", "One IP or subnet per line (IPv4 and IPv6, mask after /), # for comments.") : tr("По одному домену в строке, # — комментарий. Домен включает свои поддомены.", "One domain per line, # for comments. A domain includes its subdomains.")}</p>
        </div>
      </div>
      <div className="flex items-center justify-between gap-3 border-t border-line bg-surface-2/40 px-5 py-2.5">
        <span className="text-xs text-ink-muted">{dirty ? tr("Есть несохранённые изменения", "Unsaved changes") : tr("Все изменения сохранены", "All changes saved")}</span>
        <div className="flex gap-2">
          <Button size="sm" variant="ghost" icon={<Undo2 className="h-3.5 w-3.5" />} disabled={!dirty} onClick={() => setLists(fromSettings(settings))}>
            {tr("Отменить", "Discard")}
          </Button>
          <Button size="sm" variant="primary" icon={<Save className="h-3.5 w-3.5" />} disabled={!dirty} loading={saving} onClick={() => save({ userLists: { general: toLines(lists.general), exclude: toLines(lists.exclude), ipsetExclude: toLines(lists.ipsetExclude), ipsetInclude: toLines(lists.ipsetInclude) } }, tr("Списки сохранены", "Lists saved"))}>
            {tr("Сохранить", "Save")}
          </Button>
        </div>
      </div>
    </Card>
  );
}
