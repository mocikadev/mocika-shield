import { useEffect, useState } from "react";
import { defaultSignConfig } from "@/components/app/branding";
import { detectSystemLocale, type Locale } from "@/lib/i18n";
import { api, type AppConfig, type SignConfig, type ThemeMode, type UpdateCheckResult } from "@/lib/tauri";
import { normalizeThemeMode } from "@/hooks/use-applied-theme-mode";

export function useAppConfigState() {
  const [locale, setLocale] = useState<Locale>(detectSystemLocale);
  const [themeMode, setThemeMode] = useState<ThemeMode>("system");
  const [signConfig, setSignConfig] = useState<SignConfig>(defaultSignConfig);
  const [keystorePassword, setKeystorePassword] = useState("");
  const [keyPassword, setKeyPassword] = useState("");
  const [signConfigLoaded, setSignConfigLoaded] = useState(false);

  useEffect(() => {
    let disposed = false;
    api.getAppConfig()
      .then((value: AppConfig) => {
        if (disposed) {
          return;
        }
        setLocale(value.locale === "en" ? "en" : "zh");
        setThemeMode(normalizeThemeMode(value.theme_mode));
        setSignConfig({
          ...defaultSignConfig,
          ...value.sign_config,
          ks_type: value.sign_config.ks_type || "JKS",
        });
        setKeystorePassword(value.keystore_password ?? "");
        setKeyPassword(value.key_password ?? "");
      })
      .catch(() => undefined)
      .finally(() => {
        if (!disposed) {
          setSignConfigLoaded(true);
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
    signConfig,
    setSignConfig,
    keystorePassword,
    setKeystorePassword,
    keyPassword,
    setKeyPassword,
    signConfigLoaded,
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
