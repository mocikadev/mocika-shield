import { Clipboard, FolderKey, FolderOpen, Loader2, Play, RotateCcw, Square } from "lucide-react";
import { AppButton, DropZone, SelectedApkCard, StatusMessage } from "@/components/app/common";
import { ProtectProgressPanel } from "@/components/app/protect-progress-panel";
import { useClipboard } from "@/hooks/use-clipboard";
import { useProtectWorkflow } from "@/hooks/use-protect-workflow";
import { basename } from "@/lib/path";
import { t, type Locale } from "@/lib/i18n";
import { api, type BuildInfo, type CertificateRecord } from "@/lib/tauri";

export function ProtectPage({
  locale,
  defaultCertificate,
  certificatesLoaded,
  buildInfo,
  runtimeInfoLoaded,
  onOpenCertificates,
}: {
  locale: Locale;
  defaultCertificate: CertificateRecord | null;
  certificatesLoaded: boolean;
  buildInfo: BuildInfo | null;
  runtimeInfoLoaded: boolean;
  onOpenCertificates: () => void;
}) {
  const { copiedLabel, copy } = useClipboard(locale);
  const {
    input,
    output,
    state,
    dragActive,
    warning,
    error,
    precheck,
    currentStep,
    messages,
    autoSignReady,
    steps,
    hasInput,
    showProgress,
    browse,
    start,
    cancel,
    resetSelection,
  } = useProtectWorkflow({
    locale,
    certificate: defaultCertificate,
    buildInfo,
  });

  return (
    <section className="min-h-full px-10 py-9">
      {!hasInput ? (
        <div className="mx-auto w-full max-w-5xl">
          <h1 className="text-[28px] font-semibold tracking-normal">{t(locale, "protectTitle")}</h1>
          <div className="mt-16">
            <DropZone
              locale={locale}
              active={dragActive}
              title={t(locale, "dropApk")}
              subtitle={t(locale, "onlyApk")}
              onBrowse={browse}
            />
          </div>
          {certificatesLoaded && !defaultCertificate && (
            <div className="mt-5">
              <StatusMessage
                kind="info"
                action={
                  <AppButton size="sm" variant="secondary" onClick={onOpenCertificates}>
                    <FolderKey className="h-4 w-4" />
                    {t(locale, "navCertificates")}
                  </AppButton>
                }
              >
                {t(locale, "noDefaultCertificate")}
              </StatusMessage>
            </div>
          )}
          {warning && (
            <div className="mt-5">
              <StatusMessage kind="warning">{warning}</StatusMessage>
            </div>
          )}
        </div>
      ) : (
        <div className="mx-auto w-full max-w-6xl">
          <div className="flex flex-wrap items-center justify-between gap-4">
            <div className="min-w-0">
              <h1 className="text-[28px] font-semibold tracking-normal">{t(locale, "protectTitle")}</h1>
              <p className="mt-1 truncate text-sm text-muted-foreground">{basename(input)}</p>
            </div>
            {state === "running" ? (
              <AppButton className="min-w-[136px]" variant="danger" onClick={cancel}>
                <Square className="h-4 w-4" />
                {t(locale, "cancel")}
              </AppButton>
            ) : state === "done" || state === "failed" ? (
              <AppButton className="min-w-[136px]" variant="secondary" onClick={resetSelection}>
                <RotateCcw className="h-4 w-4" />
                {t(locale, "protectAnother")}
              </AppButton>
            ) : (
              <AppButton
                className="min-w-[136px]"
                disabled={!input || !runtimeInfoLoaded || Boolean(precheck) || state === "prechecking"}
                onClick={start}
              >
                {state === "prechecking" ? <Loader2 className="h-4 w-4 animate-spin" /> : <Play className="h-4 w-4" />}
                {state === "prechecking" ? t(locale, "prechecking") : !runtimeInfoLoaded ? t(locale, "checkingEnvironment") : t(locale, "startProtect")}
              </AppButton>
            )}
          </div>

          <div className="mt-8 grid gap-5 lg:grid-cols-[minmax(0,1fr)_320px]">
            <div className="space-y-4">
              <SelectedApkCard
                locale={locale}
                path={input}
                output={output}
                disabled={state === "prechecking" || state === "running"}
                onChange={browse}
              />
              {warning && <StatusMessage kind="warning">{warning}</StatusMessage>}
              {hasInput && autoSignReady && defaultCertificate && (
                <StatusMessage kind="info">
                  <b>{defaultCertificate.name}</b>
                  <span className="ml-1">{t(locale, "certificateWillAutoSign")}</span>
                </StatusMessage>
              )}
              {hasInput && !autoSignReady && (
                <StatusMessage
                  kind="info"
                  action={
                    <AppButton size="sm" variant="secondary" onClick={onOpenCertificates}>
                      <FolderKey className="h-4 w-4" />
                      {t(locale, "navCertificates")}
                    </AppButton>
                  }
                >
                  {t(locale, "noDefaultCertificate")}
                </StatusMessage>
              )}
              {precheck && (
                <StatusMessage kind="error">
                  <b>{t(locale, "precheckFailed")}：</b>
                  {precheck}
                </StatusMessage>
              )}
              {state === "done" && (
                <StatusMessage
                  kind="success"
                  action={
                    <AppButton size="sm" variant="secondary" onClick={() => void api.showInFolder(output)}>
                      <FolderOpen className="h-4 w-4" />
                      {t(locale, "showInFolder")}
                    </AppButton>
                  }
                >
                  {t(locale, "done")}
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
                  <b>{t(locale, "errorDetail")}：</b>
                  {error}
                </StatusMessage>
              )}
            </div>
            <ProtectProgressPanel
              locale={locale}
              state={state}
              currentStep={currentStep}
              steps={steps}
              messages={messages}
              showProgress={showProgress}
            />
          </div>
        </div>
      )}
    </section>
  );
}
