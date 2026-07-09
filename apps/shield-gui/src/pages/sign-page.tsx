import { useEffect, useMemo, useState } from "react";
import { Clipboard, FolderKey, FolderOpen, KeyRound, Loader2, RotateCcw } from "lucide-react";
import { AppButton, DropZone, SelectInput, SelectedApkCard, StatusMessage, TextInput } from "@/components/app/common";
import { useClipboard } from "@/hooks/use-clipboard";
import { useSignWorkflow } from "@/hooks/use-sign-workflow";
import { basename } from "@/lib/path";
import { t, type Locale } from "@/lib/i18n";
import { api, type BuildInfo, type CertificateRecord } from "@/lib/tauri";

export function SignPage({
  locale,
  certificates,
  certificatesLoaded,
  buildInfo,
  runtimeInfoLoaded,
  onOpenCertificates,
}: {
  locale: Locale;
  certificates: CertificateRecord[];
  certificatesLoaded: boolean;
  buildInfo: BuildInfo | null;
  runtimeInfoLoaded: boolean;
  onOpenCertificates: () => void;
}) {
  const { copiedLabel, copy } = useClipboard(locale);
  const [selectedCertificateId, setSelectedCertificateId] = useState<string>("");

  useEffect(() => {
    if (!certificates.length) {
      setSelectedCertificateId("");
      return;
    }
    if (selectedCertificateId && certificates.some((item) => item.id === selectedCertificateId)) {
      return;
    }
    setSelectedCertificateId(
      certificates.find((item) => item.is_default)?.id ?? certificates[0].id,
    );
  }, [certificates, selectedCertificateId]);

  const selectedCertificate = useMemo(
    () => certificates.find((item) => item.id === selectedCertificateId) ?? null,
    [certificates, selectedCertificateId],
  );

  const {
    apkPath,
    outputPath,
    setOutputPath,
    state,
    error,
    dragActive,
    hasApk,
    browseApk,
    sign,
    reset,
  } = useSignWorkflow({
    locale,
    certificate: selectedCertificate,
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
          {certificatesLoaded && certificates.length === 0 && (
            <div className="mt-5">
              <StatusMessage
                kind="warning"
                action={
                  <AppButton size="sm" variant="secondary" onClick={onOpenCertificates}>
                    <FolderKey className="h-4 w-4" />
                    {t(locale, "navCertificates")}
                  </AppButton>
                }
              >
                {t(locale, "noCertificates")}
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
              {state !== "done" && (
                <AppButton
                  className="min-w-[136px]"
                  disabled={!apkPath || !runtimeInfoLoaded || !selectedCertificate || state === "signing"}
                  onClick={sign}
                >
                  {state === "signing" ? <Loader2 className="h-4 w-4 animate-spin" /> : <KeyRound className="h-4 w-4" />}
                  {state === "signing" ? t(locale, "signing") : !runtimeInfoLoaded ? t(locale, "checkingEnvironment") : t(locale, "startSign")}
                </AppButton>
              )}
              {(state === "done" || state === "failed") && (
                <AppButton className="min-w-[136px]" variant="secondary" onClick={reset}>
                  <RotateCcw className="h-4 w-4" />
                  {t(locale, "signAnother")}
                </AppButton>
              )}
            </div>
          </div>

          <div className="mt-8 space-y-4">
            <SelectedApkCard locale={locale} path={apkPath} disabled={state === "signing"} onChange={browseApk} />
            <div className="rounded-[14px] border bg-card p-4">
              <label className="field-label" htmlFor="sign-certificate">{t(locale, "selectCertificate")}</label>
              <SelectInput
                id="sign-certificate"
                className="mt-2"
                value={selectedCertificateId}
                onChange={(e) => setSelectedCertificateId(e.target.value)}
              >
                {certificates.length === 0 ? (
                  <option value="">{t(locale, "noCertificates")}</option>
                ) : (
                  certificates.map((item) => (
                    <option key={item.id} value={item.id}>
                      {item.is_default ? `${item.name} · ${t(locale, "defaultCertificate")}` : item.name}
                    </option>
                  ))
                )}
              </SelectInput>
              {!selectedCertificate && (
                <div className="mt-3">
                  <StatusMessage
                    kind="warning"
                    action={
                      <AppButton size="sm" variant="secondary" onClick={onOpenCertificates}>
                        <FolderKey className="h-4 w-4" />
                        {t(locale, "navCertificates")}
                      </AppButton>
                    }
                  >
                    {t(locale, "noCertificates")}
                  </StatusMessage>
                </div>
              )}
            </div>
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
        </div>
      )}
    </section>
  );
}
