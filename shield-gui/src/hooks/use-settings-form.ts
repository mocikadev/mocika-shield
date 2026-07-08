import { useEffect, useState } from "react";
import { defaultSignConfig } from "@/components/app/branding";
import { t, type Locale } from "@/lib/i18n";
import { api, openFileDialog, type SignConfig, type ThemeMode } from "@/lib/tauri";

export type SettingsSavePayload = {
  locale: Locale;
  themeMode: ThemeMode;
  signConfig: SignConfig;
  keystorePassword: string;
  keyPassword: string;
};

export function useSettingsForm({
  locale,
  setLocale,
  themeMode,
  setThemeMode,
  signConfig,
  keystorePassword,
  keyPassword,
  onConfigSaved,
}: {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  themeMode: ThemeMode;
  setThemeMode: (mode: ThemeMode) => void;
  signConfig: SignConfig;
  keystorePassword: string;
  keyPassword: string;
  onConfigSaved: (config: SettingsSavePayload) => void;
}) {
  const [selectedLocale, setSelectedLocale] = useState<Locale>(locale);
  const [selectedThemeMode, setSelectedThemeMode] = useState<ThemeMode>(themeMode);
  const [config, setConfig] = useState<SignConfig>({
    ...defaultSignConfig,
    ...signConfig,
    ks_type: signConfig.ks_type || "JKS",
  });
  const [ksPass, setKsPass] = useState(keystorePassword);
  const [keyPass, setKeyPass] = useState(keyPassword);
  const [showPass, setShowPass] = useState(false);
  const [aliases, setAliases] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const [detecting, setDetecting] = useState(false);
  const [status, setStatus] = useState<"idle" | "saved" | "failed">("idle");
  const [error, setError] = useState("");

  useEffect(() => {
    setSelectedLocale(locale);
  }, [locale]);

  useEffect(() => {
    setSelectedThemeMode(themeMode);
  }, [themeMode]);

  useEffect(() => {
    setConfig({
      ...defaultSignConfig,
      ...signConfig,
      ks_type: signConfig.ks_type || "JKS",
    });
  }, [signConfig]);

  useEffect(() => {
    setKsPass(keystorePassword);
  }, [keystorePassword]);

  useEffect(() => {
    setKeyPass(keyPassword);
  }, [keyPassword]);

  async function browseKeystore() {
    const path = await openFileDialog("Keystore", ["jks", "keystore", "p12", "pfx", "bks"]);
    if (path) {
      setConfig((old) => ({
        ...old,
        keystore_path: path,
        ks_type:
          path.toLowerCase().endsWith(".p12") || path.toLowerCase().endsWith(".pfx")
            ? "PKCS12"
            : "JKS",
      }));
      setAliases([]);
      setStatus("idle");
    }
  }

  async function detectAlias() {
    if (!config.keystore_path || !ksPass) {
      return;
    }
    setDetecting(true);
    setError("");
    try {
      const list = await api.listKeystoreAliases(
        config.keystore_path,
        ksPass,
        config.ks_type || "JKS",
      );
      setAliases(list);
      if (list.length === 1) {
        setConfig((old) => ({ ...old, key_alias: list[0] }));
      }
    } catch (err) {
      setError(`${t(locale, "aliasFailed")}: ${String(err)}`);
    } finally {
      setDetecting(false);
    }
  }

  async function save() {
    setSaving(true);
    setError("");
    setStatus("idle");
    try {
      if (config.auto_sign_enabled && (!config.keystore_path || !config.key_alias || !ksPass)) {
        throw new Error(t(locale, "saveSignFirst"));
      }
      await api.saveAppConfig({
        locale: selectedLocale,
        theme_mode: selectedThemeMode,
        sign_config: config,
        keystore_password: ksPass,
        key_password: keyPass,
      });
      onConfigSaved({
        locale: selectedLocale,
        themeMode: selectedThemeMode,
        signConfig: config,
        keystorePassword: ksPass,
        keyPassword: keyPass,
      });
      setStatus("saved");
    } catch (err) {
      setError(String(err));
      setStatus("failed");
    } finally {
      setSaving(false);
    }
  }

  function updateLocale(next: Locale) {
    setSelectedLocale(next);
    setLocale(next);
  }

  function updateThemeMode(next: ThemeMode) {
    setSelectedThemeMode(next);
    setThemeMode(next);
  }

  return {
    selectedLocale,
    selectedThemeMode,
    config,
    setConfig,
    ksPass,
    setKsPass,
    keyPass,
    setKeyPass,
    showPass,
    setShowPass,
    aliases,
    saving,
    detecting,
    status,
    error,
    updateLocale,
    updateThemeMode,
    browseKeystore,
    detectAlias,
    save,
    signingConfigured: Boolean(config.keystore_path && config.key_alias),
  };
}
