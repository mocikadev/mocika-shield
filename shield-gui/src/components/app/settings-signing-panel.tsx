import type React from "react";
import { BadgeCheck, Check, Loader2 } from "lucide-react";
import { basename } from "@/lib/path";
import { t, type Locale } from "@/lib/i18n";
import type { SignConfig } from "@/lib/tauri";
import {
  AppButton,
  PasswordControl,
  SelectInput,
  SettingsFieldRow,
  SettingsGroup,
  TextInput,
} from "@/components/app/common";

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
  detecting,
  onBrowseKeystore,
  onDetectAlias,
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
  detecting: boolean;
  onBrowseKeystore: () => void;
  onDetectAlias: () => void;
}) {
  return (
    <SettingsGroup title={t(locale, "defaultSignConfig")}>
      <SettingsFieldRow label={t(locale, "keystore")}>
        <div className="flex min-w-0 items-center justify-end gap-3">
          <span className="min-w-0 truncate font-mono text-sm text-foreground">
            {config.keystore_path ? basename(config.keystore_path) : t(locale, "unknown")}
          </span>
          {config.keystore_path && <Check className="h-5 w-5 shrink-0 text-success" />}
          <AppButton className="shrink-0" size="sm" variant="secondary" onClick={onBrowseKeystore}>
            {t(locale, "browse")}...
          </AppButton>
        </div>
      </SettingsFieldRow>

      <SettingsFieldRow label={t(locale, "keystoreType")}>
        <SelectInput
          id="settings-kstype"
          className="sm:max-w-[220px]"
          value={config.ks_type ?? "JKS"}
          onChange={(e) => setConfig((old) => ({ ...old, ks_type: e.target.value }))}
        >
          <option value="JKS">JKS</option>
          <option value="PKCS12">PKCS12</option>
        </SelectInput>
      </SettingsFieldRow>

      <SettingsFieldRow label={t(locale, "keyAlias")}>
        <div className="grid min-w-0 gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
          <TextInput
            id="settings-alias"
            value={config.key_alias ?? ""}
            onChange={(e) => setConfig((old) => ({ ...old, key_alias: e.target.value }))}
          />
          <AppButton className="shrink-0" variant="secondary" onClick={onDetectAlias} disabled={detecting}>
            {detecting ? <Loader2 className="h-4 w-4 animate-spin" /> : <BadgeCheck className="h-4 w-4" />}
            {detecting ? t(locale, "detecting") : t(locale, "detectAlias")}
          </AppButton>
        </div>
      </SettingsFieldRow>

      {aliases.length > 1 && (
        <SettingsFieldRow label={t(locale, "keyAlias")}>
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
        </SettingsFieldRow>
      )}

      <SettingsFieldRow label={t(locale, "keystorePassword")}>
        <PasswordControl
          id="settings-kspass"
          label={t(locale, "keystorePassword")}
          value={ksPass}
          onChange={setKsPass}
          show={showPass}
          setShow={setShowPass}
        />
      </SettingsFieldRow>

      <SettingsFieldRow label={t(locale, "keyPassword")} hint={t(locale, "keyPasswordHint")}>
        <PasswordControl
          id="settings-keypass"
          label={t(locale, "keyPassword")}
          value={keyPass}
          onChange={setKeyPass}
          show={showPass}
          setShow={setShowPass}
        />
      </SettingsFieldRow>

      <SettingsFieldRow label={t(locale, "signVersions")}>
        <div className="flex flex-wrap justify-end gap-4">
          {(["v1", "v2", "v3", "v4"] as const).map((version) => {
            const key = `sign_${version}` as keyof SignConfig;
            return (
              <label key={version} className="flex min-h-8 items-center gap-2 text-sm font-semibold">
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
    </SettingsGroup>
  );
}
