import { FileDown, FileCode2, Layers, Lock, ShieldCheck, Users } from "lucide-react";
import { ReactNode, useEffect, useState } from "react";
import { ImportDialog, PromptDialog } from "../../components/strategies/Dialogs";
import { inTab, StrategyList, StrategyTab } from "../../components/strategies/StrategyList";
import { StrategyView } from "../../components/strategies/StrategyView";
import { Button, Card, Empty } from "../../components/ui";
import { api, errorText } from "../../services/api";
import type { OptionSpec } from "../../services/types";
import { useApp, useTr } from "../../stores/app";

function Point({ icon, title, text }: { icon: ReactNode; title: string; text: string }) {
  return (
    <div className="flex gap-3">
      <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-surface-3 text-ink-muted">{icon}</span>
      <div>
        <div className="text-[13px] font-medium">{title}</div>
        <div className="text-xs leading-relaxed text-ink-muted">{text}</div>
      </div>
    </div>
  );
}

function ImportPane({ onImport }: { onImport: () => void }) {
  const tr = useTr();
  return (
    <div className="mx-auto w-full max-w-2xl space-y-4 p-6">
      <div>
        <h2 className="text-lg font-semibold">{tr("Импорт стратегии", "Import a strategy")}</h2>
        <p className="mt-1 text-[13px] text-ink-muted">{tr("Добавьте стратегию из файла — например, BAT из сборки Flowseal или JSON, которым с вами поделились.", "Add a strategy from a file — e.g. a Flowseal BAT or a JSON someone shared.")}</p>
      </div>
      <Card className="flex flex-col items-center gap-3 border-dashed px-6 py-8 text-center">
        <span className="flex h-12 w-12 items-center justify-center rounded-xl bg-accent-soft text-accent">
          <FileDown className="h-6 w-6" />
        </span>
        <div className="text-sm font-medium">{tr("Выберите файл BAT, CMD, JSON или TOML", "Choose a BAT, CMD, JSON or TOML file")}</div>
        <Button variant="primary" onClick={onImport}>
          {tr("Выбрать файл…", "Choose file…")}
        </Button>
        <div className="text-xs text-ink-faint">{tr("Перед сохранением вы увидите, что именно будет импортировано.", "You will see exactly what is imported before saving.")}</div>
      </Card>
      <div className="grid grid-cols-2 gap-4 narrow:grid-cols-1">
        <Point icon={<ShieldCheck className="h-4 w-4" />} title={tr("Файл не выполняется", "The file is never run")} text={tr("Команды из BAT только читаются: Discord Bridge берёт из них параметры winws.", "BAT commands are only read: Discord Bridge takes winws options from them.")} />
        <Point icon={<Lock className="h-4 w-4" />} title={tr("Только ресурсы Flowseal", "Only Flowseal resources")} text={tr("Пути к файлам вне ресурсов Flowseal отклоняются.", "Paths outside Flowseal resources are rejected.")} />
        <Point icon={<FileCode2 className="h-4 w-4" />} title={tr("Проверка перед запуском", "Checked before running")} text={tr("Стратегию с ошибками можно сохранить как черновик и исправить в редакторе.", "A strategy with errors can be saved as a draft and fixed in the editor.")} />
        <Point icon={<Layers className="h-4 w-4" />} title={tr("Попадает в «Мои»", "Goes to “Mine”")} text={tr("После импорта стратегию можно редактировать, дублировать и экспортировать.", "After import you can edit, duplicate and export it.")} />
      </div>
    </div>
  );
}

function CommunityPane() {
  const tr = useTr();
  return (
    <div className="mx-auto w-full max-w-2xl space-y-4 p-6">
      <div>
        <h2 className="text-lg font-semibold">{tr("Стратегии сообщества", "Community strategies")}</h2>
        <p className="mt-1 text-[13px] text-ink-muted">{tr("Готовые стратегии от других пользователей — для разных провайдеров и регионов.", "Ready-made strategies from other users — for different ISPs and regions.")}</p>
      </div>
      <Card className="border-dashed">
        <Empty
          icon={<Users className="h-8 w-8" />}
          title={tr("Скоро", "Coming soon")}
          text={tr(
            "Каталог сообщества с GitHub ещё не подключён, поэтому здесь пусто — без выдуманных оценок и рейтингов. У каждой стратегии будут описание, источник, совместимая версия Flowseal и кнопка «Установить в Мои».",
            "The community catalog from GitHub is not connected yet, so this is empty — no made-up ratings. Each strategy will have a description, source, compatible Flowseal version and an “Install to Mine” button.",
          )}
        />
      </Card>
      <p className="text-xs text-ink-faint">{tr("Уже сейчас стратегию от другого человека можно добавить через вкладку «Импорт».", "You can already add someone's strategy via the Import tab.")}</p>
    </div>
  );
}

