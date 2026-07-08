import { Clipboard, FolderOpen, KeyRound, Loader2, RotateCcw, Settings } from "lucide-react";
import { AppButton, DropZone, SelectedApkCard, StatusMessage, TextInput } from "@/components/app/common";
import { SignConfigSummaryCard } from "@/components/app/sign-config-summary-card";
import { useClipboard } from "@/hooks/use-clipboard";
import { useSignWorkflow } from "@/hooks/use-sign-workflow";
import { basename } from "@/lib/path";
import { t, type Locale } from "@/lib/i18n";
import { api, type BuildInfo, type SignConfig } from "@/lib/tauri";

export function SignPage({
  locale,
  signConfig,
  signConfigLoaded,
  buildInfo,
  runtimeInfoLoaded,
  onOpenSettings,
}: {
  locale: Locale;
  signConfig: SignConfig;
  signConfigLoaded: boolean;
  buildInfo: BuildInfo | null;
  runtimeInfoLoaded: boolean;
  onOpenSettings: () => void;
}) {
  const { copiedLabel, copy } = useClipboard(locale);
  const {
    apkPath,
    outputPath,
    setOutputPath,
    state,
    error,
    dragActive,
    savedReady,
    enabledVersions,
    hasApk,
    browseApk,
    sign,
    reset,
  } = useSignWorkflow({
    locale,
    signConfig,
    signConfigLoaded,
    buildInfo,
  });

  return (
    <section className="min-h-full px-10 py-9">
      {!hasApk ? (
        <div className="mx-auto w-full max-w-5xl">
          <h1 className="text-[28px] font-semibold tracking-normal">{t(locale, "signTitle")}</h1>
          <div className="mt-16">
            <DropZone
              locale={locale}
              active={dragActive}
              title={t(locale, "dropApk")}
              subtitle={t(locale, "onlyApk")}
              onBrowse={browseApk}
            />
          </div>
          {signConfigLoaded && !savedReady && (
            <div className="mt-5">
              <StatusMessage
                kind="warning"
                action={
                  <AppButton size="sm" variant="secondary" onClick={onOpenSettings}>
                    <Settings className="h-4 w-4" />
                    {t(locale, "navSettings")}
                  </AppButton>
                }
              >
                {t(locale, "noSavedConfig")}
              </StatusMessage>
            </div>
          )}
          {error && (
            <div className="mt-5">
              <StatusMessage kind="error">{error}</StatusMessage>
            </div>
          )}
        </div>
      ) : (
        <div className="mx-auto w-full max-w-6xl">
          <div className="flex flex-wrap items-center justify-between gap-4">
            <div className="min-w-0">
              <h1 className="text-[28px] font-semibold tracking-normal">{t(locale, "signTitle")}</h1>
              <p className="mt-1 truncate text-sm text-muted-foreground">{basename(apkPath)}</p>
            </div>
            <div className="flex flex-wrap gap-2">
              <AppButton disabled={!apkPath || !signConfigLoaded || !runtimeInfoLoaded || !savedReady || state === "signing"} onClick={sign}>
                {state === "signing" ? <Loader2 className="h-4 w-4 animate-spin" /> : <KeyRound className="h-4 w-4" />}
                {state === "signing" ? t(locale, "signing") : !runtimeInfoLoaded ? t(locale, "checkingEnvironment") : t(locale, "startSign")}
              </AppButton>
              {(state === "done" || state === "failed") && (
                <AppButton variant="secondary" onClick={reset}>
                  <RotateCcw className="h-4 w-4" />
                  {t(locale, "signAnother")}
                </AppButton>
              )}
            </div>
          </div>

          <div className="mt-8 grid gap-5 lg:grid-cols-[minmax(0,1fr)_320px]">
            <div className="space-y-4">
              <SelectedApkCard locale={locale} path={apkPath} disabled={state === "signing"} onChange={browseApk} />
              <div className="rounded-[14px] border bg-card p-4">
                <label className="field-label" htmlFor="sign-output">{t(locale, "outputPath")}</label>
                <TextInput
                  id="sign-output"
                  className="mt-2 font-mono text-xs"
                  value={outputPath}
                  onChange={(e) => setOutputPath(e.target.value)}
                />
              </div>
              {state === "done" && (
                <StatusMessage
                  kind="success"
                  action={
                    <AppButton size="sm" variant="secondary" onClick={() => void api.showInFolder(outputPath)}>
                      <FolderOpen className="h-4 w-4" />
                      {t(locale, "showInFolder")}
                    </AppButton>
                  }
                >
                  {t(locale, "signDone")}
                </StatusMessage>
              )}
              {error && (
                <StatusMessage
                  kind="error"
                  action={
                    <AppButton size="sm" variant="secondary" onClick={() => void copy(error)}>
                      <Clipboard className="h-4 w-4" />
                      {copiedLabel}
                    </AppButton>
                  }
                >
                  {error}
                </StatusMessage>
              )}
            </div>
            <SignConfigSummaryCard
              locale={locale}
              signConfig={signConfig}
              signConfigLoaded={signConfigLoaded}
              savedReady={savedReady}
              enabledVersions={enabledVersions}
              onOpenSettings={onOpenSettings}
            />
          </div>
        </div>
      )}
    </section>
  );
}
