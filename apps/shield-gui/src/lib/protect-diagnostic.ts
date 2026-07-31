import type { ApkCheckResult } from "@/lib/tauri";
import type { Locale } from "@/lib/i18n";

export function buildProtectDiagnostic({
  preflight,
  locale,
  runtimeMode,
  environmentPolicy,
  currentStep,
  failed,
}: {
  preflight: ApkCheckResult | null;
  locale: Locale;
  runtimeMode: "standard" | "android_api19";
  environmentPolicy: "compatible" | "strict";
  currentStep: string;
  failed: boolean;
}) {
  const checks = preflight?.checks
    .filter((check) => check.severity !== "ready")
    .map((check) => `- ${check.code}: ${severityLabel(locale, check.severity)}`)
    .join("\n");
  const english = locale === "en";
  return [
    english ? "Mocika Shield protection diagnostic summary" : "Mocika Shield 加固诊断摘要",
    `${english ? "Task status" : "任务状态"}: ${failed ? (english ? "failed" : "失败") : (english ? "preflight" : "预检")}`,
    `${english ? "Target system" : "运行系统"}: ${runtimeMode === "android_api19" ? (english ? "Android 4.4 industrial compatibility" : "Android 4.4 工控兼容") : (english ? "Android 5.0 and above" : "Android 5.0 及以上")}`,
    `${english ? "Environment protection" : "环境保护"}: ${environmentPolicy === "strict" ? (english ? "strict" : "严格保护") : (english ? "standard" : "标准保护")}`,
    `${english ? "Current stage" : "当前阶段"}: ${currentStep || (english ? "not started" : "未开始")}`,
    `${english ? "Preflight verdict" : "预检结论"}: ${verdictLabel(locale, preflight?.verdict)}`,
    checks ? `${english ? "Risk items" : "风险项"}:\n${checks}` : `${english ? "Risk items" : "风险项"}: ${english ? "none" : "无"}`,
    english
      ? "Note: This summary excludes APK and output paths, certificates, passwords, package names, business class names, and raw error text. Also include diagnostic information from the About page. Manually redact any logs before sharing."
      : "说明: 本摘要不包含 APK 路径、输出路径、证书、密码、包名、业务类名或原始错误文本。请同时附上“关于”页面的诊断信息；如需提供日志，请先手动脱敏。",
  ].join("\n");
}

function verdictLabel(locale: Locale, verdict: ApkCheckResult["verdict"] | undefined) {
  const english = locale === "en";
  if (verdict === "blocked") return english ? "blocked" : "必须阻止";
  if (verdict === "warning") return english ? "compatibility risk" : "存在兼容风险";
  if (verdict === "ready") return english ? "passed" : "通过";
  return english ? "incomplete" : "未完成";
}

function severityLabel(locale: Locale, severity: "ready" | "warning" | "blocked") {
  const english = locale === "en";
  if (severity === "blocked") return english ? "blocked" : "必须阻止";
  if (severity === "warning") return english ? "risk" : "存在风险";
  return english ? "passed" : "通过";
}
