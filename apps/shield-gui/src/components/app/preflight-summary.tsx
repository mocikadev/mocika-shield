import { useState } from "react";
import { AlertTriangle, CheckCircle2, ChevronDown, ChevronUp, CircleSlash2, Clipboard, Loader2 } from "lucide-react";
import { t, tf, type Locale } from "@/lib/i18n";
import type { ApkCheckResult, ApkPreflightCheck } from "@/lib/tauri";
import { cn } from "@/lib/utils";

export function PreflightSummary({
  locale,
  loading,
  report,
  onCopyDiagnostic,
  copyLabel,
}: {
  locale: Locale;
  loading: boolean;
  report: ApkCheckResult | null;
  onCopyDiagnostic?: () => void;
  copyLabel?: string;
}) {
  const [expanded, setExpanded] = useState(false);
  if (loading) {
    return <div className="flex items-center gap-2 rounded-[14px] border bg-card p-4 text-sm text-muted-foreground"><Loader2 className="h-4 w-4 animate-spin" />{t(locale, "prechecking")}</div>;
  }
  if (!report || report.error) return null;

  const verdict = report.verdict;
  const attentionChecks = report.checks.filter((check) => check.severity !== "ready");
  const visibleChecks = expanded ? report.checks : attentionChecks;
  const hasCollapsedChecks = attentionChecks.length < report.checks.length;
  return (
    <section className={cn(
      "rounded-[14px] border p-4",
      verdict === "ready" && "border-success/30 bg-success/5",
      verdict === "warning" && "border-warning/35 bg-warning/5",
      verdict === "blocked" && "border-destructive/30 bg-destructive/5",
    )}>
      <div className="flex items-start justify-between gap-3">
        <SeverityIcon severity={verdict} className="mt-0.5 h-5 w-5 shrink-0" />
        <div className="min-w-0 flex-1">
          <h2 className="text-sm font-semibold">{t(locale, verdict === "ready" ? "preflightReady" : verdict === "warning" ? "preflightWarning" : "preflightBlocked")}</h2>
          <p className="mt-1 text-xs leading-5 text-muted-foreground">{t(locale, verdict === "ready" ? "preflightReadyHint" : verdict === "warning" ? "preflightWarningHint" : "preflightBlockedHint")}</p>
        </div>
        <div className="flex shrink-0 flex-wrap items-center justify-end gap-2">
          {onCopyDiagnostic && verdict !== "ready" && <button type="button" className="inline-flex items-center gap-1 text-xs font-medium text-muted-foreground hover:text-foreground" onClick={onCopyDiagnostic}><Clipboard className="h-4 w-4" />{copyLabel ?? t(locale, "copyDiagnosticSummary")}</button>}
          {hasCollapsedChecks && (
            <button type="button" className="inline-flex items-center gap-1 text-xs font-medium text-muted-foreground hover:text-foreground" onClick={() => setExpanded((value) => !value)}>
              {expanded ? t(locale, "preflightHideDetails") : tf(locale, "preflightViewDetails", { count: report.checks.length })}
              {expanded ? <ChevronUp className="h-4 w-4" /> : <ChevronDown className="h-4 w-4" />}
            </button>
          )}
        </div>
      </div>
      {visibleChecks.length > 0 && <div className="mt-4 grid gap-2 sm:grid-cols-2">
        {visibleChecks.map((check, index) => (
          <div key={`${check.code}-${index}`} className="flex items-start gap-2 rounded-xl border bg-background/70 px-3 py-2.5">
            <SeverityIcon severity={check.severity} className="mt-0.5 h-4 w-4 shrink-0" />
            <div className="min-w-0"><div className="text-xs font-medium">{checkTitle(locale, check)}</div><div className="mt-1 text-xs leading-5 text-muted-foreground">{checkDetail(locale, check)}</div></div>
          </div>
        ))}
      </div>}
    </section>
  );
}

function SeverityIcon({ severity, className }: { severity: "ready" | "warning" | "blocked"; className?: string }) {
  if (severity === "ready") return <CheckCircle2 className={cn("text-success", className)} />;
  if (severity === "warning") return <AlertTriangle className={cn("text-warning", className)} />;
  return <CircleSlash2 className={cn("text-destructive", className)} />;
}

function checkTitle(locale: Locale, check: ApkPreflightCheck) {
  const keys: Record<string, Parameters<typeof t>[1]> = {
    apk_structure: "preflightStructure",
    already_protected: "preflightProtectionStatus",
    not_protected: "preflightProtectionStatus",
    signature: "preflightSignature",
    unsigned: "preflightSignature",
    certificate: "preflightCertificate",
    certificate_mismatch: "preflightCertificate",
    certificate_unreadable: "preflightCertificate",
    dex_profile: "preflightDex",
    runtime_abi: "preflightAbi",
    native_packaging: "preflightNative",
    manifest_sdk: "preflightSdk",
    split_apk: "preflightInstallShape",
    native_manifest: "preflightNativeManifest",
    http_legacy: "preflightCompatibility",
    manifest_unreadable: "preflightManifest",
  };
  return t(locale, keys[check.code] ?? "unknown");
}

function checkDetail(locale: Locale, check: ApkPreflightCheck) {
  switch (check.code) {
    case "apk_structure": return check.severity === "ready" ? t(locale, "preflightStructureReady") : `${t(locale, "preflightStructureMissing")} ${check.detail ?? ""}`;
    case "already_protected": return t(locale, "alreadyProtected");
    case "not_protected": return t(locale, "preflightNotProtected");
    case "signature": return t(locale, "preflightSigned");
    case "unsigned": return t(locale, "notSigned");
    case "certificate": return t(locale, "preflightCertificateMatch");
    case "certificate_mismatch": return t(locale, "signMismatch");
    case "certificate_unreadable": return check.detail || t(locale, "preflightCertificateUnreadable");
    case "dex_profile": {
      const [count, size] = (check.detail ?? "0|0").split("|");
      return `${count} DEX · ${formatBytes(Number(size))}`;
    }
    case "runtime_abi": return check.detail ? `${t(locale, "preflightUnsupportedAbi")} ${check.detail}` : t(locale, "preflightAbiReady");
    case "native_packaging": {
      const [count, compressed] = (check.detail ?? "0|0").split("|");
      return `${count} SO · ${compressed} ${t(locale, "preflightCompressedNative")}`;
    }
    case "manifest_sdk": {
      const [minSdk, targetSdk] = (check.detail ?? "-|- ").split("|");
      return `${t(locale, "preflightMinSdk")} ${minSdk} · ${t(locale, "preflightTargetSdk")} ${targetSdk}`;
    }
    case "split_apk": return check.severity === "blocked" ? `${t(locale, "preflightSplitDetected")} ${check.detail ?? ""}` : t(locale, "preflightBaseApk");
    case "native_manifest": return check.detail === "false" ? t(locale, "preflightExtractNativeLibsFalse") : check.detail === "true" ? t(locale, "preflightExtractNativeLibsTrue") : t(locale, "preflightExtractNativeLibsDefault");
    case "http_legacy": return t(locale, "preflightHttpLegacy");
    case "manifest_unreadable": return t(locale, "preflightManifestUnreadable");
    default: return check.detail ?? t(locale, "unknown");
  }
}

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "0 B";
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}