export default function StrategiesPage() {
  const { selectedStrategyId, selectStrategy, refreshStrategies, toast, strategies } = useApp();
  const tr = useTr();
  const [tab, setTab] = useState<StrategyTab>(() => (strategies.find((s) => s.id === selectedStrategyId)?.source ?? "flowseal") === "flowseal" ? "builtin" : "mine");
  const [specs, setSpecs] = useState<OptionSpec[]>([]);
  const [resources, setResources] = useState<{ fakes: string[]; lists: string[] }>({ fakes: [], lists: [] });
  const [creating, setCreating] = useState(false);
  const [importing, setImporting] = useState(false);

  useEffect(() => {
    api.optionSpecs().then(setSpecs).catch((e) => toast("error", errorText(e)));
    api.resources().then(setResources).catch(() => undefined);
  }, [toast, strategies.length]);

  // Switching tabs selects the first strategy of the tab...
  const changeTab = (t: StrategyTab) => {
    setTab(t);
    if (t !== "builtin" && t !== "mine") return;
    const cur = strategies.find((s) => s.id === selectedStrategyId);
    if (!cur || !inTab(cur, t)) {
      const first = strategies.find((s) => inTab(s, t));
      selectStrategy(first?.id ?? null);
    }
  };
  // ...while selecting a strategy elsewhere (copy, import, deletion) moves the tab to it.
  useEffect(() => {
    if (tab !== "builtin" && tab !== "mine") return;
    const cur = strategies.find((s) => s.id === selectedStrategyId);
    if (cur && !inTab(cur, tab)) setTab(cur.source === "flowseal" ? "builtin" : "mine");
    if (!cur) {
      const first = strategies.find((s) => inTab(s, tab));
      if (first) selectStrategy(first.id);
    }
  }, [tab, strategies, selectedStrategyId, selectStrategy]);

  const openMine = async (id: string) => {
    await refreshStrategies();
    setTab("mine");
    selectStrategy(id);
  };

  const create = async (name: string) => {
    setCreating(false);
    try {
      const s = await api.createStrategy(name);
      await openMine(s.id);
    } catch (e) {
      toast("error", errorText(e));
    }
  };

  const selected = strategies.find((s) => s.id === selectedStrategyId);
  const showView = (tab === "builtin" || tab === "mine") && selected && inTab(selected, tab);

  return (
    <div className="flex h-full">
      <StrategyList
        tab={tab}
        onTab={changeTab}
        onCreate={() => setCreating(true)}
        onImport={() => setImporting(true)}
        onDiscordOnly={async () => {
          try {
            const s = await api.createDiscordOnly();
            await openMine(s.id);
            toast("ok", tr("Создан профиль «Discord Only». Ограничение — по доменам и портам, не по процессу.", "«Discord Only» created. Limited by domains and ports, not by process."));
          } catch (e) {
            toast("error", errorText(e));
          }
        }}
      />
      <div className="flex min-w-0 flex-1 overflow-y-auto">
        {tab === "import" ? (
          <ImportPane onImport={() => setImporting(true)} />
        ) : tab === "community" ? (
          <CommunityPane />
        ) : showView ? (
          <StrategyView key={selectedStrategyId} id={selectedStrategyId!} specs={specs} resources={resources} />
        ) : (
          <div className="flex flex-1 items-center justify-center">
            <Empty icon={<Layers className="h-8 w-8" />} title={tr("Выберите стратегию", "Select a strategy")} text={tr("Встроенные — от Flowseal, «Мои» — созданные вами, копии и импорт.", "Built-in ones come from Flowseal; “Mine” are yours, copies and imports.")} />
          </div>
        )}
      </div>
      <PromptDialog open={creating} title={tr("Новая стратегия", "New strategy")} label={tr("Название", "Name")} initial={tr("Моя стратегия", "My strategy")} onClose={() => setCreating(false)} onSubmit={create} />
      <ImportDialog open={importing} onClose={() => setImporting(false)} onImported={openMine} />
    </div>
  );
}
