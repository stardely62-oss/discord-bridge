import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { Check, Info, Loader2, Minus, MoreHorizontal, Plus, X } from "lucide-react";
import { ReactNode, useEffect, useId, useRef, useState } from "react";
import type { CheckStatus } from "../../services/types";

type Variant = "primary" | "secondary" | "ghost" | "danger" | "dangerSolid";

export function Button({
  children,
  variant = "secondary",
  size = "md",
  icon,
  loading,
  className = "",
  ...rest
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: "sm" | "md" | "lg"; icon?: ReactNode; loading?: boolean }) {
  const base =
    "inline-flex shrink-0 items-center justify-center gap-1.5 whitespace-nowrap rounded-md font-medium transition-colors duration-100 disabled:cursor-not-allowed disabled:opacity-50";
  const sizes = { sm: "h-7 px-2.5 text-xs", md: "h-8 px-3.5 text-[13px]", lg: "h-10 px-5 text-sm" }[size];
  const variants: Record<Variant, string> = {
    primary: "bg-accent text-white hover:bg-accent-strong",
    secondary: "bg-surface-2 text-ink border border-line hover:bg-surface-3 hover:border-line-strong",
    ghost: "text-ink-muted hover:text-ink hover:bg-surface-3",
    danger: "text-bad border border-bad/30 hover:bg-bad-soft",
    dangerSolid: "bg-bad text-white hover:bg-bad/90",
  };
  return (
    <button className={`${base} ${sizes} ${variants[variant]} ${className}`} disabled={loading || rest.disabled} {...rest}>
      {loading ? <Loader2 className="h-4 w-4 animate-spin" /> : icon}
      {children}
    </button>
  );
}

export function Card({ children, className = "", ...rest }: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div className={`card ${className}`} {...rest}>
      {children}
    </div>
  );
}

export function Badge({ children, tone = "neutral", className = "" }: { children: ReactNode; tone?: "neutral" | "accent" | "ok" | "warn" | "bad"; className?: string }) {
  const tones = {
    neutral: "bg-surface-3 text-ink-muted border-line",
    accent: "bg-accent-soft text-accent border-accent/25",
    ok: "bg-ok-soft text-ok border-ok/25",
    warn: "bg-warn-soft text-warn border-warn/25",
    bad: "bg-bad-soft text-bad border-bad/25",
  }[tone];
  return <span className={`inline-flex items-center gap-1 whitespace-nowrap rounded border px-1.5 py-px text-[11px] font-medium ${tones} ${className}`}>{children}</span>;
}

