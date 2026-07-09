import { useCallback, useEffect, useMemo, useState } from "react";
import { isApk, protectedOutputPath, signedOutputPath } from "@/lib/path";
import { t, type Locale } from "@/lib/i18n";
import { getProtectJavaError } from "@/lib/java";
import { notifyError, notifySuccess, notifyWarning } from "@/lib/notify";
import {
  api,
  onTauriEvent,
  openFileDialog,
  type ApkCheckResult,
  type BuildInfo,
  type CertificateRecord,
  type DragDropPayload,
  type ProtectProgress,
} from "@/lib/tauri";

export type ProtectState = "idle" | "prechecking" | "running" | "done" | "failed";

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
  locale,
  certificate,
  buildInfo,
}: {
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
  const [currentStep, setCurrentStep] = useState("");
  const [messages, setMessages] = useState<string[]>([]);

  const autoSignReady = Boolean(certificate?.auto_sign_enabled);

  const computedOutput = useMemo(() => {
    const protectedPath = protectedOutputPath(input);
    return autoSignReady ? signedOutputPath(protectedPath) : protectedPath;
  }, [autoSignReady, input]);

  useEffect(() => {
    setOutput(computedOutput);
  }, [computedOutput]);

  const appendMessage = useCallback((message: string) => {
    setMessages((items) => [...items.slice(-7), message]);
  }, []);

  const resetSelection = useCallback(() => {
    setInput("");
    setOutput("");
    setState("idle");
    setError("");
    setPrecheck("");
    setWarning("");
    setMessages([]);
    setCurrentStep("");
  }, []);

  const handleSelected = useCallback(
    async (path: string) => {
      setWarning("");
      setError("");
      setPrecheck("");
      setMessages([]);
      setCurrentStep("");
      if (!isApk(path)) {
        const message = t(locale, "onlyApk");
        setWarning(message);
        notifyError(message);
        return;
      }
      setInput(path);
      setOutput(protectedOutputPath(path));
      setState("prechecking");
      try {
        const result = await api.checkApk(path);
        const message = precheckMessage(locale, result);
        if (message) {
          setPrecheck(message);
          notifyError(message);
        }
        setState("idle");
      } catch {
        const message = t(locale, "apkCheckFailed");
        setPrecheck(message);
        notifyError(message);
        setState("idle");
      }
    },
    [locale],
  );

  useEffect(() => {
    const unlisten = Promise.all([
      onTauriEvent<ProtectProgress>("protect-progress", (payload) => {
        setCurrentStep(payload.step);
        setMessages((items) => [...items.slice(-7), payload.message]);
      }),
      onTauriEvent<string>("protect-error", (payload) => {
        setError(payload);
        notifyError(payload);
        setState("failed");
      }),
      onTauriEvent<void>("protect-done", () => setState("done")),
      onTauriEvent<DragDropPayload>("tauri://drag-drop", (payload) => {
        const first = payload.paths?.[0];
        setDragActive(false);
        if (first) {
          void handleSelected(first);
        }
      }),
      onTauriEvent<void>("tauri://drag-enter", () => setDragActive(true)),
      onTauriEvent<void>("tauri://drag-leave", () => setDragActive(false)),
    ]);
    return () => {
      void unlisten.then((items) => items.forEach((fn) => fn()));
    };
  }, [handleSelected]);

  const browse = useCallback(async () => {
    const path = await openFileDialog("APK", ["apk"]);
    if (path) {
      await handleSelected(path);
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
      setMessages([]);
      setCurrentStep("CheckTools");

      const unsignedOutput = autoSignReady ? protectedOutputPath(input) : output;
      await api.protectApk(input, unsignedOutput);
      if (autoSignReady && certificate) {
        setCurrentStep("Sign");
        appendMessage(t(locale, "autoSignStarted"));
        const compare = await api.compareCertFingerprints({
          apkPath: input,
          certificateId: certificate.id,
        });
        if (!compare.matches && !compare.error) {
          const message = t(locale, "signMismatch");
          setWarning(message);
          notifyWarning(message);
        }
        await api.signApk({
          apkPath: unsignedOutput,
          outputPath: output,
          apksignerPath: null,
          certificateId: certificate.id,
        });
        await api.deleteFile(`${output}.idsig`).catch(() => undefined);
        appendMessage(t(locale, "autoSignCompleted"));
        await api.deleteFile(unsignedOutput)
          .then(() => appendMessage(t(locale, "cleanedIntermediate")))
          .catch(() => appendMessage(t(locale, "cleanupIntermediateFailed")));
      }
      appendMessage(t(locale, "protectCompleted"));
      setState("done");
      notifySuccess(t(locale, "protectCompleted"));
    } catch (err) {
      const message = String(err);
      setError(message);
      notifyError(message);
      setState("failed");
    }
  }, [appendMessage, autoSignReady, buildInfo, certificate, input, locale, output, precheck]);

  const cancel = useCallback(async () => {
    await api.cancelProtect().catch(() => undefined);
  }, []);

  const steps = autoSignReady
    ? ["CheckTools", "Unpack", "ModifyManifest", "ProcessDex", "InjectRuntime", "Repack", "AlignApk", "Sign"]
    : ["CheckTools", "Unpack", "ModifyManifest", "ProcessDex", "InjectRuntime", "Repack", "AlignApk"];

  return {
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
    hasInput: Boolean(input),
    showProgress: Boolean(input) && (state === "running" || state === "done" || messages.length > 0),
    browse,
    start,
    cancel,
    resetSelection,
  };
}
