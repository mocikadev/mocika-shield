import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { isApk, protectedOutputPath, signedOutputPath } from "@/lib/path";
import { t, type Locale } from "@/lib/i18n";
import { getProtectJavaError } from "@/lib/java";
import { notifyError, notifySuccess } from "@/lib/notify";
import {
  api,
  onTauriEvent,
  openFileDialog,
  type ApkCheckResult,
  type BuildInfo,
  type CertificateRecord,
  type DragDropPayload,
  type TaskSnapshot,
} from "@/lib/tauri";

export type ProtectState = "idle" | "prechecking" | "running" | "done" | "failed";
export type RuntimeMode = "standard" | "android_api19";

function precheckMessage(locale: Locale, result: ApkCheckResult) {
  if (result.error) {
    return `${t(locale, "readApkFailed")}: ${result.error}`;
  }
  if (result.already_protected) {
    return t(locale, "alreadyProtected");
  }
  if (!result.is_signed) {
    return t(locale, "notSigned");
  }
  return "";
}

export function useProtectWorkflow({
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
  const [input, setInput] = useState("");
  const [output, setOutput] = useState("");
  const [state, setState] = useState<ProtectState>("idle");
  const [dragActive, setDragActive] = useState(false);
  const [warning, setWarning] = useState("");
  const [error, setError] = useState("");
  const [precheck, setPrecheck] = useState("");
  const [runtimeMode, setRuntimeMode] = useState<RuntimeMode>("standard");
  const [nativeAbis, setNativeAbis] = useState<string[]>([]);
  const [currentStep, setCurrentStep] = useState("");
  const [startedAt, setStartedAt] = useState<number | null>(null);
  const [finishedAt, setFinishedAt] = useState<number | null>(null);
  const [taskAutoSign, setTaskAutoSign] = useState<boolean | null>(null);
  const [taskCertificate, setTaskCertificate] = useState<CertificateRecord | null>(null);
  const precheckRequest = useRef(0);
  const taskId = useRef<string | null>(null);
  const taskLocked = useRef(false);

  const activeCertificate = taskAutoSign === null ? certificate : taskCertificate;
  const autoSignReady = Boolean(activeCertificate);
  const autoSignCertificateId = activeCertificate?.id ?? null;

  const computedOutput = useMemo(() => {
    const protectedPath = protectedOutputPath(input);
    return autoSignReady ? signedOutputPath(protectedPath) : protectedPath;
  }, [autoSignReady, input]);

  useEffect(() => {
    if (!taskLocked.current) {
      setOutput(computedOutput);
    }
  }, [computedOutput]);

  const resetSelection = useCallback(() => {
    setInput("");
    setOutput("");
    setState("idle");
    setError("");
    setPrecheck("");
    setWarning("");
    setCurrentStep("");
    setStartedAt(null);
    setFinishedAt(null);
    setTaskAutoSign(null);
    setTaskCertificate(null);
    setRuntimeMode("standard");
    setNativeAbis([]);
    taskId.current = null;
    taskLocked.current = false;
  }, []);

  const updateOutput = useCallback((path: string) => {
    if (!taskLocked.current) setOutput(path);
  }, []);

  const handleSelected = useCallback(
    (path: string) => {
      if (taskLocked.current) {
        return;
      }
      setWarning("");
      setError("");
      setPrecheck("");
      setCurrentStep("");
      if (!isApk(path)) {
        const message = t(locale, "onlyApk");
        setWarning(message);
        notifyError(message);
        return;
      }
      setInput(path);
      setOutput(protectedOutputPath(path));
    },
    [locale],
  );

  const runPrecheck = useCallback(
    async (path: string) => {
      const request = ++precheckRequest.current;
      setState("prechecking");
      setPrecheck("");
      try {
        const result = await api.checkApk(path);
        setNativeAbis(result.native_abis);
        let message = precheckMessage(locale, result);
        if (!message && autoSignCertificateId) {
          const compare = await api.compareCertFingerprints({
            apkPath: path,
            certificateId: autoSignCertificateId,
          });
          if (compare.error) {
            message = compare.error;
          } else if (!compare.matches) {
            message = t(locale, "signMismatch");
          }
        }
        if (request !== precheckRequest.current) {
          return;
        }
        if (message) {
          setPrecheck(message);
          notifyError(message);
        }
        setState("idle");
      } catch {
        if (request !== precheckRequest.current) {
          return;
        }
        const message = t(locale, "apkCheckFailed");
        setPrecheck(message);
        notifyError(message);
        setState("idle");
      }
    },
    [autoSignCertificateId, locale],
  );

  useEffect(() => {
    if (!input || taskLocked.current) {
      return;
    }
    void runPrecheck(input);
    return () => {
      precheckRequest.current += 1;
    };
  }, [input, runPrecheck]);

  useEffect(() => {
    const unlisten = Promise.all([
      onTauriEvent<TaskSnapshot>("task-state", (payload) => {
        if (payload.kind !== "protect" || payload.task_id !== taskId.current) return;
        setCurrentStep(payload.current_step);
        setStartedAt(payload.started_at_ms);
        setFinishedAt(payload.finished_at_ms ?? null);
        if (payload.status === "failed") {
          setError(payload.error ?? t(locale, "failed"));
          setState("failed");
        }
      }),
    ]);
    return () => { void unlisten.then((items) => items.forEach((fn) => fn())); };
  }, [locale]);

  useEffect(() => {
    if (!active) { setDragActive(false); return; }
    const unlisten = Promise.all([
      onTauriEvent<DragDropPayload>("tauri://drag-drop", (payload) => {
        const first = payload.paths?.[0];
        setDragActive(false);
        if (first) {
          handleSelected(first);
        }
      }),
      onTauriEvent<void>("tauri://drag-enter", () => setDragActive(true)),
      onTauriEvent<void>("tauri://drag-leave", () => setDragActive(false)),
    ]);
    return () => {
      void unlisten.then((items) => items.forEach((fn) => fn()));
    };
  }, [active, handleSelected]);

  const browse = useCallback(async () => {
    const path = await openFileDialog("APK", ["apk"]);
    if (path) {
      handleSelected(path);
    }
  }, [handleSelected]);

  const start = useCallback(async () => {
    if (!input || !output || precheck) {
      return;
    }
    try {
      const javaError = getProtectJavaError(locale, buildInfo);
      if (javaError) {
        setError(javaError);
        notifyError(javaError);
        setState("failed");
        return;
      }

      setState("running");
      setError("");
      setCurrentStep("CheckTools");
      setStartedAt(Date.now());
      setFinishedAt(null);
      setTaskAutoSign(autoSignReady);
      setTaskCertificate(certificate);
      taskLocked.current = true;
      taskId.current = crypto.randomUUID();

      const unsignedOutput = autoSignReady ? protectedOutputPath(input) : output;
      await api.protectApk(
        taskId.current,
        input,
        unsignedOutput,
        runtimeMode,
        autoSignReady ? output : null,
        autoSignReady && certificate ? certificate.id : null,
      );
      setFinishedAt(Date.now());
      setState("done");
      notifySuccess(t(locale, "protectCompleted"));
    } catch (err) {
      const message = String(err);
      setError(message);
      notifyError(message);
      setState("failed");
    }
  }, [autoSignReady, buildInfo, certificate, input, locale, output, precheck, runtimeMode]);

  const cancel = useCallback(async () => {
    await api.cancelProtect().catch(() => undefined);
  }, []);

  const effectiveAutoSign = taskAutoSign ?? autoSignReady;
  const steps = effectiveAutoSign
    ? ["CheckTools", "Unpack", "ModifyManifest", "ProcessDex", "InjectRuntime", "Repack", "AlignApk", "PrepareSign", "SignApk", "Cleanup"]
    : ["CheckTools", "Unpack", "ModifyManifest", "ProcessDex", "InjectRuntime", "Repack", "AlignApk"];

  return {
    input,
    output,
    setOutput: updateOutput,
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
    taskLocked: taskAutoSign !== null || state === "failed",
    steps,
    hasInput: Boolean(input),
    showProgress: Boolean(input) && (state === "running" || state === "done" || state === "failed" || Boolean(currentStep)),
    browse,
    start,
    cancel,
    resetSelection,
  };
}
