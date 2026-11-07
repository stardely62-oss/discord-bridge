import { listen } from "@tauri-apps/api/event";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useEffect } from "react";
import { LaunchDialog } from "./components/layout/LaunchDialog";
import { Onboarding } from "./components/layout/Onboarding";
import { Sidebar, Toasts, UpdateNotices } from "./components/layout/Shell";
import { ActivationDialog } from "./components/strategies/ActivationDialog";
import { ConfirmHost } from "./components/ui/confirm";
import ListsPage from "./pages/Lists/ListsPage";
import AboutPage from "./pages/About/AboutPage";
import DiagnosticsPage from "./pages/Diagnostics/DiagnosticsPage";
import HomePage from "./pages/Home/HomePage";
import SettingsPage from "./pages/Settings/SettingsPage";
import StrategiesPage from "./pages/Strategies/StrategiesPage";
import type { ActivationResult, AutoProgress, UpdatesView } from "./services/types";
import { Page, useApp } from "./stores/app";

export default function App() {
  const { page, refresh, refreshStrategies, setPage, setActivation, setAuto, overview, loadCatalog, loadServices, loadUpdates, setUpdates, loadTelegram } = useApp();
  const reduce = useReducedMotion();
  const theme = overview?.settings.theme ?? "system";

  // Theme: explicit choice or follow Windows.
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    const apply = () => {
      const light = theme === "light" || (theme === "system" && mq.matches);
      document.documentElement.dataset.theme = light ? "light" : "dark";
    };
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);

  useEffect(() => {
    refresh();
    refreshStrategies();
    loadCatalog();
    loadServices();
    loadUpdates();
    loadTelegram();
    const un: Promise<() => void>[] = [
      listen("catalog-changed", () => {
        loadCatalog();
        loadServices();
      }),
      listen("status-changed", () => {
        refresh();
        loadServices();
      }),
      listen("strategies-changed", () => {
        refreshStrategies();
        refresh();
        loadServices();
      }),
      listen<string>("navigate", (e) => setPage(e.payload as Page)),
      listen<ActivationResult>("activation-result", (e) => {
        setActivation(e.payload);
        refresh();
      }),
      listen<AutoProgress>("auto-progress", (e) => setAuto(e.payload)),
      listen<UpdatesView>("updates-changed", (e) => setUpdates(e.payload)),
      listen("telegram-changed", () => loadTelegram()),
    ];
    return () => {
      un.forEach((p) => p.then((f) => f()));
    };
  }, [refresh, refreshStrategies, setPage, setActivation, setAuto, loadCatalog, loadServices, loadUpdates, setUpdates, loadTelegram]);

  const pages: Record<Page, JSX.Element> = {
    home: <HomePage />,
    strategies: <StrategiesPage />,
    lists: <ListsPage />,
    diagnostics: <DiagnosticsPage />,
    settings: <SettingsPage />,
    about: <AboutPage />,
  };

  return (
    <div className="flex h-full overflow-hidden">
      <Sidebar />
      <main className="relative min-w-0 flex-1 overflow-y-auto">
        <AnimatePresence mode="wait">
          <motion.div
            key={page}
            initial={reduce ? false : { opacity: 0 }}
            animate={{ opacity: 1, y: 0 }}
            exit={reduce ? undefined : { opacity: 0 }}
            transition={{ duration: 0.1 }}
            className={page === "strategies" ? "h-full" : "mx-auto max-w-4xl px-7 py-6 narrow:px-5"}
          >
            {pages[page]}
          </motion.div>
        </AnimatePresence>
      </main>
      <Onboarding />
      <ActivationDialog />
      <LaunchDialog />
      <ConfirmHost />
      <Toasts />
      <UpdateNotices />
    </div>
  );
}
