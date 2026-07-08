import type React from "react";
import { useSidebar, SidebarHeader, SidebarTrigger } from "@/components/ui/sidebar";
import { t, type Locale } from "@/lib/i18n";
import type { UpdateCheckResult } from "@/lib/tauri";
import { basename } from "@/lib/path";
import { cn } from "@/lib/utils";
import { logoSvg } from "@/components/app/branding";
import {
  AlertCircle,
  Check,
  Download,
  Eye,
  EyeOff,
  FileArchive,
  FolderOpen,
  Upload,
  X,
} from "lucide-react";

export function AppSidebarHeader({ locale }: { locale: Locale }) {
  const { state, toggleSidebar } = useSidebar();
  const collapsed = state === "collapsed";

  return (
    <SidebarHeader className="h-[92px] justify-center px-4 pb-3 pt-4 group-data-[collapsible=icon]:px-3">
      {collapsed ? (
        <button
          type="button"
          className="mx-auto flex h-12 w-12 items-center justify-center rounded-[18px] border border-sidebar-border/70 bg-sidebar-accent/35 shadow-sm transition-colors hover:bg-sidebar-accent"
          onClick={toggleSidebar}
          aria-label={t(locale, "expandSidebar")}
          title={t(locale, "expandSidebar")}
        >
          <img src={logoSvg} alt="Mocika Shield" className="h-9 w-9 rounded-xl" />
        </button>
      ) : (
        <div className="flex h-14 items-center gap-3 rounded-[20px] border border-sidebar-border/70 bg-sidebar-accent/30 px-3.5 shadow-sm">
          <img src={logoSvg} alt="Mocika Shield" className="h-10 w-10 rounded-[14px]" />
          <div className="min-w-0 flex-1">
            <div className="truncate text-[15px] font-semibold tracking-normal">Mocika Shield</div>
          </div>
          <SidebarTrigger
            className="h-10 w-10 shrink-0 rounded-xl border border-transparent text-muted-foreground hover:border-sidebar-border hover:bg-sidebar-accent hover:text-foreground"
            aria-label={t(locale, "collapseSidebar")}
            title={t(locale, "collapseSidebar")}
          />
        </div>
      )}
    </SidebarHeader>
  );
}

export function AppButton({
  children,
  variant = "primary",
  size = "md",
  className,
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "ghost" | "danger";
  size?: "sm" | "md";
}) {
  return (
    <button
      className={cn(
        "inline-flex min-h-9 items-center justify-center gap-2 rounded-md px-3.5 text-sm font-medium transition-colors disabled:pointer-events-none disabled:opacity-50",
        "whitespace-nowrap",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
        variant === "primary" && "bg-primary text-primary-foreground hover:bg-primary/90",
        variant === "secondary" && "border bg-background text-foreground hover:bg-muted",
        variant === "ghost" && "text-muted-foreground hover:bg-muted hover:text-foreground",
        variant === "danger" && "bg-destructive text-destructive-foreground hover:bg-destructive/90",
        size === "sm" && "min-h-8 px-3 text-xs",
        className,
      )}
      {...props}
    >
      {children}
    </button>
  );
}

export function TextInput(props: React.InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className={cn(
        "h-10 w-full rounded-xl border border-border/80 bg-background/90 px-3.5 text-sm text-foreground placeholder:text-muted-foreground shadow-sm",
        "min-w-0",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
        props.className,
      )}
    />
  );
}

export function SelectInput(props: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      {...props}
      className={cn(
        "h-10 w-full rounded-xl border border-border/80 bg-background/90 px-3.5 text-sm text-foreground shadow-sm",
        "min-w-0",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
        props.className,
      )}
    />
  );
}

