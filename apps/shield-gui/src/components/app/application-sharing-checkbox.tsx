import { t, type Locale } from "@/lib/i18n";

export function ApplicationSharingCheckbox({ locale, operation, checked, disabled, onChange }: {
  locale: Locale; operation: "protect" | "sign"; checked: boolean; disabled: boolean; onChange: (enabled: boolean) => void;
}) {
  return <label className={`inline-flex min-w-0 items-center gap-2 text-sm ${disabled ? "text-muted-foreground" : "cursor-pointer"}`}>
    <input type="checkbox" className="h-4 w-4 shrink-0 accent-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2" checked={checked} disabled={disabled} onChange={event => onChange(event.target.checked)} />
    <span>{t(locale, operation === "protect" ? "shareProtectUsage" : "shareSignUsage")}</span>
  </label>;
}
