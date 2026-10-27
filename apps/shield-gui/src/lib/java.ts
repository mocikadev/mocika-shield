import { t, tf, type Locale } from "@/lib/i18n";
import type { BuildInfo } from "@/lib/tauri";

export function getJavaStatusText(locale: Locale, buildInfo: BuildInfo | null) {
  if (!buildInfo) {
    return "";
  }

  if (!buildInfo.java_ready) {
    if (buildInfo.java_major) {
      return tf(locale, "javaTooLowDetail", {
        version: buildInfo.java_version,
        min: buildInfo.min_java_major,
      });
    }
    return tf(locale, "javaMissingDetail", { min: buildInfo.min_java_major });
  }

  if (!buildInfo.keytool_ready) {
    return tf(locale, "javaMissingKeytool", { min: buildInfo.min_java_major });
  }

  return tf(locale, "javaReadyDetail", { version: buildInfo.java_version });
}

function withSetupHint(locale: Locale, message: string) {
  return `${message}${locale === "zh" ? "。" : ". "}${t(locale, "javaSetupHint")}`;
}

function getRuntimeError(locale: Locale, buildInfo: BuildInfo | null) {
  if (!buildInfo) {
    return t(locale, "checkingEnvironment");
  }
  if (buildInfo.java_ready) {
    return "";
  }
  if (buildInfo.java_major) {
    return withSetupHint(locale, tf(locale, "javaTooLowDetail", { version: buildInfo.java_version, min: buildInfo.min_java_major }));
  }
  return withSetupHint(locale, tf(locale, "javaMissingDetail", { min: buildInfo.min_java_major }));
}

export function getProtectJavaError(locale: Locale, buildInfo: BuildInfo | null, autoSign: boolean) {
  const runtimeError = getRuntimeError(locale, buildInfo);
  if (runtimeError || !buildInfo) return runtimeError;
  // 自动签名前仍需读取证书指纹；纯加固不要求无关工具。
  return autoSign && !buildInfo.keytool_ready
    ? withSetupHint(locale, tf(locale, "javaRequiredForAutoSign", { min: buildInfo.min_java_major }))
    : "";
}

export function getSignJavaError(locale: Locale, buildInfo: BuildInfo | null) {
  return getRuntimeError(locale, buildInfo);
}

export function getAliasJavaError(locale: Locale, buildInfo: BuildInfo | null) {
  const runtimeError = getRuntimeError(locale, buildInfo);
  if (runtimeError || !buildInfo) return runtimeError;
  return buildInfo.keytool_ready
    ? ""
    : withSetupHint(locale, tf(locale, "javaMissingKeytool", { min: buildInfo.min_java_major }));
}
