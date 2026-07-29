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

export function getProtectJavaError(locale: Locale, buildInfo: BuildInfo | null) {
  if (!buildInfo) {
    return t(locale, "checkingEnvironment");
  }
  if (buildInfo.java_ready && buildInfo.keytool_ready) {
    return "";
  }
  return tf(locale, "javaRequiredForProtect", { min: buildInfo.min_java_major });
}

export function getSignJavaError(locale: Locale, buildInfo: BuildInfo | null) {
  if (!buildInfo) {
    return t(locale, "checkingEnvironment");
  }
  if (buildInfo.java_ready && buildInfo.keytool_ready) {
    return "";
  }
  return tf(locale, "javaRequiredForSign", { min: buildInfo.min_java_major });
}

export function getAliasJavaError(locale: Locale, buildInfo: BuildInfo | null) {
  if (!buildInfo) {
    return t(locale, "checkingEnvironment");
  }
  if (buildInfo.java_ready && buildInfo.keytool_ready) {
    return "";
  }
  return tf(locale, "javaRequiredForAlias", { min: buildInfo.min_java_major });
}
