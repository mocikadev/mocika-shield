import { useCallback, useEffect, useMemo, useState } from "react";
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
} from "@/lib/tauri";

export type SignState = "idle" | "signing" | "done" | "failed";

export function useSignWorkflow({
  locale,
  certificate,
  buildInfo,
}: {
  locale: Locale;
  certificate: CertificateRecord | null;
  buildInfo: BuildInfo | null;
}) {
  const [apkPath, setApkPath] = useState("");
  const [outputPath, setOutputPath] = useState("");
  const [state, setState] = useState<SignState>("idle");
  const [error, setError] = useState("");
  const [dragActive, setDragActive] = useState(false);

  useEffect(() => {
    const unlisten = Promise.all([
      onTauriEvent<DragDropPayload>("tauri://drag-drop", (payload) => {
        const first = payload.paths?.[0];
        setDragActive(false);
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
  }, [locale]);

  const browseApk = useCallback(async () => {
    const path = await openFileDialog("APK", ["apk"]);
    if (path) {
      setApkPath(path);
      setOutputPath(signedOutputPath(path));
      setState("idle");
      setError("");
    }
  }, []);

  const sign = useCallback(async () => {
    if (!apkPath || !certificate) {
      return;
    }
    try {
      const javaError = getSignJavaError(locale, buildInfo);
      if (javaError) {
        setError(javaError);
        notifyError(javaError);
        setState("failed");
        return;
      }

      setState("signing");
      setError("");
      await api.signApk({
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
  }, []);

  const enabledVersions = useMemo(
    () =>
      certificate
        ? [
            certificate.sign_v1 && "V1",
            certificate.sign_v2 && "V2",
            certificate.sign_v3 && "V3",
            certificate.sign_v4 && "V4",
          ]
            .filter(Boolean)
            .join(" / ")
        : "",
    [certificate],
  );

  return {
    apkPath,
    outputPath,
    setOutputPath,
    state,
    error,
    dragActive,
    enabledVersions,
    hasApk: Boolean(apkPath),
    browseApk,
    sign,
    reset,
  };
}
