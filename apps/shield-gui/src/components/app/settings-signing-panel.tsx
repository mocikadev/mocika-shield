import type React from "react";
import { Check, Loader2 } from "lucide-react";
import { basename } from "@/lib/path";
import { t, type Locale } from "@/lib/i18n";
import type { SignConfig } from "@/lib/tauri";
import {
  AppButton,
  PasswordControl,
  SelectInput,
  SettingsFieldRow,
  SettingsGroup,
  StatusMessage,
  TextInput,
} from "@/components/app/common";

const successBadgeClass =
  "inline-flex h-5 w-5 items-center justify-center rounded-full border border-emerald-300/70 bg-emerald-500 text-white shadow-sm dark:border-emerald-300/30 dark:bg-emerald-400";

export function SettingsSigningPanel({
  locale,
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
  runtimeInfoLoaded,
  status,
  error,
  signingValidated,
  signingConfigured,
  onBrowseKeystore,
  onDetectAlias,
  onValidate,
  onSave,
}: {
  locale: Locale;
  config: SignConfig;
  setConfig: React.Dispatch<React.SetStateAction<SignConfig>>;
  ksPass: string;
  setKsPass: (value: string) => void;
  keyPass: string;
  setKeyPass: (value: string) => void;
  showPass: boolean;
  setShowPass: (show: boolean) => void;
  aliases: string[];
  saving: boolean;
  detecting: boolean;
  runtimeInfoLoaded: boolean;
  status: "idle" | "validating" | "validated" | "saved" | "failed";
  error: string;
  signingValidated: boolean;
  signingConfigured: boolean;
  onBrowseKeystore: () => void;
  onDetectAlias: () => void;
  onValidate: () => void;
  onSave: () => void;
}) {
  return (
    <SettingsGroup title={t(locale, "defaultSignConfig")}>
      {status === "failed" && (
        <div className="border-b border-border/60 px-6 py-4">
          <StatusMessage kind="error">{error || t(locale, "saveFailed")}</StatusMessage>
        </div>
      )}
      <SettingsFieldRow label={t(locale, "keystore")}>
        <div className="grid min-w-0 gap-3 sm:w-[520px] sm:grid-cols-[minmax(0,1fr)_108px] sm:items-center">
          <div className="flex min-w-0 items-center gap-3 rounded-[14px] border border-border/70 bg-muted/35 px-3.5 py-2.5 shadow-sm">
            <span className="min-w-0 flex-1 truncate font-mono text-sm text-foreground" title={config.keystore_path || undefined}>
              {config.keystore_path ? basename(config.keystore_path) : t(locale, "unknown")}
            </span>
            {saving && status === "validating" ? (
              <Loader2 className="h-[18px] w-[18px] shrink-0 animate-spin text-muted-foreground" />
            ) : signingConfigured ? (
              <span className={successBadgeClass}>
                <Check className="h-3.5 w-3.5" />
              </span>
            ) : null}
          </div>
          <AppButton className="shrink-0" size="sm" variant="secondary" onClick={onBrowseKeystore}>
            {t(locale, "browse")}...
          </AppButton>
        </div>
      </SettingsFieldRow>

      <SettingsFieldRow label={t(locale, "keystoreType")}>
        <div className="w-full sm:w-[240px]">
          <SelectInput
            id="settings-kstype"
            value={config.ks_type ?? "JKS"}
            onChange={(e) => setConfig((old) => ({ ...old, ks_type: e.target.value }))}
          >
            <option value="JKS">JKS</option>
            <option value="PKCS12">PKCS12</option>
          </SelectInput>
        </div>
      </SettingsFieldRow>

      <SettingsFieldRow label={t(locale, "keyAlias")}>
        <div className="grid min-w-0 gap-3 sm:w-[520px] sm:grid-cols-[minmax(0,1fr)_136px]">
          <TextInput
            id="settings-alias"
            value={config.key_alias ?? ""}
            onChange={(e) => setConfig((old) => ({ ...old, key_alias: e.target.value }))}
          />
          <AppButton className="shrink-0 justify-center" variant="secondary" onClick={onDetectAlias} disabled={detecting || !runtimeInfoLoaded}>
            {detecting && <Loader2 className="h-4 w-4 animate-spin" />}
            {!runtimeInfoLoaded ? t(locale, "checkingEnvironment") : t(locale, "detectAlias")}
          </AppButton>
        </div>
      </SettingsFieldRow>

      {aliases.length > 1 && (
        <SettingsFieldRow label={t(locale, "keyAlias")}>
          <div className="w-full sm:w-[520px]">
            <SelectInput
              value={config.key_alias ?? ""}
              onChange={(e) => setConfig((old) => ({ ...old, key_alias: e.target.value }))}
            >
              <option value="">{t(locale, "keyAlias")}</option>
              {aliases.map((item) => (
                <option key={item} value={item}>
                  {item}
                </option>
              ))}
            </SelectInput>
          </div>
        </SettingsFieldRow>
      )}

      <SettingsFieldRow label={t(locale, "keystorePassword")}>
        <div className="w-full sm:w-[520px]">
          <PasswordControl
            id="settings-kspass"
            label={t(locale, "keystorePassword")}
            value={ksPass}
            onChange={setKsPass}
            show={showPass}
            setShow={setShowPass}
          />
        </div>
      </SettingsFieldRow>

      <SettingsFieldRow label={t(locale, "keyPassword")} hint={t(locale, "keyPasswordHint")}>
        <div className="w-full sm:w-[520px]">
          <PasswordControl
            id="settings-keypass"
            label={t(locale, "keyPassword")}
            value={keyPass}
            onChange={setKeyPass}
            show={showPass}
            setShow={setShowPass}
          />
        </div>
      </SettingsFieldRow>

      <SettingsFieldRow label={t(locale, "signVersions")}>
        <div className="grid min-w-0 grid-cols-2 gap-2 sm:w-[520px] sm:grid-cols-4">
          {(["v1", "v2", "v3", "v4"] as const).map((version) => {
            const key = `sign_${version}` as keyof SignConfig;
            return (
              <label
                key={version}
                className="flex min-h-10 items-center gap-2 rounded-xl border border-border/70 bg-muted/30 px-3.5 text-sm font-semibold shadow-sm"
              >
                <input
                  type="checkbox"
                  className="h-5 w-5 accent-primary"
                  checked={Boolean(config[key])}
                  onChange={(e) => setConfig((old) => ({ ...old, [key]: e.target.checked }))}
                />
                {version.toUpperCase()}
              </label>
            );
          })}
        </div>
      </SettingsFieldRow>

      <SettingsFieldRow label="">
        <div className="flex flex-wrap justify-end gap-3 sm:w-[520px]">
          <AppButton
            className="min-w-[120px] justify-center"
            variant="secondary"
            onClick={onValidate}
            disabled={saving || detecting || !runtimeInfoLoaded}
          >
            {saving && status === "validating" ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : signingValidated ? (
              <span className={successBadgeClass}>
                <Check className="h-3.5 w-3.5" />
              </span>
            ) : null}
            {!runtimeInfoLoaded ? t(locale, "checkingEnvironment") : t(locale, "validate")}
          </AppButton>
          <AppButton
            className="min-w-[120px] justify-center"
            variant="secondary"
            onClick={onSave}
            disabled={saving || !signingValidated}
          >
            {signingConfigured ? (
              <span className={successBadgeClass}>
                <Check className="h-3.5 w-3.5" />
              </span>
            ) : null}
            {t(locale, "save")}
          </AppButton>
        </div>
      </SettingsFieldRow>
    </SettingsGroup>
  );
}
