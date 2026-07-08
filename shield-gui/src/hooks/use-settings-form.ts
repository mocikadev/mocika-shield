import { useEffect, useRef, useState } from "react";
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

function signingConfigEquals(left: SignConfig, right: SignConfig) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function signingDraftEquals(
  left: { signConfig: SignConfig; keystorePassword: string; keyPassword: string },
  right: { signConfig: SignConfig; keystorePassword: string; keyPassword: string },
) {
  return (
    signingConfigEquals(left.signConfig, right.signConfig) &&
    left.keystorePassword === right.keystorePassword &&
    left.keyPassword === right.keyPassword
  );
}

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
  const [autoSignSaving, setAutoSignSaving] = useState(false);
  const [detecting, setDetecting] = useState(false);
  const [status, setStatus] = useState<"idle" | "validating" | "validated" | "saved" | "failed">("idle");
  const [error, setError] = useState("");
  const [signingDirty, setSigningDirty] = useState(false);
  const persistedSignConfigRef = useRef<SignConfig>({
    ...defaultSignConfig,
    ...signConfig,
    ks_type: signConfig.ks_type || "JKS",
  });
  const persistedKsPassRef = useRef(keystorePassword);
  const persistedKeyPassRef = useRef(keyPassword);
  const validatedSigningRef = useRef<{
    signConfig: SignConfig;
    keystorePassword: string;
    keyPassword: string;
  } | null>(null);
  const statusTimerRef = useRef<number | null>(null);

  useEffect(() => {
    setSelectedLocale(locale);
  }, [locale]);

  useEffect(() => {
    setSelectedThemeMode(themeMode);
  }, [themeMode]);

  useEffect(() => {
    const normalizedConfig = {
      ...defaultSignConfig,
      ...signConfig,
      ks_type: signConfig.ks_type || "JKS",
    };
    setConfig(normalizedConfig);
    setKsPass(keystorePassword);
    setKeyPass(keyPassword);
    persistedSignConfigRef.current = normalizedConfig;
    persistedKsPassRef.current = keystorePassword;
    persistedKeyPassRef.current = keyPassword;
    validatedSigningRef.current =
      normalizedConfig.keystore_path && normalizedConfig.key_alias
        ? {
            signConfig: normalizedConfig,
            keystorePassword,
            keyPassword,
          }
        : null;
    setSigningDirty(false);
    setStatus("idle");
    setError("");
  }, [keyPassword, keystorePassword, signConfig]);

  useEffect(() => {
    return () => {
      if (statusTimerRef.current) {
        window.clearTimeout(statusTimerRef.current);
      }
    };
  }, []);

  async function persistConfig(payload: {
    locale: Locale;
    theme_mode: ThemeMode;
    sign_config: SignConfig;
    keystore_password: string;
    key_password: string;
  }) {
    await api.saveAppConfig(payload);
    persistedSignConfigRef.current = {
      ...defaultSignConfig,
      ...payload.sign_config,
      ks_type: payload.sign_config.ks_type || "JKS",
    };
    persistedKsPassRef.current = payload.keystore_password;
    persistedKeyPassRef.current = payload.key_password;
    onConfigSaved({
      locale: payload.locale,
      themeMode: payload.theme_mode,
      signConfig: payload.sign_config,
      keystorePassword: payload.keystore_password,
      keyPassword: payload.key_password,
    });
  }

  function showSavedStatus() {
    if (statusTimerRef.current) {
      window.clearTimeout(statusTimerRef.current);
    }
    setStatus("saved");
    statusTimerRef.current = window.setTimeout(() => {
      setStatus("idle");
      statusTimerRef.current = null;
    }, 1600);
  }

  function updateSignConfig(updater: React.SetStateAction<SignConfig>) {
    setSigningDirty(true);
    setStatus("idle");
    setError("");
    validatedSigningRef.current = null;
    setConfig(updater);
  }

  function updateKeystorePassword(value: string) {
    setSigningDirty(true);
    setStatus("idle");
    setError("");
    validatedSigningRef.current = null;
    setKsPass(value);
  }

  function updateKeyPassword(value: string) {
    setSigningDirty(true);
    setStatus("idle");
    setError("");
    validatedSigningRef.current = null;
    setKeyPass(value);
  }

  async function browseKeystore() {
    const path = await openFileDialog("Keystore", ["jks", "keystore", "p12", "pfx", "bks"]);
    if (path) {
      updateSignConfig((old) => ({
        ...old,
        keystore_path: path,
        ks_type:
          path.toLowerCase().endsWith(".p12") || path.toLowerCase().endsWith(".pfx")
            ? "PKCS12"
            : "JKS",
      }));
      setAliases([]);
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
        updateSignConfig((old) => ({ ...old, key_alias: list[0] }));
      }
    } catch (err) {
      setError(`${t(locale, "aliasFailed")}: ${String(err)}`);
    } finally {
      setDetecting(false);
    }
  }

  async function validateSigningConfig() {
    const keystorePath = config.keystore_path?.trim() || "";
    const alias = config.key_alias?.trim() || "";
    const ksType = config.ks_type || "JKS";

    setSaving(true);
    setError("");
    setStatus("validating");

    try {
      if (!keystorePath) {
        throw new Error(t(locale, "missingKeystore"));
      }
      if (!ksPass.trim()) {
        throw new Error(t(locale, "missingPassword"));
      }

      const list = await api.listKeystoreAliases(keystorePath, ksPass, ksType);
      setAliases(list);

      let resolvedAlias = alias;
      if (!resolvedAlias && list.length === 1) {
        resolvedAlias = list[0];
      }
      if (!resolvedAlias) {
        throw new Error(t(locale, "missingAlias"));
      }
      if (!list.includes(resolvedAlias)) {
        throw new Error(t(locale, "aliasFailed"));
      }

      const nextConfig =
        resolvedAlias === alias ? config : { ...config, key_alias: resolvedAlias };
      if (resolvedAlias !== alias) {
        setConfig(nextConfig);
      }

      validatedSigningRef.current = {
        signConfig: nextConfig,
        keystorePassword: ksPass,
        keyPassword: keyPass,
      };
      setStatus("validated");
    } catch (err) {
      validatedSigningRef.current = null;
      setError(String(err));
      setStatus("failed");
    } finally {
      setSaving(false);
    }
  }

  async function saveSigningConfig() {
    const currentDraft = {
      signConfig: config,
      keystorePassword: ksPass,
      keyPassword: keyPass,
    };

    if (!validatedSigningRef.current || !signingDraftEquals(currentDraft, validatedSigningRef.current)) {
      setError(t(locale, "validateBeforeSave"));
      setStatus("failed");
      return;
    }

    setSaving(true);
    setError("");
    setStatus("idle");

    try {
      await persistConfig({
        locale: selectedLocale,
        theme_mode: selectedThemeMode,
        sign_config: currentDraft.signConfig,
        keystore_password: currentDraft.keystorePassword,
        key_password: currentDraft.keyPassword,
      });
      setSigningDirty(false);
      validatedSigningRef.current = currentDraft;
      showSavedStatus();
    } catch (err) {
      setError(String(err));
      setStatus("failed");
    } finally {
      setSaving(false);
    }
  }

  const hasPersistedSigningConfig = Boolean(
    persistedSignConfigRef.current.keystore_path && persistedSignConfigRef.current.key_alias,
  );
  const currentSigningConfigSaved =
    !signingDirty &&
    hasPersistedSigningConfig &&
    signingConfigEquals(config, persistedSignConfigRef.current) &&
    ksPass === persistedKsPassRef.current &&
    keyPass === persistedKeyPassRef.current;
  const signingValidated =
    validatedSigningRef.current !== null &&
    signingDraftEquals(
      {
        signConfig: config,
        keystorePassword: ksPass,
        keyPassword: keyPass,
      },
      validatedSigningRef.current,
    );

  async function toggleAutoSign(checked: boolean) {
    if (!currentSigningConfigSaved) {
      return;
    }

    const savedConfig = persistedSignConfigRef.current;
    const keystorePath = savedConfig.keystore_path?.trim() || "";
    const alias = savedConfig.key_alias?.trim() || "";
    const ksType = savedConfig.ks_type || "JKS";
    const savedKsPass = persistedKsPassRef.current;
    const savedKeyPass = persistedKeyPassRef.current;

    setAutoSignSaving(true);
    setError("");

    try {
      if (!keystorePath || !alias || !savedKsPass.trim()) {
        throw new Error(t(locale, "saveSignFirst"));
      }

      const list = await api.listKeystoreAliases(keystorePath, savedKsPass, ksType);
      if (!list.includes(alias)) {
        throw new Error(t(locale, "aliasFailed"));
      }

      const nextConfig = { ...savedConfig, auto_sign_enabled: checked };
      await persistConfig({
        locale: selectedLocale,
        theme_mode: selectedThemeMode,
        sign_config: nextConfig,
        keystore_password: savedKsPass,
        key_password: savedKeyPass,
      });
      setConfig((old) => ({ ...old, auto_sign_enabled: checked }));
    } catch (err) {
      setConfig((old) => ({
        ...old,
        auto_sign_enabled: persistedSignConfigRef.current.auto_sign_enabled,
      }));
      setError(String(err));
      setStatus("failed");
    } finally {
      setAutoSignSaving(false);
    }
  }

  function updateLocale(next: Locale) {
    setSelectedLocale(next);
    setLocale(next);
    setSaving(true);
    setError("");
    void persistConfig({
      locale: next,
      theme_mode: selectedThemeMode,
      sign_config: persistedSignConfigRef.current,
      keystore_password: persistedKsPassRef.current,
      key_password: persistedKeyPassRef.current,
    })
      .then(() => showSavedStatus())
      .catch((err) => {
        setError(String(err));
        setStatus("failed");
      })
      .finally(() => setSaving(false));
  }

  function updateThemeMode(next: ThemeMode) {
    setSelectedThemeMode(next);
    setThemeMode(next);
    setSaving(true);
    setError("");
    void persistConfig({
      locale: selectedLocale,
      theme_mode: next,
      sign_config: persistedSignConfigRef.current,
      keystore_password: persistedKsPassRef.current,
      key_password: persistedKeyPassRef.current,
    })
      .then(() => showSavedStatus())
      .catch((err) => {
        setError(String(err));
        setStatus("failed");
      })
      .finally(() => setSaving(false));
  }

  return {
    selectedLocale,
    selectedThemeMode,
    config,
    setConfig: updateSignConfig,
    ksPass,
    setKsPass: updateKeystorePassword,
    keyPass,
    setKeyPass: updateKeyPassword,
    showPass,
    setShowPass,
    aliases,
    saving,
    autoSignSaving,
    detecting,
    status,
    error,
    updateLocale,
    updateThemeMode,
    browseKeystore,
    detectAlias,
    validateSigningConfig,
    saveSigningConfig,
    toggleAutoSign,
    signingValidated,
    signingConfigured: currentSigningConfigSaved,
  };
}
