import { useEffect, useMemo, useRef, useState } from "react";
import { Clipboard, FolderOpen, RotateCcw, Square } from "lucide-react";
import { AppButton, DropZone, SelectedApkCard, StatusMessage, TextInput } from "@/components/app/common";
import { ProtectConfigurationPanel } from "@/components/app/protect-configuration-panel";
import { PreflightSummary } from "@/components/app/preflight-summary";
import { ProtectProgressPanel } from "@/components/app/protect-progress-panel";
import { useClipboard } from "@/hooks/use-clipboard";
import { useProtectWorkflow } from "@/hooks/use-protect-workflow";
import { basename } from "@/lib/path";
import { t, type Locale } from "@/lib/i18n";
import { api, openDirectoryDialog, type BuildInfo, type CertificateRecord, type ProtectDefaults } from "@/lib/tauri";

export function ProtectPage({
  active,
  locale,
  certificates,
  defaultCertificate,
  certificatesLoaded,
  buildInfo,
  runtimeInfoLoaded,
  configLoaded,
  protectDefaults,
  onProtectDefaultsChange,
  onOpenCertificates,
}: {
  active: boolean;
  locale: Locale;
  certificates: CertificateRecord[];
  defaultCertificate: CertificateRecord | null;
  certificatesLoaded: boolean;
  buildInfo: BuildInfo | null;
  runtimeInfoLoaded: boolean;
  configLoaded: boolean;
  protectDefaults: ProtectDefaults;
  onProtectDefaultsChange: (defaults: ProtectDefaults) => void;
  onOpenCertificates: () => void;
}) {
  const { copiedLabel, copy } = useClipboard(locale);
  const [signAfterProtect, setSignAfterProtect] = useState(false);
  const [selectedCertificateId, setSelectedCertificateId] = useState("");
  const initialized = useRef(false);
  const certificateInitialized = useRef(false);

  useEffect(() => {
    if (!configLoaded || !certificatesLoaded || initialized.current) return;
    initialized.current = true;
    setSignAfterProtect(protectDefaults.sign_after_protect ?? Boolean(defaultCertificate?.auto_sign_enabled));
  }, [certificatesLoaded, configLoaded, defaultCertificate, protectDefaults.sign_after_protect]);

  useEffect(() => {
    if (!configLoaded || !certificatesLoaded) return;
    setSelectedCertificateId((current) => {
      if (certificateInitialized.current && certificates.some((item) => item.id === current)) {
        return current;
      }
      certificateInitialized.current = true;
      const preferred = protectDefaults.certificate_id;
      if (preferred && certificates.some((item) => item.id === preferred)) {
        return preferred;
      }
      return defaultCertificate?.id ?? certificates[0]?.id ?? "";
    });
  }, [certificates, certificatesLoaded, configLoaded, defaultCertificate, protectDefaults.certificate_id]);

  const signingCertificate = useMemo(
    () => signAfterProtect ? certificates.find((item) => item.id === selectedCertificateId) ?? null : null,
    [certificates, selectedCertificateId, signAfterProtect],
  );
  const workflow = useProtectWorkflow({ active, locale, certificate: signingCertificate, buildInfo, defaults: protectDefaults });
  const locked = workflow.taskLocked || workflow.state === "prechecking" || workflow.state === "running";

  function updateDefaults(patch: Partial<ProtectDefaults>) {
    onProtectDefaultsChange({ ...protectDefaults, ...patch });
  }

  function updateSignAfterProtect(value: boolean) {
    setSignAfterProtect(value);
    updateDefaults({ sign_after_protect: value });
  }

  function restoreRecommended() {
    const recommended: ProtectDefaults = {
      runtime_mode: "standard",
      environment_policy: "compatible",
      sign_after_protect: null,
      certificate_id: null,
      output_directory_mode: "source",
      fixed_output_directory: "",
    };
    setSignAfterProtect(Boolean(defaultCertificate?.auto_sign_enabled));
    onProtectDefaultsChange(recommended);
  }

  async function chooseOutputDirectory() {
    const path = await openDirectoryDialog(workflow.fixedOutputDirectory || workflow.outputDirectory);
    if (!path) return;
    workflow.setFixedOutputDirectory(path);
    updateDefaults({ output_directory_mode: "fixed", fixed_output_directory: path });
  }

  const filenameMessage = workflow.outputFilenameError === "empty"
    ? t(locale, "outputFilenameRequired")
    : workflow.outputFilenameError === "invalid" ? t(locale, "outputFilenameInvalid") : "";
  const startDisabled = !runtimeInfoLoaded || Boolean(workflow.precheck) || workflow.state === "prechecking"
    || workflow.preflight?.verdict === "blocked"
    || Boolean(workflow.outputFilenameError) || (signAfterProtect && !signingCertificate)
    || (workflow.outputDirectoryMode === "fixed" && !workflow.fixedOutputDirectory);

  return (
    <section className="min-h-full px-10 py-9">
      {!workflow.hasInput ? (
        <div className="mx-auto w-full max-w-5xl">
          <h1 className="text-[28px] font-semibold">{t(locale, "protectTitle")}</h1>
          <div className="mt-16">
            <DropZone locale={locale} active={workflow.dragActive} title={t(locale, "dropApk")} subtitle={t(locale, "onlyApk")} onBrowse={workflow.browse} />
          </div>
          {workflow.warning && <div className="mt-5"><StatusMessage kind="warning">{workflow.warning}</StatusMessage></div>}
        </div>
      ) : (
        <div className="mx-auto w-full max-w-6xl">
          <div className="flex flex-wrap items-center justify-between gap-4">
            <div className="min-w-0"><h1 className="text-[28px] font-semibold">{t(locale, "protectTitle")}</h1><p className="mt-1 truncate text-sm text-muted-foreground">{basename(workflow.input)}</p></div>
            {workflow.state === "running" ? (
              <AppButton className="min-w-[136px]" variant="danger" onClick={workflow.cancel}><Square className="h-4 w-4" />{t(locale, "cancel")}</AppButton>
            ) : (workflow.state === "done" || workflow.state === "failed") && (
              <AppButton className="min-w-[136px]" variant="secondary" onClick={workflow.resetSelection}><RotateCcw className="h-4 w-4" />{t(locale, "protectAnother")}</AppButton>
            )}
          </div>

          <div className="mt-8 grid gap-5 lg:grid-cols-[minmax(0,1fr)_340px]">
            <div className="space-y-4">
              <SelectedApkCard locale={locale} path={workflow.input} disabled={locked} onChange={workflow.browse} />
              <div className="rounded-[14px] border bg-card p-4">
                <label className="field-label" htmlFor="protect-output-name">{t(locale, "outputFilename")}</label>
                <TextInput id="protect-output-name" className="mt-2 font-mono" value={workflow.outputFilename} disabled={locked} onChange={(event) => workflow.setOutputFilename(event.target.value)} />
                {filenameMessage && <p className="mt-2 text-xs text-destructive">{filenameMessage}</p>}
                <div className="mt-3 flex items-start justify-between gap-3 rounded-xl bg-muted/50 p-3">
                  <div className="min-w-0"><div className="text-xs font-medium text-muted-foreground">{t(locale, "saveLocation")}</div><div className="path-text mt-1">{workflow.outputDirectory}</div></div>
                  {!locked && <AppButton size="sm" variant="secondary" onClick={() => void chooseOutputDirectory()}><FolderOpen className="h-4 w-4" />{t(locale, "change")}</AppButton>}
                </div>
              </div>
              <PreflightSummary locale={locale} loading={workflow.state === "prechecking"} report={workflow.preflight} />
              {workflow.warning && <StatusMessage kind="warning">{workflow.warning}</StatusMessage>}
              {workflow.precheck && <StatusMessage kind="error"><b>{t(locale, "precheckFailed")}：</b>{workflow.precheck}</StatusMessage>}
              {workflow.state === "done" && <StatusMessage kind="success" action={<AppButton size="sm" variant="secondary" onClick={() => void api.showInFolder(workflow.output)}><FolderOpen className="h-4 w-4" />{t(locale, "showInFolder")}</AppButton>}>{t(locale, "done")}</StatusMessage>}
              {workflow.error && <StatusMessage kind="error" action={<AppButton size="sm" variant="secondary" onClick={() => void copy(workflow.error)}><Clipboard className="h-4 w-4" />{copiedLabel}</AppButton>}><b>{t(locale, "errorDetail")}：</b>{workflow.error}</StatusMessage>}
            </div>
            {workflow.showProgress ? (
              <ProtectProgressPanel locale={locale} state={workflow.state} currentStep={workflow.currentStep} steps={workflow.steps} showProgress startedAt={workflow.startedAt} finishedAt={workflow.finishedAt} />
            ) : (
              <ProtectConfigurationPanel
                locale={locale} disabled={locked} startDisabled={startDisabled}
                runtimeMode={workflow.runtimeMode} environmentPolicy={workflow.environmentPolicy}
                signAfterProtect={signAfterProtect} selectedCertificateId={selectedCertificateId} certificates={certificates}
                outputDirectoryMode={workflow.outputDirectoryMode} fixedOutputDirectory={workflow.fixedOutputDirectory}
                onRuntimeModeChange={(value) => { workflow.setRuntimeMode(value); updateDefaults({ runtime_mode: value }); }}
                onEnvironmentPolicyChange={(value) => { workflow.setEnvironmentPolicy(value); updateDefaults({ environment_policy: value }); }}
                onSignAfterProtectChange={updateSignAfterProtect} onCertificateChange={(value) => { setSelectedCertificateId(value); updateDefaults({ certificate_id: value || null }); }}
                onOutputDirectoryModeChange={(value) => { workflow.setOutputDirectoryMode(value); updateDefaults({ output_directory_mode: value }); }}
                onChooseDirectory={() => void chooseOutputDirectory()} onOpenCertificates={onOpenCertificates}
                onRestoreRecommended={restoreRecommended} onStart={() => void workflow.start()}
              />
            )}
          </div>
        </div>
      )}
    </section>
  );
}
