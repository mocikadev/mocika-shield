import { useEffect, useState } from "react";
import { Check, Loader2 } from "lucide-react";
import { stepLabels } from "@/components/app/branding";
import { t, type Locale } from "@/lib/i18n";

export function ProtectProgressPanel({
  locale,
  state,
  currentStep,
  steps,
  showProgress,
  startedAt,
  finishedAt,
}: {
  locale: Locale;
  state: "idle" | "prechecking" | "running" | "done" | "failed";
  currentStep: string;
  steps: string[];
  showProgress: boolean;
  startedAt?: number | null;
  finishedAt?: number | null;
}) {
  const [tick, refresh] = useState(0);
  useEffect(() => {
    if (!startedAt || finishedAt) return;
    const timer = window.setInterval(() => refresh((value) => value + 1), 1000);
    return () => window.clearInterval(timer);
  }, [finishedAt, startedAt]);
  const currentIndex = steps.indexOf(currentStep);
  const percent = state === "done" ? 100 : currentIndex < 0 ? 0 : Math.min(99, Math.round(((currentIndex + 0.5) / steps.length) * 100));
  const elapsed = (() => {
    void tick;
    if (!startedAt) return "";
    const seconds = Math.max(0, Math.floor(((finishedAt ?? Date.now()) - startedAt) / 1000));
    return seconds < 60 ? `${seconds}s` : `${Math.floor(seconds / 60)}m ${String(seconds % 60).padStart(2, "0")}s`;
  })();
  return (
    <aside className="rounded-[14px] border bg-card p-4">
      <div className="mb-3 flex items-center justify-between gap-3">
        <h2 className="text-sm font-semibold">{showProgress ? t(locale, "running") : t(locale, "ready")}</h2>
        {showProgress && <span className="text-xs tabular-nums text-muted-foreground">{percent}%{elapsed ? ` · ${elapsed}` : ""}</span>}
      </div>
      <div className="mb-4 h-2 overflow-hidden rounded-full bg-muted"><div className="h-full rounded-full bg-primary transition-[width]" style={{ width: `${percent}%` }} /></div>
      <div className="space-y-2">
        {steps.map((step) => (
          <div key={step} className="flex items-center gap-2 rounded-md px-2 py-1.5">
            {currentStep === step && state === "running" ? (
              <Loader2 className="h-4 w-4 animate-spin text-primary" />
            ) : steps.indexOf(step) < steps.indexOf(currentStep) || state === "done" ? (
              <Check className="h-4 w-4 text-success" />
            ) : (
              <span className="h-4 w-4 rounded-full border" />
            )}
            <span className="text-sm">{stepLabels[step]?.[locale] ?? step}</span>
          </div>
        ))}
      </div>
    </aside>
  );
}
