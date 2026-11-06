import { ShieldAlert } from "lucide-react";
import { useLaunch } from "../../hooks/useActivate";
import { useApp, useTr } from "../../stores/app";
import { Button, Modal } from "../ui";

/** Shown when «Launch Discord» could not start the bypass: the error is not hidden,
 * and launching without the bypass is an explicit user choice. */
export function LaunchDialog() {
  const { launchFailure: r, setLaunchFailure, setPage } = useApp();
  const launch = useLaunch();
  const tr = useTr();
  if (!r) return null;
  const close = () => setLaunchFailure(null);
  return (
    <Modal
      open
      onClose={close}
      title={tr("Обход DPI не запустился", "DPI bypass did not start")}
      footer={
        <>
          <Button
            variant="ghost"
            onClick={() => {
              close();
              setPage("diagnostics");
            }}
          >
            {tr("Диагностика", "Diagnostics")}
          </Button>
          <Button
            onClick={() => {
              close();
              launch("withoutBypass");
            }}
          >
            {tr("Запустить без обхода", "Launch without bypass")}
          </Button>
        </>
      }
    >
      <div className="flex gap-2 rounded-md border border-bad/30 bg-bad-soft px-3 py-2 text-sm text-bad">
        <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" />
        <span className="selectable break-words">{r.message}</span>
      </div>
      {r.bypass?.rolledBackTo && <p className="mt-3 text-xs text-ink-muted">{tr(`Продолжает работать прежняя конфигурация: ${r.bypass.rolledBackTo}`, `Previous configuration keeps running: ${r.bypass.rolledBackTo}`)}</p>}
    </Modal>
  );
}
