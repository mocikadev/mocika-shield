import { useCallback, useEffect, useMemo, useState } from "react";
import { isApk, signedOutputPath } from "@/lib/path";
import { t, type Locale } from "@/lib/i18n";
import { getSignJavaError } from "@/lib/java";
import {
  api,
  onTauriEvent,
  openFileDialog,
  type BuildInfo,
  type DragDropPayload,
  type SignConfig,
} from "@/lib/tauri";

export type SignState = "idle" | "signing" | "done" | "failed";

export function useSignWorkflow({
  locale,
  signConfig,
  signConfigLoaded,
  buildInfo,
}: {
  locale: Locale;
  signConfig: SignConfig;
  signConfigLoaded: boolean;
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
          setError(t(locale, "onlyApk"));
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
    if (!apkPath) {
      return;
    }
    if (!signConfig.keystore_path) {
      setError(t(locale, "missingKeystore"));
      return;
    }
    if (!signConfig.key_alias) {
      setError(t(locale, "missingAlias"));
      return;
    }
    try {
      const javaError = getSignJavaError(locale, buildInfo);
      if (javaError) {
        setError(javaError);
        setState("failed");
        return;
      }

      setState("signing");
      setError("");
      await api.signApk({
        apkPath,
        outputPath: outputPath || null,
        apksignerPath: null,
        keystorePath: signConfig.keystore_path,
        keyAlias: signConfig.key_alias,
        ksType: signConfig.ks_type ?? "JKS",
        signV1: signConfig.sign_v1,
        signV2: signConfig.sign_v2,
        signV3: signConfig.sign_v3,
        signV4: signConfig.sign_v4,
      });
      await api.deleteFile(`${outputPath}.idsig`).catch(() => undefined);
      setState("done");
    } catch (err) {
      setError(String(err));
      setState("failed");
    }
  }, [apkPath, locale, outputPath, signConfig]);

  const reset = useCallback(() => {
    setApkPath("");
    setOutputPath("");
    setState("idle");
    setError("");
  }, []);

  const savedReady = Boolean(
    signConfigLoaded && signConfig.keystore_path && signConfig.key_alias,
  );
  const enabledVersions = useMemo(
    () =>
      [
        signConfig.sign_v1 && "V1",
        signConfig.sign_v2 && "V2",
        signConfig.sign_v3 && "V3",
        signConfig.sign_v4 && "V4",
      ]
        .filter(Boolean)
        .join(" / "),
    [signConfig],
  );

  return {
    apkPath,
    outputPath,
    setOutputPath,
    state,
    error,
    dragActive,
    savedReady,
    enabledVersions,
    hasApk: Boolean(apkPath),
    browseApk,
    sign,
    reset,
  };
}
