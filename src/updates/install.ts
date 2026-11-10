import { useState } from "react";
import { confirmDialog } from "../components/ui/confirm";
import { api, errorText } from "../services/api";
import { useApp, useTr } from "../stores/app";

/**
 * Installing a newer Bridge from inside the app. Offered only when the last
 * check verified the signed manifest of that release (`installable`); in
 * every other case the release page stays the only way. The backend decides
 * what is downloaded: nothing but the request itself is sent from here.
 */
export function useAppUpdate() {
  const { updates, toast } = useApp();
  const tr = useTr();
  const [installing, setInstalling] = useState(false);
  const info = updates?.app;
  const canInstall = !!info && info.status === "available" && info.newer && info.installable;
  const install = async () => {
    if (!canInstall || installing) return;
    const ok = await confirmDialog({
      title: tr(`Установить Bridge ${info.latest}?`, `Install Bridge ${info.latest}?`),
      body: tr(
        "Bridge скачает установщик, сверит его с подписанным манифестом релиза и запустит. Windows спросит разрешение администратора. На время установки Bridge закроется.",
        "Bridge will download the installer, check it against the signed release manifest and start it. Windows will ask for administrator permission. Bridge closes while the update is installed.",
      ),
      confirmLabel: tr("Скачать и установить", "Download and install"),
    });
    if (!ok) return;
    setInstalling(true);
    try {
      await api.appUpdateInstall();
      toast("ok", tr("Установщик запущен, Bridge закрывается.", "The installer has started, Bridge is closing."));
    } catch (e) {
      toast("error", errorText(e));
    } finally {
      setInstalling(false);
    }
  };
  return { canInstall, installing, install };
}
