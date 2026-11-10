import { CloudDownload } from "lucide-react";
import { useEffect, useState } from "react";
import { PageHeader, PageSkeleton, Segmented } from "../../components/ui";
import { api, errorText } from "../../services/api";
import type { Settings } from "../../services/types";
import { useApp, useTr } from "../../stores/app";
import { ApplyBanner } from "./packs";
import { Catalog } from "./Catalog";
import { MyApps } from "./MyApps";
import { ManualLists } from "./ManualLists";

type Tab = "catalog" | "mine" | "manual";

/** Settings for the manual lists. Saving always starts from fresh settings
 * and changes only the given fields, so pack operations done meanwhile
 * (installedPacks, via Rust) are never overwritten. */
function useListsState() {
  const { toast, refresh } = useApp();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    api.getSettings().then(setSettings).catch((e) => toast("error", errorText(e)));
  }, [toast]);
  const saveSettings = async (patch: Partial<Settings>, okText: string) => {
    setSaving(true);
    try {
      const fresh = await api.getSettings();
      const saved = await api.saveSettings({ ...fresh, ...patch });
      setSettings(saved);
      await refresh();
      toast("ok", okText);
      return true;
    } catch (e) {
      toast("error", errorText(e));
      return false;
    } finally {
      setSaving(false);
    }
  };
  return { settings, saving, saveSettings };
}

export default function ListsPage() {
  const tr = useTr();
  const [tab, setTab] = useState<Tab>("catalog");
  const { settings, saving, saveSettings } = useListsState();
  const catalog = useApp((s) => s.catalog);
  if (!settings || !catalog) return <PageSkeleton rows={3} />;
  const mineCount = catalog.packs.filter((v) => v.installed || v.state === "flowseal").length + catalog.orphans.length;
  const updates = catalog.packs.filter((v) => v.state === "updateAvailable").length;
  return (
    <div className="space-y-4">
      <PageHeader
        title={tr("Приложения и списки", "Apps & lists")}
        subtitle={tr("Для каких игр, сервисов и сайтов работает обход.", "Which games, services and sites the bypass covers.")}
        actions={
          <Segmented<Tab>
            ariaLabel={tr("Разделы", "Sections")}
            value={tab}
            onChange={setTab}
            options={[
              { value: "catalog", label: tr("Каталог", "Catalog"), icon: <CloudDownload className="h-3.5 w-3.5" /> },
              { value: "mine", label: updates ? tr(`Мои · ${mineCount} · ↑${updates}`, `Mine · ${mineCount} · ↑${updates}`) : tr(`Мои · ${mineCount}`, `Mine · ${mineCount}`) },
              { value: "manual", label: tr("Ручные списки", "Manual lists") },
            ]}
          />
        }
      />
      <ApplyBanner />
      {tab === "catalog" && <Catalog view={catalog} />}
      {tab === "mine" && <MyApps view={catalog} manual={settings.userLists.general} toManual={() => setTab("manual")} />}
      {tab === "manual" && <ManualLists settings={settings} saving={saving} save={saveSettings} />}
    </div>
  );
}
