import { CheckCircle2, RefreshCw, Search, TriangleAlert, Users, WifiOff } from "lucide-react";
import { useEffect, useState } from "react";
import { CATEGORY_EN, CATEGORY_RU, filterPacks, loc, presentCategories, syncLine } from "../../catalog/apps";
import { Button, Card, Empty, Segmented } from "../../components/ui";
import { api, errorText } from "../../services/api";
import type { CatalogView, PackCategory } from "../../services/types";
import { useApp, useTr } from "../../stores/app";
import { ServiceCard, ServiceDetails } from "./ServiceCards";
import { AppIcon, PackCard, PackDetails, UpdateDialog } from "./packs";

export function SyncBar({ view }: { view: CatalogView }) {
  const { lang, setCatalog, toast } = useApp();
  const tr = useTr();
  const [busy, setBusy] = useState(false);
  const line = syncLine(view, lang);
  const refresh = async () => {
    setBusy(true);
    try {
      const v = await api.catalogRefresh();
      setCatalog(v);
      if (!v.error) toast("ok", v.serial !== view.serial ? tr("Каталог обновлён и проверен", "Catalog updated and verified") : tr("Новых версий каталога нет", "No newer catalog"));
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const tone = { ok: "text-ink-muted", warn: "text-warn", info: "text-ink-muted" }[line.tone];
  return (
    <div className="flex flex-wrap items-center gap-3 rounded-lg border border-line bg-surface px-4 py-2.5 text-xs">
      {view.offline ? <WifiOff className="h-4 w-4 shrink-0 text-warn" /> : line.tone === "warn" ? <TriangleAlert className="h-4 w-4 shrink-0 text-warn" /> : <CheckCircle2 className={`h-4 w-4 shrink-0 ${view.origin === "remote" ? "text-ok" : "text-ink-faint"}`} />}
      <span className={`min-w-0 flex-1 ${tone}`}>{line.text}</span>
      {view.repaired && <span className="text-warn">{tr("Повреждённый кеш заменён проверенной копией", "A damaged cache was replaced by the verified copy")}</span>}
      <Button size="sm" variant="ghost" icon={<RefreshCw className={`h-3.5 w-3.5 ${busy ? "animate-spin" : ""}`} />} disabled={busy} onClick={refresh}>
        {tr("Проверить обновления", "Check for updates")}
      </Button>
    </div>
  );
}

export function Catalog({ view }: { view: CatalogView }) {
  const tr = useTr();
  const { lang } = useApp();
  const [source, setSource] = useState<"official" | "community">("official");
  const [category, setCategory] = useState<PackCategory | "all">("all");
  const [q, setQ] = useState("");
  const [details, setDetails] = useState<string | null>(null);
  const [service, setService] = useState<string | null>(null);
  const [updating, setUpdating] = useState<string | null>(null);
  const services = useApp((st) => st.services);
  const loadServices = useApp((st) => st.loadServices);
  useEffect(() => {
    loadServices();
  }, [loadServices, view.serial]);
  const summary = (id: string) => services?.cards.find((c) => c.id === id);
  const nameOf = (id: string) => {
    const p = view.packs.find((v) => v.pack.id === id)?.pack;
    return p ? loc(p.name, lang) : id;
  };
  const items = filterPacks(view.packs, q, category, source);
  const cats = presentCategories(view.packs.filter((v) => v.pack.source === source));
  const find = (id: string | null) => view.packs.find((v) => v.pack.id === id) ?? null;
  return (
    <div className="space-y-3">
      <SyncBar view={view} />
      <div className="flex flex-wrap items-center gap-3">
        <Segmented<"official" | "community">
          ariaLabel={tr("Источник", "Source")}
          value={source}
          onChange={(v) => {
            setSource(v);
            setCategory("all");
          }}
          options={[
            { value: "official", label: tr("Официальные", "Official") },
            { value: "community", label: tr("Сообщество", "Community") },
          ]}
        />
        <label className="relative min-w-[200px] flex-1">
          <Search className="pointer-events-none absolute left-2.5 top-2 h-4 w-4 text-ink-faint" />
          <input className="field h-8 py-1 pl-8" placeholder={tr("Поиск: название или домен", "Search: name or domain")} value={q} onChange={(e) => setQ(e.target.value)} aria-label={tr("Поиск", "Search")} />
        </label>
      </div>
      {cats.length > 1 && (
        <div className="flex flex-wrap gap-1.5" role="group" aria-label={tr("Категории", "Categories")}>
          {(["all", ...cats] as (PackCategory | "all")[]).map((c) => (
            <button
              key={c}
              aria-pressed={category === c}
              onClick={() => setCategory(c)}
              className={`rounded-full border px-2.5 py-0.5 text-xs transition-colors ${category === c ? "border-accent/50 bg-accent-soft text-accent" : "border-line text-ink-muted hover:text-ink"}`}
            >
              {c === "all" ? tr("Все", "All") : lang === "en" ? CATEGORY_EN[c] : CATEGORY_RU[c]}
            </button>
          ))}
        </div>
      )}
      {items.length === 0 ? (
        <Card className="border-dashed">
          {source === "community" && !q ? (
            <Empty
              icon={<Users className="h-8 w-8" />}
              title={tr("Наборов сообщества пока нет", "No community sets yet")}
              text={tr("Наборы сообщества публикуются в том же подписанном каталоге после ревью. Пока опубликованы только официальные наборы.", "Community sets are published in the same signed catalog after review. Only official sets are published so far.")}
            />
          ) : (
            <Empty icon={<Search className="h-8 w-8" />} title={tr("Ничего не найдено", "Nothing found")} />
          )}
        </Card>
      ) : (
        <div className="grid grid-cols-2 gap-3 xl:grid-cols-3 narrow:grid-cols-1">
          {items.map((v) =>
            v.state === "flowseal" ? (
              <ServiceCard key={v.pack.id} pack={v.pack} icon={<AppIcon name={loc(v.pack.name, lang)} icon={v.pack.icon} />} summary={summary(v.pack.id)} onOpen={() => setService(v.pack.id)} />
            ) : (
              <PackCard key={v.pack.id} view={v} onOpen={() => setDetails(v.pack.id)} onUpdate={() => setUpdating(v.pack.id)} />
            ),
          )}
        </div>
      )}
      <PackDetails
        view={find(details)}
        onClose={() => setDetails(null)}
        onUpdate={(v) => {
          setDetails(null);
          setUpdating(v.pack.id);
        }}
      />
      <UpdateDialog view={find(updating)} onClose={() => setUpdating(null)} />
      <ServiceDetails id={service} pack={find(service)?.pack ?? null} names={nameOf} onClose={() => setService(null)} />
    </div>
  );
}
