import { useCallback, useEffect, useState } from "react";
import { t, type Locale } from "@/lib/i18n";
import { api, type UpdateCheckResult } from "@/lib/tauri";

type AppInfo = {
  version: string;
  git_hash: string;
  build_date: string;
};

const defaultAppInfo: AppInfo = {
  version: "1.2.0-rc.1",
  git_hash: "dev",
  build_date: "unknown",
};

export function useAboutPage({
  locale,
  setUpdateInfo,
}: {
  locale: Locale;
  setUpdateInfo: (result: UpdateCheckResult | null) => void;
}) {
  const [appInfo, setAppInfo] = useState<AppInfo>(defaultAppInfo);
  const [checking, setChecking] = useState(false);
  const [message, setMessage] = useState("");

  useEffect(() => {
    api.getAppInfo().then(setAppInfo).catch(() => undefined);
  }, []);

  const checkUpdate = useCallback(async () => {
    setChecking(true);
    setMessage("");
    try {
      const result = await api.checkUpdate(true);
      if (result.has_update && result.latest_version) {
        setUpdateInfo(result);
        setMessage(`${t(locale, "updateAvailable")} v${result.latest_version}`);
      } else {
        setMessage(t(locale, "upToDate"));
      }
    } catch {
      setMessage(t(locale, "updateFailed"));
    } finally {
      setChecking(false);
    }
  }, [locale, setUpdateInfo]);

  return {
    appInfo,
    checking,
    message,
    checkUpdate,
  };
}
