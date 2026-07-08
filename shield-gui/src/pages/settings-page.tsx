import { SettingsSigningPanel } from "@/components/app/settings-signing-panel";
import {
  PillSegment,
  SettingsFieldRow,
  SettingsGroup,
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
    <section className="mx-auto max-w-[920px] px-8 py-10">
      <header className="mb-9">
        <h1 className="text-[24px] font-semibold tracking-normal">{t(locale, "settingsTitle")}</h1>
      </header>

      <div className="space-y-10">
        <SettingsGroup title={t(locale, "appearance")}>
          <SettingsFieldRow label={t(locale, "theme")}>
            <div className="flex flex-wrap justify-end">
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
            <div className="flex flex-wrap justify-end">
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
          saving={saving}
          detecting={detecting}
          status={status}
          error={error}
          signingValidated={signingValidated}
          signingConfigured={signingConfigured}
          onBrowseKeystore={browseKeystore}
          onDetectAlias={detectAlias}
          onValidate={() => void validateSigningConfig()}
          onSave={() => void saveSigningConfig()}
        />

        <SettingsGroup title={t(locale, "protectSection")}>
          <SettingsFieldRow
            label={t(locale, "autoSignAfterProtect")}
            hint={!signingConfigured ? t(locale, "noSavedConfig") : undefined}
          >
            <div className="flex items-center justify-end gap-3">
              <span className="text-sm text-muted-foreground">
                {config.auto_sign_enabled ? t(locale, "enabled") : t(locale, "disabled")}
              </span>
              <Switch
                checked={Boolean(config.auto_sign_enabled)}
                disabled={!signingConfigured || autoSignSaving}
                onCheckedChange={(checked) => void toggleAutoSign(checked)}
                aria-label={t(locale, "autoSignAfterProtect")}
              />
            </div>
          </SettingsFieldRow>
        </SettingsGroup>
      </div>
    </section>
  );
}
