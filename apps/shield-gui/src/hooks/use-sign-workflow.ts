import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { isApk, signedOutputPath } from "@/lib/path";
import { t, type Locale } from "@/lib/i18n";
import { getSignJavaError } from "@/lib/java";
import { notifyError, notifySuccess } from "@/lib/notify";
import {
  api,
  onTauriEvent,
  openFileDialog,
  type BuildInfo,
  type CertificateRecord,
  type DragDropPayload,
  type TaskSnapshot,
} from "@/lib/tauri";

export type SignState = "idle" | "signing" | "done" | "failed";

export function useSignWorkflow({
  active,
  locale,
  certificate,
  buildInfo,
}: {
  active: boolean;
  locale: Locale;
  certificate: CertificateRecord | null;
  buildInfo: BuildInfo | null;
}) {
  const [apkPath, setApkPath] = useState("");
  const [outputPath, setOutputPath] = useState("");
  const [state, setState] = useState<SignState>("idle");
  const [error, setError] = useState("");
  const [dragActive, setDragActive] = useState(false);
  const [currentStep, setCurrentStep] = useState("");
  const [startedAt, setStartedAt] = useState<number | null>(null);
  const [finishedAt, setFinishedAt] = useState<number | null>(null);
  const [taskCertificate, setTaskCertificate] = useState<CertificateRecord | null>(null);
  const taskId = useRef<string | null>(null);
  const taskLocked = useRef(false);

  const activeCertificate = taskCertificate ?? certificate;

  useEffect(() => {
    const unlisten = onTauriEvent<TaskSnapshot>("task-state", (payload) => {
      if (payload.kind !== "sign" || payload.task_id !== taskId.current) return;
      setCurrentStep(payload.current_step);
      setStartedAt(payload.started_at_ms);
      setFinishedAt(payload.finished_at_ms ?? null);
      if (payload.status === "failed") setState("failed");
    });
    return () => { void unlisten.then((fn) => fn()); };
  }, []);

  useEffect(() => {
    if (!active) { setDragActive(false); return; }
    const unlisten = Promise.all([
      onTauriEvent<DragDropPayload>("tauri://drag-drop", (payload) => {
        const first = payload.paths?.[0];
        setDragActive(false);
        if (taskLocked.current) return;
        if (first && isApk(first)) {
          setApkPath(first);
          setOutputPath(signedOutputPath(first));
          setState("idle");
          setError("");
        } else if (first) {
          const message = t(locale, "onlyApk");
          setError(message);
          notifyError(message);
        }
      }),
      onTauriEvent<void>("tauri://drag-enter", () => setDragActive(true)),
      onTauriEvent<void>("tauri://drag-leave", () => setDragActive(false)),
    ]);
    return () => {
      void unlisten.then((items) => items.forEach((fn) => fn()));
    };
  }, [active, locale]);

  const browseApk = useCallback(async () => {
    if (taskLocked.current) return;
    const path = await openFileDialog("APK", ["apk"]);
    if (path) {
      setApkPath(path);
      setOutputPath(signedOutputPath(path));
      setState("idle");
      setError("");
    }
  }, []);

  const updateOutputPath = useCallback((path: string) => {
    if (!taskLocked.current) setOutputPath(path);
  }, []);

  const sign = useCallback(async () => {
    if (!apkPath || !certificate) {
      return;
    }
    try {
      setTaskCertificate(certificate);
      taskLocked.current = true;
      const javaError = getSignJavaError(locale, buildInfo);
      if (javaError) {
        setError(javaError);
        notifyError(javaError);
        setState("failed");
        return;
      }

      setState("signing");
      setError("");
      setCurrentStep("PrepareSign");
      setStartedAt(Date.now());
      setFinishedAt(null);
      taskId.current = crypto.randomUUID();
      await api.signApk({
        taskId: taskId.current,
        apkPath,
        outputPath: outputPath || null,
        apksignerPath: null,
        certificateId: certificate.id,
      });
      await api.deleteFile(`${outputPath}.idsig`).catch(() => undefined);
      setState("done");
      notifySuccess(t(locale, "signDone"));
    } catch (err) {
      const message = String(err);
      setError(message);
      notifyError(message);
      setState("failed");
    }
  }, [apkPath, buildInfo, certificate, locale, outputPath]);

  const reset = useCallback(() => {
    setApkPath("");
    setOutputPath("");
    setState("idle");
    setError("");
    setCurrentStep("");
    setStartedAt(null);
    setFinishedAt(null);
    setTaskCertificate(null);
    taskId.current = null;
    taskLocked.current = false;
  }, []);

  const enabledVersions = useMemo(
    () =>
      activeCertificate
        ? [
            activeCertificate.sign_v1 && "V1",
            activeCertificate.sign_v2 && "V2",
            activeCertificate.sign_v3 && "V3",
            activeCertificate.sign_v4 && "V4",
          ]
            .filter(Boolean)
            .join(" / ")
        : "",
    [activeCertificate],
  );

  return {
    apkPath,
    outputPath,
    setOutputPath: updateOutputPath,
    state,
    error,
    dragActive,
    currentStep,
    startedAt,
    finishedAt,
    enabledVersions,
    taskLocked: taskCertificate !== null || state === "failed",
    hasApk: Boolean(apkPath),
    browseApk,
    sign,
    reset,
  };
}
