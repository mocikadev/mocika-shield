import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  dirname,
  isApk,
  joinPath,
  normalizeApkFilename,
  protectedOutputFilename,
  validateOutputFilename,
} from "@/lib/path";
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
  type ProtectDefaults,
  type TaskSnapshot,
} from "@/lib/tauri";

export type ProtectState = "idle" | "prechecking" | "running" | "done" | "failed";
export type RuntimeMode = "standard" | "android_api19";
export type EnvironmentPolicy = "compatible" | "strict";

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
  defaults,
}: {
  active: boolean;
  locale: Locale;
  certificate: CertificateRecord | null;
  buildInfo: BuildInfo | null;
  defaults: ProtectDefaults;
}) {
  const [input, setInput] = useState("");
  const [outputFilename, setOutputFilenameState] = useState("");
  const [filenameEdited, setFilenameEdited] = useState(false);
  const [outputDirectoryMode, setOutputDirectoryMode] = useState<"source" | "fixed">(defaults.output_directory_mode);
  const [fixedOutputDirectory, setFixedOutputDirectory] = useState(defaults.fixed_output_directory);
  const [state, setState] = useState<ProtectState>("idle");
  const [dragActive, setDragActive] = useState(false);
  const [warning, setWarning] = useState("");
  const [error, setError] = useState("");
  const [precheck, setPrecheck] = useState("");
  const [runtimeMode, setRuntimeMode] = useState<RuntimeMode>(defaults.runtime_mode);
  const [environmentPolicy, setEnvironmentPolicy] = useState<EnvironmentPolicy>(defaults.environment_policy);
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

  const outputDirectory = outputDirectoryMode === "fixed" && fixedOutputDirectory
    ? fixedOutputDirectory
    : dirname(input);
  const normalizedOutputFilename = normalizeApkFilename(outputFilename);
  const output = useMemo(
    () => input && normalizedOutputFilename ? joinPath(outputDirectory, normalizedOutputFilename) : "",
    [input, normalizedOutputFilename, outputDirectory],
  );
  const outputFilenameError = validateOutputFilename(outputFilename);

  useEffect(() => {
    if (taskLocked.current) return;
    setRuntimeMode(defaults.runtime_mode);
    setEnvironmentPolicy(defaults.environment_policy);
    setOutputDirectoryMode(defaults.output_directory_mode);
    setFixedOutputDirectory(defaults.fixed_output_directory);
  }, [defaults]);

  useEffect(() => {
    if (!taskLocked.current && !filenameEdited && input) {
      setOutputFilenameState(protectedOutputFilename(input, autoSignReady));
    }
  }, [autoSignReady, filenameEdited, input]);

  const resetSelection = useCallback(() => {
    setInput("");
    setOutputFilenameState("");
    setFilenameEdited(false);
    setState("idle");
    setError("");
    setPrecheck("");
    setWarning("");
    setCurrentStep("");
    setStartedAt(null);
    setFinishedAt(null);
    setTaskAutoSign(null);
    setTaskCertificate(null);
    setRuntimeMode(defaults.runtime_mode);
    setEnvironmentPolicy(defaults.environment_policy);
    setOutputDirectoryMode(defaults.output_directory_mode);
    setFixedOutputDirectory(defaults.fixed_output_directory);
    setNativeAbis([]);
    taskId.current = null;
    taskLocked.current = false;
  }, [defaults]);

  const setOutputFilename = useCallback((filename: string) => {
    if (!taskLocked.current) {
      setFilenameEdited(true);
      setOutputFilenameState(filename);
    }
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
      setFilenameEdited(false);
      setOutputFilenameState(protectedOutputFilename(path, Boolean(certificate)));
    },
    [certificate, locale],
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
    if (!input || !output || precheck || outputFilenameError || (outputDirectoryMode === "fixed" && !fixedOutputDirectory)) {
      return;
    }
    try {
      if (await api.checkFileExists(output)) {
        const confirmed = window.confirm(t(locale, "confirmOverwriteOutput"));
        if (!confirmed) return;
      }
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

      const intermediateOutput = joinPath(outputDirectory, protectedOutputFilename(input, false));
      const unsignedOutput = autoSignReady && intermediateOutput === output
        ? joinPath(outputDirectory, `${protectedOutputFilename(input, false).replace(/\.apk$/i, "")}_unsigned.apk`)
        : autoSignReady ? intermediateOutput : output;
      await api.protectApk(
        taskId.current,
        input,
        unsignedOutput,
        runtimeMode,
        environmentPolicy,
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
  }, [autoSignReady, buildInfo, certificate, environmentPolicy, fixedOutputDirectory, input, locale, output, outputDirectory, outputDirectoryMode, outputFilenameError, precheck, runtimeMode]);

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
    outputFilename,
    setOutputFilename,
    outputFilenameError,
    outputDirectory,
    outputDirectoryMode,
    setOutputDirectoryMode,
    fixedOutputDirectory,
    setFixedOutputDirectory,
    state,
    dragActive,
    warning,
    error,
    precheck,
    runtimeMode,
    setRuntimeMode,
    environmentPolicy,
    setEnvironmentPolicy,
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
