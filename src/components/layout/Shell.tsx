import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { Activity, AppWindow, CheckCircle2, Home, Info, Layers, Settings as SettingsIcon, X, XCircle } from "lucide-react";
import type { ReactNode } from "react";
import { Page, useApp, useTr } from "../../stores/app";
import { StatusDot } from "../ui";

export function Sidebar() {
  const { page, setPage, overview } = useApp();
  const tr = useTr();
  const running = overview?.engine?.state === "running";
  const failed = overview?.engine?.state === "failed";
  const main: { id: Page; label: string; icon: ReactNode }[] = [
    { id: "home", label: tr("Главная", "Home"), icon: <Home className="h-4 w-4" /> },
    { id: "strategies", label: tr("Стратегии", "Strategies"), icon: <Layers className="h-4 w-4" /> },
    { id: "lists", label: tr("Приложения", "Apps"), icon: <AppWindow className="h-4 w-4" /> },
    { id: "diagnostics", label: tr("Диагностика", "Diagnostics"), icon: <Activity className="h-4 w-4" /> },
  ];
  const bottom: { id: Page; label: string; icon: ReactNode }[] = [
    { id: "settings", label: tr("Настройки", "Settings"), icon: <SettingsIcon className="h-4 w-4" /> },
    { id: "about", label: tr("О программе", "About"), icon: <Info className="h-4 w-4" /> },
  ];
  const NavList = ({ items }: { items: typeof main }) => (
    <ul className="flex flex-col gap-px">
      {items.map((it) => (
        <li key={it.id}>
          <button
            onClick={() => setPage(it.id)}
            aria-current={page === it.id ? "page" : undefined}
            title={it.label}
            className={`relative flex h-8 w-full items-center gap-2.5 rounded-md px-2.5 text-[13px] transition-colors narrow:justify-center narrow:px-0 ${
              page === it.id ? "bg-surface-3 font-medium text-ink" : "text-ink-muted hover:bg-surface-2 hover:text-ink"
            }`}
          >
            {page === it.id && <span className="absolute left-0 top-1.5 h-5 w-[3px] rounded-r bg-accent narrow:top-1.5" aria-hidden />}
            {it.icon}
            <span className="narrow:hidden">{it.label}</span>
          </button>
        </li>
      ))}
    </ul>
  );
  return (
    <nav aria-label={tr("Разделы", "Sections")} className="flex w-52 shrink-0 flex-col border-r border-line bg-rail px-2.5 py-3 narrow:w-14 narrow:px-2">
      <div className="mb-4 flex items-center gap-2.5 px-1.5 py-1 narrow:justify-center narrow:px-0">
        <span className="relative h-8 w-8 shrink-0" aria-hidden>
          <img src="/brand-mark-dark.svg" alt="" className="brand-on-dark h-8 w-8" />
          <img src="/brand-mark-light.svg" alt="" className="brand-on-light h-8 w-8" />
        </span>
        <div className="narrow:hidden">
          <div className="text-[13px] font-semibold leading-tight tracking-[-0.01em]">
            Discord <span className="text-accent">Bridge</span>
          </div>
          <div className="text-[11px] text-ink-faint">{overview?.appVersion ?? ""}</div>
        </div>
      </div>
      <NavList items={main} />
      <div className="mt-auto space-y-2">
        <NavList items={bottom} />
      <div className="flex items-center gap-2 rounded-md border border-line bg-surface px-2.5 py-2 text-xs narrow:justify-center narrow:border-0 narrow:bg-transparent narrow:px-0" title={running ? overview?.engine?.strategyName ?? "" : ""}>
        <StatusDot status={running ? "running" : failed ? "error" : "stopped"} />
        <div className="min-w-0 narrow:hidden">
          <div className="font-medium">
            {running ? tr("Обход включён", "Bypass on") : failed ? tr("Ошибка Zapret", "Zapret error") : overview && !overview.helper.available ? tr("Служба недоступна", "Service unavailable") : tr("Обход выключен", "Bypass off")}
          </div>
          {running && <div className="truncate text-[11px] text-ink-faint">{overview?.engine?.strategyName}</div>}
        </div>
      </div>
      </div>
    </nav>
  );
}

export function Toasts() {
  const { toasts, dismiss } = useApp();
  const reduce = useReducedMotion();
  return (
    <div aria-live="polite" className="pointer-events-none fixed right-4 top-4 z-50 flex w-96 max-w-[calc(100vw-2rem)] flex-col gap-2">
      <AnimatePresence>
        {toasts.map((t) => (
          <motion.div
            key={t.id}
            initial={reduce ? false : { opacity: 0, y: -8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0 }}
            className="pointer-events-auto flex items-start gap-3 rounded-md border border-line-strong bg-surface px-3.5 py-2.5 text-[13px] shadow-pop"
          >
            {t.kind === "ok" ? <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0 text-ok" /> : t.kind === "error" ? <XCircle className="mt-0.5 h-4 w-4 shrink-0 text-bad" /> : <Info className="mt-0.5 h-4 w-4 shrink-0 text-accent" />}
            <div className="selectable min-w-0 flex-1 break-words">{t.text}</div>
            <button aria-label="Закрыть" onClick={() => dismiss(t.id)} className="text-ink-faint hover:text-ink">
              <X className="h-4 w-4" />
            </button>
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  );
}