export function StatusMessage({
  kind,
  children,
  action,
}: {
  kind: "info" | "success" | "warning" | "error";
  children: React.ReactNode;
  action?: React.ReactNode;
}) {
  return (
    <div
      className={cn(
        "flex items-start justify-between gap-3 rounded-md border px-3 py-2 text-sm",
        kind === "info" && "border-primary/25 bg-primary/10 text-foreground",
        kind === "success" && "border-success/30 bg-success/10 text-foreground",
        kind === "warning" && "border-warning/35 bg-warning/10 text-foreground",
        kind === "error" && "border-destructive/30 bg-destructive/10 text-foreground",
      )}
      role={kind === "error" ? "alert" : "status"}
    >
      <div className="flex min-w-0 items-start gap-2">
        {kind === "success" ? (
          <Check className="mt-0.5 h-4 w-4 shrink-0 text-success" />
        ) : (
          <AlertCircle
            className={cn(
              "mt-0.5 h-4 w-4 shrink-0",
              kind === "error" && "text-destructive",
              kind === "warning" && "text-warning",
              kind === "info" && "text-primary",
            )}
          />
        )}
        <div className="min-w-0 break-words">{children}</div>
      </div>
      {action}
    </div>
  );
}

export function DropZone({
  locale,
  active,
  title,
  subtitle,
  onBrowse,
}: {
  locale: Locale;
  active: boolean;
  title: string;
  subtitle: string;
  onBrowse: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onBrowse}
      className={cn(
        "flex min-h-[260px] w-full flex-col items-center justify-center gap-4 rounded-[28px] border-2 border-dashed bg-transparent p-8 text-center transition-colors",
        "border-border/80 hover:border-primary/70 hover:bg-primary/5",
        active && "border-primary bg-primary/10",
      )}
      aria-label={t(locale, "selectApk")}
    >
      <span className="flex h-16 w-16 items-center justify-center rounded-[18px] bg-muted text-muted-foreground">
        <Upload className="h-8 w-8" />
      </span>
      <span className="text-[19px] font-semibold">{title}</span>
      <span className="text-[15px] font-medium text-muted-foreground">{subtitle}</span>
    </button>
  );
}

export function SelectedApkCard({
  locale,
  path,
  output,
  disabled,
  onChange,
}: {
  locale: Locale;
  path: string;
  output?: string;
  disabled?: boolean;
  onChange: () => void;
}) {
  return (
    <div className="rounded-[14px] border bg-card p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="flex min-w-0 items-start gap-3">
          <span className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-muted text-muted-foreground">
            <FileArchive className="h-5 w-5" />
          </span>
          <div className="min-w-0">
            <div className="text-xs font-medium text-muted-foreground">{t(locale, "selectedApk")}</div>
            <div className="mt-1 truncate text-sm font-semibold">{basename(path) || "-"}</div>
            <div className="path-text mt-1">{path}</div>
          </div>
        </div>
        <AppButton size="sm" variant="secondary" disabled={disabled} onClick={onChange}>
          <FolderOpen className="h-4 w-4" />
          {t(locale, "changeApk")}
        </AppButton>
      </div>
      {output && (
        <div className="mt-4 rounded-xl bg-muted/50 p-3">
          <div className="mb-1 flex items-center gap-2 text-xs font-medium text-muted-foreground">
            <FolderOpen className="h-4 w-4" />
            {t(locale, "outputPath")}
          </div>
          <div className="path-text">{output}</div>
        </div>
      )}
    </div>
  );
}

