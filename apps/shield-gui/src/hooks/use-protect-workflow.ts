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
  const precheckRequest = useRef(0);

  const autoSignReady = Boolean(certificate?.auto_sign_enabled);
  const autoSignCertificateId = autoSignReady ? certificate?.id ?? null : null;

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
    (path: string) => {
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
    if (!input) {
      return;
    }
    void runPrecheck(input);
    return () => {
      precheckRequest.current += 1;
    };
  }, [input, runPrecheck]);

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
          handleSelected(first);
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
      setMessages([]);
      setCurrentStep("CheckTools");

      const unsignedOutput = autoSignReady ? protectedOutputPath(input) : output;
      await api.protectApk(
        input,
        unsignedOutput,
        autoSignReady && certificate ? certificate.id : null,
      );
      if (autoSignReady && certificate) {
        setCurrentStep("Sign");
        appendMessage(t(locale, "autoSignStarted"));
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
