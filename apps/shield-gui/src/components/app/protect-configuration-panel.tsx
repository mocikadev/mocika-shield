import { FolderKey, FolderOpen, Play, Settings2 } from "lucide-react";
import { AppButton, SelectInput } from "@/components/app/common";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
import type { EnvironmentPolicy, RuntimeMode } from "@/hooks/use-protect-workflow";
import { t, type Locale } from "@/lib/i18n";
import type { CertificateRecord } from "@/lib/tauri";

export function ProtectConfigurationPanel({
  locale,
  disabled,
  startDisabled,
  runtimeMode,
  environmentPolicy,
  signAfterProtect,
  selectedCertificateId,
  certificates,
  outputDirectoryMode,
  fixedOutputDirectory,
  onRuntimeModeChange,
  onEnvironmentPolicyChange,
  onSignAfterProtectChange,
  onCertificateChange,
  onOutputDirectoryModeChange,
  onChooseDirectory,
  onOpenCertificates,
  onRestoreRecommended,
  onStart,
}: {
  locale: Locale;
  disabled: boolean;
  startDisabled: boolean;
  runtimeMode: RuntimeMode;
  environmentPolicy: EnvironmentPolicy;
  signAfterProtect: boolean;
  selectedCertificateId: string;
  certificates: CertificateRecord[];
  outputDirectoryMode: "source" | "fixed";
  fixedOutputDirectory: string;
  onRuntimeModeChange: (value: RuntimeMode) => void;
  onEnvironmentPolicyChange: (value: EnvironmentPolicy) => void;
  onSignAfterProtectChange: (value: boolean) => void;
  onCertificateChange: (value: string) => void;
  onOutputDirectoryModeChange: (value: "source" | "fixed") => void;
  onChooseDirectory: () => void;
  onOpenCertificates: () => void;
  onRestoreRecommended: () => void;
  onStart: () => void;
}) {
  return (
    <aside className="rounded-[14px] border bg-card p-5">
      <div className="flex items-center justify-between gap-3">
        <h2 className="text-sm font-semibold">{t(locale, "protectPlan")}</h2>
        <Sheet>
          <SheetTrigger asChild>
            <AppButton size="sm" variant="ghost" disabled={disabled}>
              <Settings2 className="h-4 w-4" />
              {t(locale, "modifySettings")}
            </AppButton>
          </SheetTrigger>
          <SheetContent className="w-full overflow-y-auto sm:max-w-md">
            <SheetHeader>
              <SheetTitle>{t(locale, "protectSettings")}</SheetTitle>
              <SheetDescription>{t(locale, "protectSettingsHint")}</SheetDescription>
            </SheetHeader>
            <div className="mt-7 space-y-6">
              <SettingSelect label={t(locale, "targetSystem")} value={runtimeMode} onChange={(value) => onRuntimeModeChange(value as RuntimeMode)} hint={runtimeMode === "android_api19" ? t(locale, "runtimeAndroid44Hint") : t(locale, "runtimeStandardHint")}>
                <option value="standard">{t(locale, "runtimeStandard")}</option>
                <option value="android_api19">{t(locale, "runtimeAndroid44")}</option>
              </SettingSelect>
              <SettingSelect label={t(locale, "environmentPolicy")} value={environmentPolicy} onChange={(value) => onEnvironmentPolicyChange(value as EnvironmentPolicy)} hint={environmentPolicy === "strict" ? t(locale, "environmentStrictHint") : t(locale, "environmentCompatibleHint")}>
                <option value="compatible">{t(locale, "environmentCompatible")}</option>
                <option value="strict">{t(locale, "environmentStrict")}</option>
              </SettingSelect>
              <div className="rounded-xl border p-4">
                <label className="flex items-center justify-between gap-4 text-sm font-medium">
                  <span>{t(locale, "signAfterProtect")}</span>
                  <input type="checkbox" className="h-4 w-4 accent-primary" checked={signAfterProtect} onChange={(event) => onSignAfterProtectChange(event.target.checked)} />
                </label>
                {signAfterProtect && (
                  <div className="mt-4 space-y-3">
                    <SettingSelect label={t(locale, "selectCertificate")} value={selectedCertificateId} onChange={onCertificateChange}>
                      {certificates.length === 0 ? <option value="">{t(locale, "noCertificates")}</option> : certificates.map((item) => (
                        <option key={item.id} value={item.id}>{item.is_default ? `${item.name} · ${t(locale, "defaultCertificate")}` : item.name}</option>
                      ))}
                    </SettingSelect>
                    {certificates.length === 0 && (
                      <AppButton className="w-full" variant="secondary" onClick={onOpenCertificates}>
                        <FolderKey className="h-4 w-4" />{t(locale, "navCertificates")}
                      </AppButton>
                    )}
                  </div>
                )}
              </div>
              <SettingSelect label={t(locale, "saveLocation")} value={outputDirectoryMode} onChange={(value) => onOutputDirectoryModeChange(value as "source" | "fixed")}>
                <option value="source">{t(locale, "sourceDirectory")}</option>
                <option value="fixed">{t(locale, "fixedDirectory")}</option>
              </SettingSelect>
              {outputDirectoryMode === "fixed" && (
                <div>
                  <div className="path-text mb-2 rounded-xl border bg-muted/40 p-3">{fixedOutputDirectory || t(locale, "directoryNotSelected")}</div>
                  <AppButton className="w-full" variant="secondary" onClick={onChooseDirectory}>
                    <FolderOpen className="h-4 w-4" />{t(locale, "chooseDirectory")}
                  </AppButton>
                </div>
              )}
              <AppButton className="w-full" variant="ghost" onClick={onRestoreRecommended}>
                {t(locale, "restoreRecommended")}
              </AppButton>
            </div>
          </SheetContent>
        </Sheet>
      </div>
      <dl className="mt-5 space-y-3 text-sm">
        <Summary label={t(locale, "targetSystem")} value={runtimeMode === "android_api19" ? t(locale, "runtimeAndroid44") : t(locale, "runtimeStandard")} hint={runtimeMode === "android_api19" ? t(locale, "runtimeAndroid44Summary") : t(locale, "runtimeStandardSummary")} />
        <Summary label={t(locale, "environmentPolicy")} value={environmentPolicy === "strict" ? t(locale, "environmentStrict") : t(locale, "environmentCompatible")} hint={environmentPolicy === "strict" ? t(locale, "environmentStrictSummary") : t(locale, "environmentCompatibleSummary")} />
        <Summary label={t(locale, "signAfterProtect")} value={signAfterProtect ? t(locale, "enabled") : t(locale, "disabled")} />
      </dl>
      <AppButton className="mt-6 w-full" disabled={startDisabled} onClick={onStart}>
        <Play className="h-4 w-4" />{t(locale, "startProtect")}
      </AppButton>
    </aside>
  );
}

function SettingSelect({ label, value, onChange, hint, children }: { label: string; value: string; onChange: (value: string) => void; hint?: string; children: React.ReactNode }) {
  return <div><label className="field-label">{label}</label><SelectInput className="mt-2" value={value} onChange={(event) => onChange(event.target.value)}>{children}</SelectInput>{hint && <p className="mt-2 text-xs leading-5 text-muted-foreground">{hint}</p>}</div>;
}

function Summary({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return <div className="border-b pb-3 last:border-b-0 last:pb-0"><div className="flex items-start justify-between gap-3"><dt className="text-muted-foreground">{label}</dt><dd className="text-right font-medium">{value}</dd></div>{hint && <p className="mt-1.5 text-xs leading-5 text-muted-foreground">{hint}</p>}</div>;
}
