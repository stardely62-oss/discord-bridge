import { TriangleAlert } from "lucide-react";
import { create } from "zustand";
import { useTr } from "../../stores/app";
import { Button, Modal } from "./index";

export interface ConfirmOptions {
  title: string;
  body?: React.ReactNode;
  /** Shell command or other exact text shown in a code box. */
  code?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
}

interface ConfirmStore {
  req: (ConfirmOptions & { resolve: (v: boolean) => void }) | null;
  set: (r: ConfirmStore["req"]) => void;
}

const useConfirmStore = create<ConfirmStore>((set) => ({ req: null, set: (req) => set({ req }) }));

/** In-app replacement for `window.confirm`: resolves to true if confirmed. */
export function confirmDialog(o: ConfirmOptions): Promise<boolean> {
  return new Promise((resolve) => {
    const prev = useConfirmStore.getState().req;
    prev?.resolve(false);
    useConfirmStore.getState().set({ ...o, resolve });
  });
}

export function ConfirmHost() {
  const { req, set } = useConfirmStore();
  const tr = useTr();
  const done = (v: boolean) => {
    req?.resolve(v);
    set(null);
  };
  return (
    <Modal
      open={!!req}
      onClose={() => done(false)}
      title={req?.title ?? ""}
      footer={
        <>
          <Button variant="ghost" onClick={() => done(false)}>
            {req?.cancelLabel ?? tr("Отмена", "Cancel")}
          </Button>
          <Button variant={req?.danger ? "dangerSolid" : "primary"} autoFocus onClick={() => done(true)}>
            {req?.confirmLabel ?? tr("Продолжить", "Continue")}
          </Button>
        </>
      }
    >
      <div className="space-y-3 text-sm leading-relaxed text-ink-muted">
        {req?.danger && (
          <div className="flex items-center gap-2 text-bad">
            <TriangleAlert className="h-4 w-4 shrink-0" />
            <span className="text-[13px] font-medium">{tr("Это действие нельзя отменить", "This cannot be undone")}</span>
          </div>
        )}
        {typeof req?.body === "string" ? <p className="whitespace-pre-line">{req.body}</p> : req?.body}
        {req?.code && <pre className="code-box selectable whitespace-pre-wrap break-all px-3 py-2 text-xs text-ink">{req.code}</pre>}
      </div>
    </Modal>
  );
}
