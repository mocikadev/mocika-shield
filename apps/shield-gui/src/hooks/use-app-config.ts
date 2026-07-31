import { useEffect, useState } from "react";
import { detectSystemLocale, type Locale } from "@/lib/i18n";
import { api, type AppConfig, type ProtectDefaults, type ThemeMode, type UpdateCheckResult } from "@/lib/tauri";
import { normalizeThemeMode } from "@/hooks/use-applied-theme-mode";

export function useAppConfigState() {
  const [locale, setLocale] = useState<Locale>(detectSystemLocale);
  const [themeMode, setThemeMode] = useState<ThemeMode>("system");
  const [configLoaded, setConfigLoaded] = useState(false);
  const [telemetryEnabled, setTelemetryEnabled] = useState(true);
  const [protectDefaults, setProtectDefaults] = useState<ProtectDefaults>({
    runtime_mode: "standard",
    environment_policy: "compatible",
    sign_after_protect: null,
    certificate_id: null,
    output_directory_mode: "source",
    fixed_output_directory: "",
  });

  useEffect(() => {
    void api.syncTelemetry();
    let disposed = false;
    api.getAppConfig()
      .then((value: AppConfig) => {
        if (disposed) {
          return;
        }
        setLocale(value.locale === "en" ? "en" : "zh");
        setThemeMode(normalizeThemeMode(value.theme_mode));
        setTelemetryEnabled(value.telemetry_enabled !== false);
        if (value.protect_defaults) {
          setProtectDefaults(value.protect_defaults);
        }
      })
      .catch(() => undefined)
      .finally(() => {
        if (!disposed) {
          setConfigLoaded(true);
        }
      });
    return () => {
      disposed = true;
    };
  }, []);

  return {
    locale,
    setLocale,
    themeMode,
    setThemeMode,
    configLoaded,
    telemetryEnabled,
    setTelemetryEnabled,
    protectDefaults,
    setProtectDefaults,
  };
}

export function useAutoUpdateNotice() {
  const [updateInfo, setUpdateInfo] = useState<UpdateCheckResult | null>(null);
  const [majorDialogOpen, setMajorDialogOpen] = useState(false);

  useEffect(() => {
    let disposed = false;
    async function run() {
      try {
        const dismissed = await api.getDismissedVersion().catch(() => null);
        const result = await api.checkUpdate(false);
        if (disposed || !result.has_update || !result.latest_version) {
          return;
        }
        if (result.update_level !== "major" && dismissed === result.latest_version) {
          return;
        }
        setUpdateInfo(result);
        if (result.update_level === "major") {
          window.setTimeout(() => setMajorDialogOpen(true), 1200);
        }
      } catch {
        // 自动更新检查静默失败。
      }
    }
    void run();
    return () => {
      disposed = true;
    };
  }, []);

  return {
    updateInfo,
    setUpdateInfo,
    majorDialogOpen,
    setMajorDialogOpen,
  };
}
