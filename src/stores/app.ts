import { create } from "zustand";
import { api, errorText } from "../services/api";
import type { ActivationResult, AutoProgress, CatalogView, LaunchResult, Overview, StrategyListItem } from "../services/types";

export type Page = "home" | "strategies" | "lists" | "diagnostics" | "settings" | "about";

export interface Toast {
  id: number;
  kind: "ok" | "error" | "info";
  text: string;
}

interface AppStore {
  page: Page;
  lang: "ru" | "en";
  overview: Overview | null;
  overviewError: string | null;
  strategies: StrategyListItem[];
  selectedStrategyId: string | null;
  busy: boolean;
  toasts: Toast[];
  lastActivation: ActivationResult | null;
  auto: AutoProgress | null;
  launchFailure: LaunchResult | null;
  catalog: CatalogView | null;
  setCatalog: (c: CatalogView) => void;
  loadCatalog: () => Promise<void>;
  setLaunchFailure: (r: LaunchResult | null) => void;
  setPage: (p: Page) => void;
  setLang: (l: "ru" | "en") => void;
  selectStrategy: (id: string | null) => void;
  refresh: () => Promise<void>;
  refreshStrategies: () => Promise<void>;
  toast: (kind: Toast["kind"], text: string) => void;
  dismiss: (id: number) => void;
  setBusy: (b: boolean) => void;
  setActivation: (r: ActivationResult | null) => void;
  setAuto: (a: AutoProgress | null) => void;
}

let toastId = 1;

export const useApp = create<AppStore>((set, get) => ({
  page: "home",
  lang: "ru",
  overview: null,
  overviewError: null,
  strategies: [],
  selectedStrategyId: null,
  busy: false,
  toasts: [],
  lastActivation: null,
  auto: null,
  launchFailure: null,
  catalog: null,
  setCatalog: (catalog) => set({ catalog }),
  loadCatalog: async () => {
    try {
      set({ catalog: await api.catalogGet() });
    } catch (e) {
      get().toast("error", errorText(e));
    }
  },
  setLaunchFailure: (launchFailure) => set({ launchFailure }),
  setPage: (page) => set({ page }),
  setLang: (lang) => set({ lang }),
  selectStrategy: (selectedStrategyId) => set({ selectedStrategyId }),
  refresh: async () => {
    try {
      const overview = await api.overview();
      set({ overview, overviewError: null, lang: overview.settings.language });
    } catch (e) {
      set({ overviewError: errorText(e) });
    }
  },
  refreshStrategies: async () => {
    try {
      const strategies = await api.listStrategies();
      set({ strategies });
      const sel = get().selectedStrategyId;
      if (!sel || !strategies.some((s) => s.id === sel)) {
        set({ selectedStrategyId: get().overview?.settings.activeStrategyId ?? strategies[0]?.id ?? null });
      }
    } catch (e) {
      get().toast("error", errorText(e));
    }
  },
  toast: (kind, text) => {
    const id = toastId++;
    set({ toasts: [...get().toasts, { id, kind, text }] });
    setTimeout(() => get().dismiss(id), kind === "error" ? 8000 : 4500);
  },
  dismiss: (id) => set({ toasts: get().toasts.filter((t) => t.id !== id) }),
  setBusy: (busy) => set({ busy }),
  setActivation: (lastActivation) => set({ lastActivation }),
  setAuto: (auto) => set({ auto }),
}));

/** Inline bilingual strings: `tr("Русский", "English")`. Russian by default. */
export function useTr() {
  const lang = useApp((s) => s.lang);
  return (ru: string, en: string) => (lang === "en" ? en : ru);
}

/** Russian plural form for `n`: [one, few, many] — 1 правило, 2 правила, 5 правил. */
export function ruPlural(n: number, forms: [string, string, string]): string {
  const a = Math.abs(n) % 100;
  const b = a % 10;
  if (a > 10 && a < 20) return forms[2];
  if (b === 1) return forms[0];
  if (b >= 2 && b <= 4) return forms[1];
  return forms[2];
}

/** `count(5, ["правило", "правила", "правил"], ["rule", "rules"])` → «5 правил» / "5 rules". */
export function useCount() {
  const lang = useApp((s) => s.lang);
  return (n: number, ru: [string, string, string], en: [string, string]) =>
    `${n} ${lang === "en" ? (n === 1 ? en[0] : en[1]) : ruPlural(n, ru)}`;
}
