import { AlertTriangle, ShieldAlert } from "lucide-react";
import { useActivate } from "../../hooks/useActivate";
import { useApp, useTr } from "../../stores/app";
import { Button, Modal } from "../ui";

/** Explains why a strategy could not be activated and offers safe next steps. */
export function ActivationDialog() {
  const { lastActivation: r, setActivation, overview, setPage } = useApp();
  const activate = useActivate();
  const tr = useTr();
  if (!r) return null;
  const close = () => setActivation(null);
  const strategyId = overview?.settings.activeStrategyId ?? null;
  const title =
    r.outcome === "conflicts"
      ? tr("Обнаружен другой обход DPI", "Another DPI bypass detected")
      : r.outcome === "invalid"
        ? tr("Стратегия содержит ошибки", "Strategy has errors")
        : r.outcome === "helperUnavailable"
          ? tr("Служба недоступна", "Service unavailable")
          : r.outcome === "rolledBack"
            ? tr("Новая стратегия не запустилась — прежняя восстановлена", "New strategy failed — previous one restored")
            : r.outcome === "prepareFailed"
              ? tr("Стратегию не удалось подготовить — работающий обход не тронут", "Could not prepare strategy — running bypass untouched")
              : r.outcome === "rollbackFailed"
                ? tr("Не удалось запустить и восстановить прежнюю конфигурацию", "Start and restore both failed")
                : tr("Не удалось запустить Zapret", "Failed to start Zapret");
  return (
    <Modal
      open
      onClose={close}
      title={title}
      footer={
        r.outcome === "conflicts" ? (
          <>
            <Button variant="ghost" onClick={close}>
              {tr("Отмена", "Cancel")}
            </Button>
            <Button
              variant="danger"
              onClick={() => {
                close();
                activate(r.requestedId ?? strategyId, true);
              }}
            >
              {tr("Запустить всё равно", "Start anyway")}
            </Button>
          </>
        ) : r.outcome === "invalid" ? (
          <Button
            variant="primary"
            onClick={() => {
              close();
              setPage("strategies");
            }}
          >
            {tr("Открыть редактор", "Open editor")}
          </Button>
        ) : (
          <Button variant="primary" onClick={close}>
            {tr("Понятно", "OK")}
          </Button>
        )
      }
    >
      <div className="space-y-3 text-sm">
        {r.message && (
          <div className="flex gap-2 rounded-md border border-bad/30 bg-bad-soft px-3 py-2 text-bad">
            <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" />
            <span className="selectable break-words">{r.message}</span>
          </div>
        )}
        {r.outcome === "conflicts" && (
          <>
            <p className="text-ink-muted">
              {tr(
                "Bridge не останавливает чужие процессы и службы. Два движка одновременно обрабатывают одни и те же пакеты, из-за чего соединения обычно ломаются. Рекомендуется сначала остановить другой обход (например, у Flowseal: service.bat → Remove Services).",
                "Bridge never stops foreign processes or services. Two engines processing the same packets usually break connections. Stop the other bypass first (e.g. Flowseal service.bat → Remove Services).",
              )}
            </p>
            <ul className="space-y-1.5">
              {r.conflicts.map((c, i) => (
                <li key={i} className="rounded-lg border border-line bg-surface-2 px-3 py-2">
                  <div className="font-medium">
                    {c.kind === "service" ? tr("Служба", "Service") : tr("Процесс", "Process")} {c.name}
                    {c.pid ? ` · PID ${c.pid}` : ""}
                  </div>
                  <div className="text-xs text-ink-muted">{c.detail}</div>
                </li>
              ))}
            </ul>
          </>
        )}
        {r.issues.length > 0 && (
          <ul className="max-h-60 space-y-1 overflow-auto">
            {r.issues.map((i, n) => (
              <li key={n} className={`rounded-lg px-3 py-1.5 text-xs ${i.severity === "error" ? "bg-bad-soft text-bad" : "bg-warn-soft text-warn"}`}>
                <span className="font-mono opacity-70">{i.path}</span> — {i.message}
              </li>
            ))}
          </ul>
        )}
        {r.rolledBackTo && (
          <div className="flex gap-2 rounded-md border border-warn/30 bg-warn-soft px-3 py-2 text-warn">
            <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
            {tr(`Работает прежняя конфигурация (точно та, что была запущена): ${r.rolledBackTo}`, `The exact previous configuration runs: ${r.rolledBackTo}`)}
          </div>
        )}
        {r.rollbackError && (
          <div className="rounded-md border border-bad/30 bg-bad-soft px-3 py-2 text-xs text-bad">
            {tr("Восстановление тоже не удалось: ", "Restore failed too: ")}
            {r.rollbackError}. {tr("Обход DPI сейчас выключен.", "DPI bypass is now off.")}
          </div>
        )}
        {r.outcome === "helperUnavailable" && (
          <p className="text-ink-muted">
            {tr(
              "Обход DPI выполняет системная служба «Discord Bridge Helper», которую регистрирует установщик. Проверьте её в services.msc или переустановите приложение.",
              "DPI bypass is run by the \"Discord Bridge Helper\" system service registered by the installer. Check it in services.msc or reinstall the app.",
            )}
          </p>
        )}
      </div>
    </Modal>
  );
}