export function PasswordControl({
  id,
  label,
  value,
  onChange,
  show,
  setShow,
}: {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  show: boolean;
  setShow: (show: boolean) => void;
}) {
  return (
    <div className="flex min-w-0 gap-2">
      <TextInput id={id} className="flex-1" type={show ? "text" : "password"} value={value} onChange={(e) => onChange(e.target.value)} />
      <button type="button" className="icon-button shrink-0" onClick={() => setShow(!show)} aria-label={label}>
        {show ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
      </button>
    </div>
  );
}

export function SettingsFieldRow({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="grid min-h-[72px] gap-4 border-b border-border/60 px-6 py-4 last:border-b-0 sm:grid-cols-[220px_minmax(0,1fr)] sm:items-center">
      <div className="min-w-0">
        <div className="text-[14px] font-semibold text-foreground">{label}</div>
        {hint && <div className="field-hint mt-1">{hint}</div>}
      </div>
      <div className="min-w-0 sm:flex sm:items-center sm:justify-end">{children}</div>
    </div>
  );
}

export function SettingsGroup({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section>
      <h2 className="mb-3 px-1 text-[15px] font-semibold tracking-normal text-foreground/90">{title}</h2>
      <div className="overflow-hidden rounded-[22px] border border-border/80 bg-card/95 shadow-sm">
        {children}
      </div>
    </section>
  );
}

export function PillSegment<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: Array<{ value: T; label: string }>;
  onChange: (value: T) => void;
}) {
  return (
    <div
      className="inline-grid min-h-10 rounded-[14px] border border-border/70 bg-muted/75 p-1 shadow-inner"
      style={{ gridTemplateColumns: `repeat(${options.length}, minmax(0, 1fr))` }}
    >
      {options.map((item) => {
        const active = value === item.value;
        return (
          <button
            key={item.value}
            type="button"
            className={cn(
              "min-h-8 rounded-[10px] px-5 text-sm font-semibold transition-[background-color,color,box-shadow]",
              active
                ? "bg-card text-foreground shadow-sm"
                : "text-muted-foreground hover:bg-background/70 hover:text-foreground",
            )}
            onClick={() => onChange(item.value)}
            aria-pressed={active}
          >
            {item.label}
          </button>
        );
      })}
    </div>
  );
}

export function SummaryRow({ label, value, muted = false }: { label: string; value: string; muted?: boolean }) {
  return (
    <div className="grid gap-1 py-3 sm:grid-cols-[140px_minmax(0,1fr)] sm:gap-4">
      <div className="text-sm font-medium text-muted-foreground">{label}</div>
      <div className={cn("min-w-0 break-all text-sm", muted ? "text-muted-foreground" : "text-foreground")}>{value}</div>
    </div>
  );
}

export function UpdateBanner({
  locale,
  updateInfo,
  onDismiss,
  onViewRelease,
}: {
  locale: Locale;
  updateInfo: UpdateCheckResult | null;
  onDismiss: () => void;
  onViewRelease: (url: string) => void;
}) {
  if (!updateInfo?.has_update || !updateInfo.latest_version) {
    return null;
  }
  return (
    <div className="border-b bg-primary/10 px-4 py-2 text-sm">
      <div className="flex items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2">
          <Download className="h-4 w-4 shrink-0 text-primary" />
          <span className="truncate">
            {t(locale, "updateAvailable")} v{updateInfo.latest_version}
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {updateInfo.release_url && (
            <AppButton size="sm" variant="ghost" onClick={() => onViewRelease(updateInfo.release_url!)}>
              {t(locale, "viewRelease")}
            </AppButton>
          )}
          {updateInfo.update_level !== "major" && (
            <button className="icon-button" type="button" aria-label={t(locale, "ignore")} onClick={onDismiss}>
              <X className="h-4 w-4" />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

export function MajorUpdateDialog({
  locale,
  open,
  latestVersion,
  releaseUrl,
  onClose,
  onViewRelease,
}: {
  locale: Locale;
  open: boolean;
  latestVersion?: string;
  releaseUrl?: string | null;
  onClose: () => void;
  onViewRelease: (url: string) => void;
}) {
  if (!open || !latestVersion) {
    return null;
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-6">
      <section className="app-panel w-full max-w-md p-6 text-center">
        <div className="mx-auto mb-4 flex h-12 w-12 items-center justify-center rounded-md bg-primary/12 text-primary">
          <Download className="h-6 w-6" />
        </div>
        <h2 className="text-lg font-semibold">{t(locale, "majorUpdate")}</h2>
        <p className="mt-2 text-sm text-muted-foreground">
          {t(locale, "updateAvailable")} v{latestVersion}
        </p>
        <div className="mt-6 flex justify-center gap-3">
          <AppButton
            onClick={() => {
              if (releaseUrl) {
                onViewRelease(releaseUrl);
              }
              onClose();
            }}
          >
            <Download className="h-4 w-4" />
            {t(locale, "viewRelease")}
          </AppButton>
          <AppButton variant="secondary" onClick={onClose}>
            {t(locale, "ignore")}
          </AppButton>
        </div>
      </section>
    </div>
  );
}
