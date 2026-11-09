import { useCallback, useEffect, useRef, useState } from "react";
import { detectSystemLocale, type Locale } from "@/lib/i18n";
import { api, type AppConfig, type ProtectDefaults, type ThemeMode, type UpdateCheckResult } from "@/lib/tauri";
import { normalizeThemeMode } from "@/hooks/use-applied-theme-mode";
import { PreferenceSync } from "@/lib/preference-sync";
import { notifyError } from "@/lib/notify";

export function useAppConfigState() {
  const [locale, setLocale] = useState<Locale>(detectSystemLocale);
  const [themeMode, setThemeMode] = useState<ThemeMode>("system");
  const [configLoaded, setConfigLoaded] = useState(false);
  const [telemetryEnabled, setTelemetryEnabled] = useState(true);
  const [protectDefaults, setProtectDefaults] = useState<ProtectDefaults>({
    runtime_mode: "standard", environment_policy: "compatible", sign_after_protect: null,
    certificate_id: null, output_directory_mode: "source", fixed_output_directory: "",
  });
  const [sync] = useState(() => new PreferenceSync());
  const lastError = useRef("");
  const refresh = useCallback(async () => {
    const token = sync.beginRead();
    if (token === undefined) return;
    try {
      const value = await api.getAppConfig();
      if (!sync.canApply(token)) return;
      setLocale(value.locale === "en" ? "en" : "zh");
      setThemeMode(normalizeThemeMode(value.theme_mode));
      setTelemetryEnabled(value.telemetry_enabled !== false);
      if (value.protect_defaults) setProtectDefaults(value.protect_defaults);
      setConfigLoaded(true);
      lastError.current = "";
    } catch (error) {
      if (!sync.canApply(token)) return;
      const message = String(error);
      if (message !== lastError.current) notifyError(message);
      lastError.current = message;
    }
  }, [sync]);

  const savePreferences = useCallback(async (patch: Partial<AppConfig>) => {
    try {
      await sync.write(async () => {
        await api.saveAppConfig(patch);
        if (patch.locale !== undefined) setLocale(patch.locale === "en" ? "en" : "zh");
        if (patch.theme_mode !== undefined) setThemeMode(normalizeThemeMode(patch.theme_mode));
        if (patch.telemetry_enabled !== undefined) setTelemetryEnabled(patch.telemetry_enabled);
      });
    } finally { if (!sync.busy) await refresh(); }
  }, [sync, refresh]);
  const saveProtectDefaults = useCallback(async (patch: Partial<ProtectDefaults>) => {
    setProtectDefaults((current) => ({ ...current, ...patch }));
    try { await sync.write(() => api.saveProtectDefaults(patch)); }
    finally { if (!sync.busy) await refresh(); }
  }, [sync, refresh]);

  useEffect(() => {
    void api.syncTelemetry();
    void refresh();
    const onFocus = () => { void refresh(); };
    const onVisibility = () => { if (document.visibilityState === "visible") void refresh(); };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      sync.invalidate();
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [refresh, sync]);
  return { locale, themeMode, configLoaded, telemetryEnabled, protectDefaults, savePreferences, saveProtectDefaults, refresh };
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
