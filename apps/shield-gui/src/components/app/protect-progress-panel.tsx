import { Check, Loader2 } from "lucide-react";
import { stepLabels } from "@/components/app/branding";
import { t, type Locale } from "@/lib/i18n";

export function ProtectProgressPanel({
  locale,
  state,
  currentStep,
  steps,
  messages,
  showProgress,
}: {
  locale: Locale;
  state: "idle" | "prechecking" | "running" | "done" | "failed";
  currentStep: string;
  steps: string[];
  messages: string[];
  showProgress: boolean;
}) {
  return (
    <aside className="rounded-[14px] border bg-card p-4">
      <h2 className="mb-3 text-sm font-semibold">
        {showProgress ? t(locale, "running") : t(locale, "ready")}
      </h2>
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
      {messages.length > 0 && (
        <div className="mt-4 rounded-xl bg-muted/50 p-3">
          <div className="mb-2 text-xs font-medium text-muted-foreground">Log</div>
          <div className="space-y-1 font-mono text-xs text-muted-foreground">
            {messages.map((item, index) => (
              <div key={`${item}-${index}`} className="break-words">
                {item}
              </div>
            ))}
          </div>
        </div>
      )}
    </aside>
  );
}
