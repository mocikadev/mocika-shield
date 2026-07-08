import { Loader2, Save } from "lucide-react";
import { SettingsSigningPanel } from "@/components/app/settings-signing-panel";
import {
  AppButton,
  PillSegment,
  SettingsFieldRow,
  SettingsGroup,
  StatusMessage,
} from "@/components/app/common";
import { useSettingsForm, type SettingsSavePayload } from "@/hooks/use-settings-form";
import { Switch } from "@/components/ui/switch";
import { t, type Locale } from "@/lib/i18n";
import type { SignConfig, ThemeMode } from "@/lib/tauri";

export function SettingsPage({
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
  const {
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
    signingConfigured,
  } = useSettingsForm({
    locale,
    setLocale,
    themeMode,
    setThemeMode,
    signConfig,
    keystorePassword,
    keyPassword,
    onConfigSaved,
  });

  return (
    <section className="mx-auto max-w-[820px] px-8 py-9">
      <h1 className="mb-8 text-[22px] font-semibold">{t(locale, "settingsTitle")}</h1>
      <div className="space-y-8">
        <SettingsGroup title={t(locale, "appearance")}>
          <SettingsFieldRow label={t(locale, "theme")}>
            <div className="flex justify-end">
              <PillSegment
                value={selectedThemeMode}
                onChange={updateThemeMode}
                options={[
                  { value: "system", label: t(locale, "system") },
                  { value: "light", label: t(locale, "light") },
                  { value: "dark", label: t(locale, "dark") },
                ]}
              />
            </div>
          </SettingsFieldRow>
          <SettingsFieldRow label={t(locale, "language")}>
            <div className="flex justify-end">
              <PillSegment
                value={selectedLocale}
                onChange={updateLocale}
                options={[
                  { value: "zh", label: "中文" },
                  { value: "en", label: "English" },
                ]}
              />
            </div>
          </SettingsFieldRow>
        </SettingsGroup>

        <SettingsSigningPanel
          locale={locale}
          config={config}
          setConfig={setConfig}
          ksPass={ksPass}
          setKsPass={setKsPass}
          keyPass={keyPass}
          setKeyPass={setKeyPass}
          showPass={showPass}
          setShowPass={setShowPass}
          aliases={aliases}
          detecting={detecting}
          onBrowseKeystore={browseKeystore}
          onDetectAlias={detectAlias}
        />

        <SettingsGroup title={t(locale, "protectSection")}>
          <SettingsFieldRow label={t(locale, "autoSignAfterProtect")} hint={signingConfigured ? t(locale, "ready") : t(locale, "noSavedConfig")}>
            <div className="flex justify-end">
              <Switch
                checked={Boolean(config.auto_sign_enabled)}
                onCheckedChange={(checked) => setConfig((old) => ({ ...old, auto_sign_enabled: checked }))}
                aria-label={t(locale, "autoSignAfterProtect")}
              />
            </div>
          </SettingsFieldRow>
        </SettingsGroup>

        <div className="space-y-3">
          {status === "saved" && <StatusMessage kind="success">{t(locale, "saved")}</StatusMessage>}
          {status === "failed" && <StatusMessage kind="error">{error || t(locale, "saveFailed")}</StatusMessage>}
          {error && status !== "failed" && <StatusMessage kind="error">{error}</StatusMessage>}
          <div className="flex justify-end">
            <AppButton onClick={save} disabled={saving}>
              {saving ? <Loader2 className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}
              {t(locale, "save")}
            </AppButton>
          </div>
        </div>
      </div>
    </section>
  );
}