export function Switch({ checked, onChange, disabled, label }: { checked: boolean; onChange: (v: boolean) => void; disabled?: boolean; label: string }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-5 w-9 shrink-0 items-center rounded-full border transition-colors duration-150 disabled:opacity-50 ${
        checked ? "border-accent/60 bg-accent" : "border-line-strong bg-surface-3"
      }`}
    >
      <span className={`inline-block h-3 w-3 rounded-full bg-white transition-transform duration-150 ${checked ? "translate-x-[18px]" : "translate-x-[3px]"}`} />
    </button>
  );
}

/** Styled checkbox with an optional title and description. */
export function Checkbox({ checked, onChange, disabled, title, description, className = "" }: { checked: boolean; onChange: (v: boolean) => void; disabled?: boolean; title: ReactNode; description?: ReactNode; className?: string }) {
  return (
    <label className={`flex cursor-pointer items-start gap-3 ${disabled ? "cursor-not-allowed opacity-60" : ""} ${className}`}>
      <input type="checkbox" className="peer sr-only" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
      <span
        aria-hidden
        className={`mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded border transition-colors peer-focus-visible:ring-2 peer-focus-visible:ring-accent ${
          checked ? "border-accent bg-accent text-white" : "border-line-strong bg-surface-2"
        }`}
      >
        {checked && <Check className="h-3 w-3" strokeWidth={3} />}
      </span>
      <span className="min-w-0">
        <span className="block text-sm font-medium text-ink">{title}</span>
        {description && <span className="mt-0.5 block text-xs leading-relaxed text-ink-muted">{description}</span>}
      </span>
    </label>
  );
}

/** Integer input with − / + buttons instead of the native spinner. Commits on blur, Enter and button press. */
export function NumberField({ value, onCommit, min, max, ariaLabel, disabled, invalid, className = "" }: { value: string | number; onCommit: (v: string) => void; min?: number | null; max?: number | null; ariaLabel: string; disabled?: boolean; invalid?: boolean; className?: string }) {
  const [text, setText] = useState(String(value ?? ""));
  useEffect(() => setText(String(value ?? "")), [value]);
  const clamp = (n: number) => Math.min(max ?? Infinity, Math.max(min ?? -Infinity, n));
  const step = (d: number) => {
    const n = Number(text);
    const next = String(clamp((Number.isFinite(n) ? n : (min ?? 0)) + d));
    setText(next);
    onCommit(next);
  };
  const btn = "flex h-full w-7 items-center justify-center text-ink-muted transition-colors hover:bg-surface-3 hover:text-ink disabled:opacity-40";
  return (
    <div className={`inline-flex h-8 items-stretch overflow-hidden rounded-md border bg-surface-2 focus-within:border-accent ${invalid ? "border-bad/70" : "border-line"} ${disabled ? "opacity-60" : ""} ${className}`}>
      <button type="button" tabIndex={-1} className={btn} disabled={disabled} aria-label="−" onClick={() => step(-1)}>
        <Minus className="h-3.5 w-3.5" />
      </button>
      <input
        type="number"
        inputMode="numeric"
        aria-label={ariaLabel}
        disabled={disabled}
        className="w-full min-w-0 border-x border-line bg-transparent px-2 text-center font-mono text-sm text-ink focus:outline-none"
        value={text}
        min={min ?? undefined}
        max={max ?? undefined}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => text !== String(value) && onCommit(text)}
        onKeyDown={(e) => e.key === "Enter" && onCommit(text)}
      />
      <button type="button" tabIndex={-1} className={btn} disabled={disabled} aria-label="+" onClick={() => step(1)}>
        <Plus className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}

export function Skeleton({ className = "" }: { className?: string }) {
  return <div className={`skeleton ${className}`} aria-hidden />;
}

/** Placeholder for a page whose data is loading. */
export function PageSkeleton({ rows = 3 }: { rows?: number }) {
  return (
    <div className="space-y-4" aria-busy="true">
      <Skeleton className="h-7 w-48" />
      <Skeleton className="h-36 w-full" />
      {Array.from({ length: rows }, (_, i) => (
        <Skeleton key={i} className="h-14 w-full" />
      ))}
    </div>
  );
}

export function Select({ value, onChange, options, className = "", ariaLabel, disabled }: { value: string; onChange: (v: string) => void; options: { value: string; label: string; group?: string }[]; className?: string; ariaLabel: string; disabled?: boolean }) {
  const groups = Array.from(new Set(options.map((o) => o.group ?? "")));
  return (
    <select aria-label={ariaLabel} disabled={disabled} className={`field cursor-pointer ${className}`} value={value} onChange={(e) => onChange(e.target.value)}>
      {groups.map((g) =>
        g ? (
          <optgroup key={g} label={g}>
            {options.filter((o) => (o.group ?? "") === g).map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </optgroup>
        ) : (
          options.filter((o) => !o.group).map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))
        ),
      )}
    </select>
  );
}

export function Tooltip({ text, children }: { text: string; children?: ReactNode }) {
  const id = useId();
  return (
    <span className="group relative inline-flex">
      <span tabIndex={0} aria-describedby={id} className="inline-flex cursor-help text-ink-faint hover:text-ink-muted">
        {children ?? <Info className="h-3.5 w-3.5" />}
      </span>
      <span
        role="tooltip"
        id={id}
        className="pointer-events-none absolute bottom-full left-1/2 z-50 mb-2 w-64 -translate-x-1/2 rounded-md border border-line-strong bg-surface px-3 py-2 text-xs font-normal leading-relaxed text-ink opacity-0 shadow-pop transition-opacity duration-100 group-focus-within:opacity-100 group-hover:opacity-100"
      >
        {text}
      </span>
    </span>
  );
}

export function StatusDot({ status }: { status: CheckStatus | "running" | "stopped"; pulse?: boolean }) {
  const color = {
    ok: "bg-ok",
    running: "bg-ok",
    warning: "bg-warn",
    error: "bg-bad",
    unknown: "bg-ink-faint",
    skipped: "bg-ink-faint",
    stopped: "bg-ink-faint",
  }[status];
  return (
    <span className="relative inline-flex h-2 w-2 shrink-0">
      <span className={`relative inline-flex h-2 w-2 rounded-full ${color}`} />
    </span>
  );
}

export function Modal({ open, onClose, title, children, wide, footer }: { open: boolean; onClose: () => void; title: string; children: ReactNode; wide?: boolean; footer?: ReactNode }) {
  const reduce = useReducedMotion();
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    ref.current?.focus();
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);
  return (
    <AnimatePresence>
      {open && (
        <motion.div className="fixed inset-0 z-40 flex items-center justify-center bg-black/50 p-6 backdrop-blur-[1px]" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} onMouseDown={onClose}>
          <motion.div
            ref={ref}
            tabIndex={-1}
            role="dialog"
            aria-modal="true"
            aria-label={title}
            onMouseDown={(e) => e.stopPropagation()}
            initial={reduce ? false : { opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={reduce ? undefined : { opacity: 0 }}
            transition={{ duration: 0.12, ease: "easeOut" }}
            className={`card flex max-h-[88vh] w-full flex-col shadow-pop outline-none focus:outline-none ${wide ? "max-w-5xl" : "max-w-xl"}`}
          >
            <div className="flex items-center justify-between border-b border-line px-5 py-3">
              <h2 className="text-[15px] font-semibold">{title}</h2>
              <button aria-label="Закрыть" className="rounded p-1 text-ink-muted hover:bg-surface-3 hover:text-ink" onClick={onClose}>
                <X className="h-4 w-4" />
              </button>
            </div>
            <div className="min-h-0 flex-1 overflow-auto px-5 py-4">{children}</div>
            {footer && <div className="flex justify-end gap-2 border-t border-line px-5 py-3">{footer}</div>}
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

export function Segmented<T extends string>({ value, onChange, options, ariaLabel, full }: { value: T; onChange: (v: T) => void; options: { value: T; label: string; icon?: ReactNode }[]; ariaLabel: string; full?: boolean }) {
  return (
    <div role="tablist" aria-label={ariaLabel} className={`${full ? "flex w-full" : "inline-flex shrink-0"} rounded-md border border-line bg-surface-2 p-0.5`}>
      {options.map((o) => (
        <button
          key={o.value}
          role="tab"
          aria-selected={value === o.value}
          onClick={() => onChange(o.value)}
          className={`inline-flex items-center justify-center gap-1.5 whitespace-nowrap rounded px-2.5 py-1 text-xs font-medium transition-colors ${full ? "flex-1 px-1" : ""} ${
            value === o.value ? "bg-surface-3 text-ink" : "text-ink-muted hover:text-ink"
          }`}
        >
          {o.icon}
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function SettingRow({ title, description, children }: { title: ReactNode; description?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6 py-3">
      <div className="min-w-0">
        <div className="text-sm font-medium">{title}</div>
        {description && <div className="mt-0.5 text-xs leading-relaxed text-ink-muted">{description}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

export function PageHeader({ title, subtitle, actions }: { title: string; subtitle?: string; actions?: ReactNode }) {
  return (
    <div className="mb-4 flex items-end justify-between gap-4">
      <div>
        <h1 className="text-lg font-semibold">{title}</h1>
        {subtitle && <p className="mt-0.5 text-[13px] text-ink-muted">{subtitle}</p>}
      </div>
      {actions && <div className="flex shrink-0 gap-2">{actions}</div>}
    </div>
  );
}

export function Empty({ icon, title, text }: { icon: ReactNode; title: string; text?: string }) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 py-12 text-center">
      <div className="text-ink-faint">{icon}</div>
      <div className="text-sm font-medium">{title}</div>
      {text && <div className="max-w-sm text-xs text-ink-muted">{text}</div>}
    </div>
  );
}

export interface MenuItem {
  label: string;
  icon?: ReactNode;
  onClick: () => void;
  danger?: boolean;
  hidden?: boolean;
}

/** Overflow menu ("⋯") for secondary actions. Closes on outside click and Escape. */
export function Menu({ items, label, trigger }: { items: MenuItem[]; label: string; trigger?: ReactNode }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && setOpen(false);
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);
  const visible = items.filter((i) => !i.hidden);
  return (
    <div ref={ref} className="relative">
      <Button size="sm" variant={trigger ? "secondary" : "ghost"} aria-label={label} title={label} aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen(!open)} icon={trigger ? undefined : <MoreHorizontal className="h-4 w-4" />}>
        {trigger}
      </Button>
      {open && (
        <div role="menu" className="absolute right-0 top-full z-30 mt-1 min-w-[220px] rounded-md border border-line-strong bg-surface py-1 shadow-pop">
          {visible.map((i) => (
            <button
              key={i.label}
              role="menuitem"
              onClick={() => {
                setOpen(false);
                i.onClick();
              }}
              className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-[13px] hover:bg-surface-2 ${i.danger ? "text-bad" : "text-ink"}`}
            >
              <span className={i.danger ? "text-bad" : "text-ink-muted"}>{i.icon}</span>
              {i.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
