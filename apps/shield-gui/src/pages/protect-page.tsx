import { useEffect, useMemo, useRef, useState } from "react";
import { Clipboard, FolderKey, FolderOpen, Loader2, Play, RotateCcw, Square } from "lucide-react";
import { AppButton, DropZone, SelectInput, SelectedApkCard, StatusMessage, TextInput } from "@/components/app/common";
import { ProtectProgressPanel } from "@/components/app/protect-progress-panel";
import { useClipboard } from "@/hooks/use-clipboard";
import { useProtectWorkflow } from "@/hooks/use-protect-workflow";
import { basename } from "@/lib/path";
import { t, type Locale } from "@/lib/i18n";
import { api, type BuildInfo, type CertificateRecord } from "@/lib/tauri";

export function ProtectPage({
  active,
  locale,
  certificates,
  defaultCertificate,
  certificatesLoaded,
  buildInfo,
  runtimeInfoLoaded,
  onOpenCertificates,
}: {
  active: boolean;
  locale: Locale;
  certificates: CertificateRecord[];
  defaultCertificate: CertificateRecord | null;
  certificatesLoaded: boolean;
  buildInfo: BuildInfo | null;
  runtimeInfoLoaded: boolean;
  onOpenCertificates: () => void;
}) {
  const { copiedLabel, copy } = useClipboard(locale);
  const [signAfterProtect, setSignAfterProtect] = useState(false);
  const [selectedCertificateId, setSelectedCertificateId] = useState("");
  const initialized = useRef(false);

  useEffect(() => {
    if (!certificatesLoaded || initialized.current) return;
    initialized.current = true;
    setSignAfterProtect(Boolean(defaultCertificate?.auto_sign_enabled));
  }, [certificatesLoaded, defaultCertificate]);

  useEffect(() => {
    if (!certificates.length) { setSelectedCertificateId(""); return; }
    if (certificates.some((item) => item.id === selectedCertificateId)) return;
    setSelectedCertificateId(defaultCertificate?.id ?? certificates[0].id);
  }, [certificates, defaultCertificate, selectedCertificateId]);

  const signingCertificate = useMemo(
    () => signAfterProtect ? certificates.find((item) => item.id === selectedCertificateId) ?? null : null,
    [certificates, selectedCertificateId, signAfterProtect],
  );
  const {
    input,
    output,
    setOutput,
    state,
    dragActive,
    warning,
    error,
    precheck,
    runtimeMode,
    setRuntimeMode,
    nativeAbis,
    currentStep,
    startedAt,
    finishedAt,
    autoSignReady,
    activeCertificate,
    taskLocked,
    steps,
    hasInput,
    showProgress,
    browse,
    start,
    cancel,
    resetSelection,
  } = useProtectWorkflow({
    active,
    locale,
    certificate: signingCertificate,
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
          {certificatesLoaded && certificates.length === 0 && (
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
                disabled={!input || !runtimeInfoLoaded || Boolean(precheck) || state === "prechecking" || (signAfterProtect && !signingCertificate)}
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
                disabled={taskLocked || state === "prechecking" || state === "running"}
                onChange={browse}
              />
              <div className="rounded-[14px] border bg-card p-4">
                <label className="field-label" htmlFor="protect-output">{t(locale, "outputPath")}</label>
                <TextInput
                  id="protect-output"
                  className="mt-2 font-mono text-xs"
                  value={output}
                  disabled={taskLocked || state === "prechecking" || state === "running"}
                  onChange={(event) => setOutput(event.target.value)}
                />
              </div>
              <div className="rounded-[14px] border bg-card p-4">
                <label className="field-label" htmlFor="protect-runtime-mode">{t(locale, "runtimeCompatibility")}</label>
                <SelectInput
                  id="protect-runtime-mode"
                  className="mt-2"
                  value={runtimeMode}
                  disabled={taskLocked || state === "prechecking" || state === "running"}
                  onChange={(event) => setRuntimeMode(event.target.value as "standard" | "android_api19")}
                >
                  <option value="standard">{t(locale, "runtimeStandard")}</option>
                  <option value="android_api19">{t(locale, "runtimeAndroid44")}</option>
                </SelectInput>
                <p className="mt-2 text-xs text-muted-foreground">
                  {runtimeMode === "android_api19" ? t(locale, "runtimeAndroid44Hint") : t(locale, "runtimeStandardHint")}
                  {nativeAbis.length > 0 ? ` · ABI: ${nativeAbis.join(", ")}` : ""}
                </p>
              </div>
              <div className="rounded-[14px] border bg-card p-4">
                <label className="flex items-center justify-between gap-4 text-sm font-medium">
                  <span>{t(locale, "signAfterProtect")}</span>
                  <input
                    type="checkbox"
                    className="h-4 w-4 accent-primary"
                    checked={signAfterProtect}
                    disabled={taskLocked || state === "prechecking" || state === "running"}
                    onChange={(event) => setSignAfterProtect(event.target.checked)}
                  />
                </label>
                {signAfterProtect && (
                  <div className="mt-4">
                    <label className="field-label" htmlFor="protect-sign-certificate">{t(locale, "selectCertificate")}</label>
                    <SelectInput
                      id="protect-sign-certificate"
                      className="mt-2"
                      value={selectedCertificateId}
                      disabled={taskLocked || state === "prechecking" || state === "running"}
                      onChange={(event) => setSelectedCertificateId(event.target.value)}
                    >
                      {certificates.length === 0 ? <option value="">{t(locale, "noCertificates")}</option> : certificates.map((item) => (
                        <option key={item.id} value={item.id}>{item.is_default ? `${item.name} · ${t(locale, "defaultCertificate")}` : item.name}</option>
                      ))}
                    </SelectInput>
                  </div>
                )}
              </div>
              {warning && <StatusMessage kind="warning">{warning}</StatusMessage>}
              {hasInput && autoSignReady && activeCertificate && (
                <StatusMessage kind="info">
                  <b>{activeCertificate.name}</b>
                  <span className="ml-1">{t(locale, "certificateWillAutoSign")}</span>
                </StatusMessage>
              )}
              {hasInput && signAfterProtect && !signingCertificate && (
                <StatusMessage
                  kind="info"
                  action={
                    <AppButton size="sm" variant="secondary" onClick={onOpenCertificates}>
                      <FolderKey className="h-4 w-4" />
                      {t(locale, "navCertificates")}
                    </AppButton>
                  }
                >
                  {t(locale, "noCertificates")}
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
              showProgress={showProgress}
              startedAt={startedAt}
              finishedAt={finishedAt}
            />
          </div>
        </div>
      )}
    </section>
  );
}
