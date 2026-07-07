import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertCircle,
  BadgeCheck,
  Check,
  Clipboard,
  Download,
  Eye,
  EyeOff,
  FileArchive,
  FolderOpen,
  Info,
  KeyRound,
  Loader2,
  PencilLine,
  Play,
  RotateCcw,
  Save,
  Settings,
  ShieldCheck,
  Square,
  Upload,
  X,
} from "lucide-react";
import { cn } from "./lib/utils";
import { detectSystemLocale, type Locale, t } from "./lib/i18n";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarTrigger,
  useSidebar,
} from "@/components/ui/sidebar";
import { Switch } from "@/components/ui/switch";
import {
  api,
  onTauriEvent,
  openFileDialog,
  type ApkCheckResult,
  type BuildInfo,
  type DragDropPayload,
  type ProtectProgress,
  type SignConfig,
  type UpdateCheckResult,
} from "./lib/tauri";
import { basename, isApk, protectedOutputPath, signedOutputPath } from "./lib/path";

type Page = "protect" | "sign" | "settings" | "about";
type ThemeMode = "system" | "light" | "dark";
type ProtectState = "idle" | "prechecking" | "running" | "done" | "failed";
type SignState = "idle" | "signing" | "done" | "failed";

const defaultSignConfig: SignConfig = {
  auto_sign_enabled: false,
  sign_v1: true,
  sign_v2: true,
  sign_v3: true,
  sign_v4: false,
  ks_type: "JKS",
};

const logoSvg =
  "data:image/svg+xml,%3Csvg%20xmlns%3D'http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg'%20viewBox%3D'0%200%201024%201024'%3E%20%3Cdefs%3E%20%3ClinearGradient%20id%3D'bg'%20x1%3D'180'%20y1%3D'120'%20x2%3D'860'%20y2%3D'900'%20gradientUnits%3D'userSpaceOnUse'%3E%20%3Cstop%20offset%3D'0'%20stop-color%3D'%2310141d'%2F%3E%20%3Cstop%20offset%3D'1'%20stop-color%3D'%230b6b66'%2F%3E%20%3C%2FlinearGradient%3E%20%3ClinearGradient%20id%3D'shield'%20x1%3D'300'%20y1%3D'180'%20x2%3D'760'%20y2%3D'830'%20gradientUnits%3D'userSpaceOnUse'%3E%20%3Cstop%20offset%3D'0'%20stop-color%3D'%232ee6f2'%2F%3E%20%3Cstop%20offset%3D'1'%20stop-color%3D'%230d8cf0'%2F%3E%20%3C%2FlinearGradient%3E%20%3C%2Fdefs%3E%20%3Crect%20width%3D'1024'%20height%3D'1024'%20rx%3D'224'%20fill%3D'url(%23bg)'%2F%3E%20%3Cpath%20d%3D'M512%20176c106%2070%20201%2090%20286%20109v212c0%20176-109%20300-286%20381-177-81-286-205-286-381V285c85-19%20180-39%20286-109z'%20fill%3D'url(%23shield)'%20%2F%3E%20%3Cpath%20d%3D'M512%20252c70%2042%20136%2060%20198%2075v164c0%20121-72%20209-198%20272V252z'%20fill%3D'%23ffffff'%20opacity%3D'.9'%20%2F%3E%20%3Cpath%20d%3D'M512%20252c70%2042%20136%2060%20198%2075v164c0%20121-72%20209-198%20272V252z'%20fill%3D'%23dffaff'%20opacity%3D'.35'%20%2F%3E%20%3C%2Fsvg%3E";

const stepLabels: Record<string, { zh: string; en: string }> = {
  CheckTools: { zh: "检查工具", en: "Check tools" },
  Unpack: { zh: "解包 APK", en: "Unpack APK" },
  ModifyManifest: { zh: "修改 Manifest", en: "Modify Manifest" },
  ProcessDex: { zh: "处理 DEX", en: "Process DEX" },
  InjectRuntime: { zh: "注入 Runtime", en: "Inject runtime" },
  Repack: { zh: "重打包", en: "Repack" },
  Sign: { zh: "自动签名", en: "Auto sign" },
};

function AppSidebarHeader({ locale }: { locale: Locale }) {
  const { state, toggleSidebar } = useSidebar();
  const collapsed = state === "collapsed";

  return (
    <SidebarHeader className="h-[82px] justify-center px-3">
      {collapsed ? (
        <button
          type="button"
          className="mx-auto flex h-10 w-10 items-center justify-center rounded-xl hover:bg-sidebar-accent"
          onClick={toggleSidebar}
          aria-label={t(locale, "expandSidebar")}
          title={t(locale, "expandSidebar")}
        >
          <img src={logoSvg} alt="Mocika Shield" className="h-8 w-8 rounded-lg" />
        </button>
      ) : (
        <div className="flex items-center gap-3">
          <img src={logoSvg} alt="Mocika Shield" className="ml-1.5 h-9 w-9 rounded-lg" />
          <div className="min-w-0 flex-1">
            <div className="truncate text-[15px] font-semibold">Mocika Shield</div>
            <div className="truncate text-xs text-muted-foreground">{t(locale, "appSubtitle")}</div>
          </div>
          <SidebarTrigger
            className="h-9 w-9 shrink-0"
            aria-label={t(locale, "collapseSidebar")}
            title={t(locale, "collapseSidebar")}
          />
        </div>
      )}
    </SidebarHeader>
  );
}

function AppButton({
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

function TextInput(props: React.InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className={cn(
        "h-9 w-full rounded-md border bg-background px-3 text-sm text-foreground placeholder:text-muted-foreground",
        "min-w-0",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
        props.className,
      )}
    />
  );
}

function SelectInput(props: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select
      {...props}
      className={cn(
        "h-9 w-full rounded-md border bg-background px-3 text-sm text-foreground",
        "min-w-0",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
        props.className,
      )}
    />
  );
}

function StatusMessage({
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

function DropZone({
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

function SelectedApkCard({
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

function useThemeMode() {
  const [mode, setMode] = useState<ThemeMode>(() => {
    const stored = localStorage.getItem("mocika-theme-mode");
    return stored === "light" || stored === "dark" || stored === "system" ? stored : "system";
  });

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const resolved = mode === "system" ? (media.matches ? "dark" : "light") : mode;
      document.documentElement.setAttribute("data-theme", resolved);
      localStorage.setItem("mocika-theme", resolved);
      localStorage.setItem("mocika-theme-mode", mode);
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [mode]);

  return [mode, setMode] as const;
}

function useClipboard(locale: Locale) {
  const [copied, setCopied] = useState(false);
  const copy = useCallback(
    async (text: string) => {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    },
    [setCopied],
  );
  return { copiedLabel: copied ? t(locale, "copied") : t(locale, "copy"), copy };
}

export function App() {
  const [page, setPage] = useState<Page>("protect");
  const [locale, setLocale] = useState<Locale>(detectSystemLocale);
  const [themeMode, setThemeMode] = useThemeMode();
  const [updateInfo, setUpdateInfo] = useState<UpdateCheckResult | null>(null);
  const [majorDialogOpen, setMajorDialogOpen] = useState(false);
  const [signConfig, setSignConfig] = useState<SignConfig>(defaultSignConfig);
  const [signConfigLoaded, setSignConfigLoaded] = useState(false);

  useEffect(() => {
    api.getLocale().then((value) => setLocale(value === "en" ? "en" : "zh")).catch(() => undefined);
  }, []);

  useEffect(() => {
    let disposed = false;
    api.getSignConfig()
      .then((value) => {
        if (!disposed) {
          setSignConfig({ ...defaultSignConfig, ...value, ks_type: value.ks_type || "JKS" });
        }
      })
      .catch(() => undefined)
      .finally(() => {
        if (!disposed) {
          setSignConfigLoaded(true);
        }
      });
    return () => {
      disposed = true;
    };
  }, []);

  useEffect(() => {
    let disposed = false;
    async function run() {
      try {
        const dismissed = await api.getDismissedVersion().catch(() => null);
        const result = await api.checkUpdate(false);
        if (disposed || !result.has_update || !result.latest_version) {
          return;
        }
        if (result.update_level !== "major" && dismissed === result.latest_version) {
          return;
        }
        setUpdateInfo(result);
        if (result.update_level === "major") {
          window.setTimeout(() => setMajorDialogOpen(true), 1200);
        }
      } catch {
        // 自动更新检查静默失败。
      }
    }
    run();
    return () => {
      disposed = true;
    };
  }, []);

  const primaryNavItems = [
    { key: "protect" as const, icon: ShieldCheck, label: t(locale, "navProtect") },
    { key: "sign" as const, icon: PencilLine, label: t(locale, "navSign") },
  ];
  const utilityNavItems = [
    { key: "settings" as const, icon: Settings, label: t(locale, "navSettings") },
    { key: "about" as const, icon: Info, label: t(locale, "navAbout") },
  ];
  return (
    <SidebarProvider>
      <div className="flex h-dvh w-full overflow-hidden bg-background text-foreground">
        <Sidebar collapsible="icon" className="border-r bg-sidebar">
          <AppSidebarHeader locale={locale} />
          <SidebarContent className="scrollbar-none px-0 group-data-[collapsible=icon]:items-center">
            <SidebarGroup className="p-0 group-data-[collapsible=icon]:items-center">
              <SidebarGroupContent>
                <SidebarMenu className="gap-2 group-data-[collapsible=icon]:items-center">
                  {primaryNavItems.map((item) => {
                    const Icon = item.icon;
                    return (
                      <SidebarMenuItem key={item.key}>
                        <SidebarMenuButton
                          type="button"
                          size="lg"
                          isActive={page === item.key}
                          tooltip={item.label}
                          className="mx-3 h-11 rounded-xl pl-[14px] group-data-[collapsible=icon]:mx-auto group-data-[collapsible=icon]:pl-0"
                          onClick={() => setPage(item.key)}
                        >
                          <Icon />
                          <span className="group-data-[collapsible=icon]:hidden">{item.label}</span>
                        </SidebarMenuButton>
                      </SidebarMenuItem>
                    );
                  })}
                </SidebarMenu>
              </SidebarGroupContent>
            </SidebarGroup>
          </SidebarContent>
          <SidebarFooter className="gap-0 px-0 pb-4 group-data-[collapsible=icon]:items-center">
            <SidebarMenu className="gap-2 group-data-[collapsible=icon]:items-center">
              {utilityNavItems.map((item) => {
                const Icon = item.icon;
                return (
                  <SidebarMenuItem key={item.key}>
                    <SidebarMenuButton
                      type="button"
                      size="lg"
                      isActive={page === item.key}
                      tooltip={item.label}
                      className="mx-3 h-11 rounded-xl pl-[14px] group-data-[collapsible=icon]:mx-auto group-data-[collapsible=icon]:pl-0"
                      onClick={() => setPage(item.key)}
                    >
                      <Icon />
                      <span className="group-data-[collapsible=icon]:hidden">{item.label}</span>
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                );
              })}
            </SidebarMenu>
          </SidebarFooter>
        </Sidebar>

      <SidebarInset className="flex min-w-0 flex-1 flex-col overflow-hidden">
        <UpdateBanner
          locale={locale}
          updateInfo={updateInfo}
          onDismiss={async () => {
            if (updateInfo?.latest_version) {
              await api.dismissUpdate(updateInfo.latest_version).catch(() => undefined);
            }
            setUpdateInfo(null);
          }}
        />
        <div className="scrollbar-none min-h-0 flex-1 overflow-auto">
          {page === "protect" && <ProtectPage locale={locale} signConfig={signConfig} signConfigLoaded={signConfigLoaded} />}
          {page === "sign" && (
            <SignPage
              locale={locale}
              signConfig={signConfig}
              signConfigLoaded={signConfigLoaded}
              onOpenSettings={() => setPage("settings")}
            />
          )}
          {page === "settings" && (
            <SettingsPage
              locale={locale}
              setLocale={setLocale}
              themeMode={themeMode}
              setThemeMode={setThemeMode}
              signConfig={signConfig}
              onSignConfigSaved={(value) => setSignConfig({ ...defaultSignConfig, ...value, ks_type: value.ks_type || "JKS" })}
            />
          )}
          {page === "about" && <AboutPage locale={locale} setUpdateInfo={setUpdateInfo} />}
        </div>
      </SidebarInset>

      {majorDialogOpen && updateInfo?.latest_version && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-6">
          <section className="app-panel w-full max-w-md p-6 text-center">
            <div className="mx-auto mb-4 flex h-12 w-12 items-center justify-center rounded-md bg-primary/12 text-primary">
              <Download className="h-6 w-6" />
            </div>
            <h2 className="text-lg font-semibold">{t(locale, "majorUpdate")}</h2>
            <p className="mt-2 text-sm text-muted-foreground">
              {t(locale, "updateAvailable")} v{updateInfo.latest_version}
            </p>
            <div className="mt-6 flex justify-center gap-3">
              <AppButton
                onClick={() => {
                  if (updateInfo.release_url) {
                    void api.openUrl(updateInfo.release_url);
                  }
                  setMajorDialogOpen(false);
                }}
              >
                <Download className="h-4 w-4" />
                {t(locale, "viewRelease")}
              </AppButton>
              <AppButton variant="secondary" onClick={() => setMajorDialogOpen(false)}>
                {t(locale, "ignore")}
              </AppButton>
            </div>
          </section>
        </div>
      )}
      </div>
    </SidebarProvider>
  );
}

function UpdateBanner({
  locale,
  updateInfo,
  onDismiss,
}: {
  locale: Locale;
  updateInfo: UpdateCheckResult | null;
  onDismiss: () => void;
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
            <AppButton size="sm" variant="ghost" onClick={() => void api.openUrl(updateInfo.release_url!)}>
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

function ProtectPage({
  locale,
  signConfig,
  signConfigLoaded,
}: {
  locale: Locale;
  signConfig: SignConfig;
  signConfigLoaded: boolean;
}) {
  const [input, setInput] = useState("");
  const [output, setOutput] = useState("");
  const [state, setState] = useState<ProtectState>("idle");
  const [dragActive, setDragActive] = useState(false);
  const [warning, setWarning] = useState("");
  const [error, setError] = useState("");
  const [precheck, setPrecheck] = useState("");
  const [currentStep, setCurrentStep] = useState("");
  const [messages, setMessages] = useState<string[]>([]);
  const { copiedLabel, copy } = useClipboard(locale);

  const autoSignReady = Boolean(
    signConfigLoaded && signConfig.auto_sign_enabled && signConfig.keystore_path && signConfig.key_alias,
  );

  const computedOutput = useMemo(() => {
    const protectedPath = protectedOutputPath(input);
    return autoSignReady ? signedOutputPath(protectedPath) : protectedPath;
  }, [autoSignReady, input]);

  useEffect(() => {
    setOutput(computedOutput);
  }, [computedOutput]);

  const handleSelected = useCallback(
    async (path: string) => {
      setWarning("");
      setError("");
      setPrecheck("");
      setMessages([]);
      setCurrentStep("");
      if (!isApk(path)) {
        setWarning(t(locale, "onlyApk"));
        return;
      }
      setInput(path);
      setOutput(protectedOutputPath(path));
      setState("prechecking");
      try {
        const result = await api.checkApk(path);
        const precheckError = precheckMessage(locale, result);
        if (precheckError) {
          setPrecheck(precheckError);
        }
        setState("idle");
      } catch {
        setPrecheck(t(locale, "apkCheckFailed"));
        setState("idle");
      }
    },
    [locale],
  );

  useEffect(() => {
    const unlisten: Promise<(() => void)[]> = Promise.all([
      onTauriEvent<ProtectProgress>("protect-progress", (payload) => {
        setCurrentStep(payload.step);
        setMessages((items) => [...items.slice(-7), payload.message]);
      }),
      onTauriEvent<string>("protect-error", (payload) => {
        setError(payload);
        setState("failed");
      }),
      onTauriEvent<void>("protect-done", () => setState("done")),
      onTauriEvent<DragDropPayload>("tauri://drag-drop", (payload) => {
        const first = payload.paths?.[0];
        setDragActive(false);
        if (first) {
          void handleSelected(first);
        }
      }),
      onTauriEvent<void>("tauri://drag-enter", () => setDragActive(true)),
      onTauriEvent<void>("tauri://drag-leave", () => setDragActive(false)),
    ]);
    return () => {
      void unlisten.then((items) => items.forEach((fn) => fn()));
    };
  }, [handleSelected]);

  async function browse() {
    const path = await openFileDialog("APK", ["apk"]);
    if (path) {
      await handleSelected(path);
    }
  }

  async function start() {
    if (!input || !output || precheck) {
      return;
    }
    setState("running");
    setError("");
    setMessages([]);
    setCurrentStep("CheckTools");
    try {
      const unsignedOutput = autoSignReady ? protectedOutputPath(input) : output;
      await api.protectApk(input, unsignedOutput);
      if (autoSignReady && signConfig.keystore_path && signConfig.key_alias) {
        setCurrentStep("Sign");
        const [ksPass] = await api.getSignPasswords();
        const compare = await api.compareCertFingerprints({
          apkPath: input,
          keystorePath: signConfig.keystore_path,
          ksPass,
          ksType: signConfig.ks_type ?? "JKS",
          keyAlias: signConfig.key_alias,
        });
        if (!compare.matches && !compare.error) {
          setWarning(t(locale, "signMismatch"));
        }
        await api.signApk({
          apkPath: unsignedOutput,
          outputPath: output,
          apksignerPath: null,
          keystorePath: signConfig.keystore_path,
          keyAlias: signConfig.key_alias,
          ksType: signConfig.ks_type ?? "JKS",
          signV1: signConfig.sign_v1,
          signV2: signConfig.sign_v2,
          signV3: signConfig.sign_v3,
          signV4: signConfig.sign_v4,
        });
        await api.deleteFile(`${output}.idsig`).catch(() => undefined);
      }
      setState("done");
    } catch (err) {
      setError(String(err));
      setState("failed");
    }
  }

  async function cancel() {
    await api.cancelProtect().catch(() => undefined);
  }

  const steps = autoSignReady
    ? ["CheckTools", "Unpack", "ModifyManifest", "ProcessDex", "InjectRuntime", "Repack", "Sign"]
    : ["CheckTools", "Unpack", "ModifyManifest", "ProcessDex", "InjectRuntime", "Repack"];
  const hasInput = Boolean(input);
  const showProgress = hasInput && (state === "running" || state === "done" || messages.length > 0);

  return (
    <section className="min-h-full px-10 py-9">
      {!hasInput ? (
        <div className="mx-auto w-full max-w-5xl">
          <h1 className="text-[28px] font-semibold tracking-normal">{t(locale, "protectTitle")}</h1>
          <div className="mt-16">
            <DropZone
              locale={locale}
              active={dragActive}
              title={t(locale, "dropApk")}
              subtitle={t(locale, "onlyApk")}
              onBrowse={browse}
            />
          </div>
          {warning && (
            <div className="mt-5">
              <StatusMessage kind="warning">{warning}</StatusMessage>
            </div>
          )}
        </div>
      ) : (
        <div className="mx-auto w-full max-w-6xl">
          <div className="flex flex-wrap items-center justify-between gap-4">
            <div className="min-w-0">
              <h1 className="text-[28px] font-semibold tracking-normal">{t(locale, "protectTitle")}</h1>
              <p className="mt-1 truncate text-sm text-muted-foreground">{basename(input)}</p>
            </div>
            {state === "running" ? (
              <AppButton variant="danger" onClick={cancel}>
                <Square className="h-4 w-4" />
                {t(locale, "cancel")}
              </AppButton>
            ) : state === "done" || state === "failed" ? (
              <AppButton
                variant="secondary"
                onClick={() => {
                  setInput("");
                  setOutput("");
                  setState("idle");
                  setError("");
                  setPrecheck("");
                  setWarning("");
                  setMessages([]);
                }}
              >
                <RotateCcw className="h-4 w-4" />
                {t(locale, "protectAnother")}
              </AppButton>
            ) : (
              <AppButton disabled={!input || !signConfigLoaded || Boolean(precheck) || state === "prechecking"} onClick={start}>
                {state === "prechecking" ? <Loader2 className="h-4 w-4 animate-spin" /> : <Play className="h-4 w-4" />}
                {state === "prechecking" ? t(locale, "prechecking") : t(locale, "startProtect")}
              </AppButton>
            )}
          </div>

          <div className="mt-8 grid gap-5 lg:grid-cols-[minmax(0,1fr)_320px]">
            <div className="space-y-4">
              <SelectedApkCard
                locale={locale}
                path={input}
                  output={output}
                disabled={state === "prechecking" || state === "running"}
                onChange={browse}
              />
              {warning && <StatusMessage kind="warning">{warning}</StatusMessage>}
              {hasInput && autoSignReady && (
                <StatusMessage kind="info">
                  <b>{t(locale, "autoSignConfigured")}</b>
                  <span className="ml-1">{t(locale, "autoSignWillRun")}</span>
                </StatusMessage>
              )}
              {precheck && (
                <StatusMessage kind="error">
                  <b>{t(locale, "precheckFailed")}：</b>
                  {precheck}
                </StatusMessage>
              )}
              {state === "done" && (
                <StatusMessage
                  kind="success"
                  action={
                    <AppButton size="sm" variant="secondary" onClick={() => void api.showInFolder(output)}>
                      <FolderOpen className="h-4 w-4" />
                      {t(locale, "showInFolder")}
                    </AppButton>
                  }
                >
                  {t(locale, "done")}
                </StatusMessage>
              )}
              {error && (
                <StatusMessage
                  kind="error"
                  action={
                    <AppButton size="sm" variant="secondary" onClick={() => void copy(error)}>
                      <Clipboard className="h-4 w-4" />
                      {copiedLabel}
                    </AppButton>
                  }
                >
                  <b>{t(locale, "errorDetail")}：</b>
                  {error}
                </StatusMessage>
              )}
            </div>

            <aside className="rounded-[14px] border bg-card p-4">
              <h2 className="mb-3 text-sm font-semibold">{showProgress ? t(locale, "running") : t(locale, "ready")}</h2>
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
                      <div key={`${item}-${index}`} className="break-words">{item}</div>
                    ))}
                  </div>
                </div>
              )}
            </aside>
          </div>
        </div>
      )}
    </section>
  );
}

function precheckMessage(locale: Locale, result: ApkCheckResult) {
  if (result.error) {
    return `${t(locale, "readApkFailed")}: ${result.error}`;
  }
  if (result.already_protected) {
    return t(locale, "alreadyProtected");
  }
  if (!result.is_signed) {
    return t(locale, "notSigned");
  }
  return "";
}

function SignPage({
  locale,
  signConfig,
  signConfigLoaded,
  onOpenSettings,
}: {
  locale: Locale;
  signConfig: SignConfig;
  signConfigLoaded: boolean;
  onOpenSettings: () => void;
}) {
  const [apkPath, setApkPath] = useState("");
  const [outputPath, setOutputPath] = useState("");
  const [state, setState] = useState<SignState>("idle");
  const [error, setError] = useState("");
  const [dragActive, setDragActive] = useState(false);
  const { copiedLabel, copy } = useClipboard(locale);

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

  async function browseApk() {
    const path = await openFileDialog("APK", ["apk"]);
    if (path) {
      setApkPath(path);
      setOutputPath(signedOutputPath(path));
      setState("idle");
      setError("");
    }
  }

  async function sign() {
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
    setState("signing");
    setError("");
    try {
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
  }

  const savedReady = Boolean(signConfigLoaded && signConfig.keystore_path && signConfig.key_alias);
  const hasApk = Boolean(apkPath);
  const enabledVersions = [
    signConfig.sign_v1 && "V1",
    signConfig.sign_v2 && "V2",
    signConfig.sign_v3 && "V3",
    signConfig.sign_v4 && "V4",
  ].filter(Boolean).join(" / ");

  return (
    <section className="min-h-full px-10 py-9">
      {!hasApk ? (
        <div className="mx-auto w-full max-w-5xl">
          <h1 className="text-[28px] font-semibold tracking-normal">{t(locale, "signTitle")}</h1>
          <div className="mt-16">
            <DropZone
              locale={locale}
              active={dragActive}
              title={t(locale, "dropApk")}
              subtitle={t(locale, "onlyApk")}
              onBrowse={browseApk}
            />
          </div>
          {signConfigLoaded && !savedReady && (
            <div className="mt-5">
              <StatusMessage
                kind="warning"
                action={
                  <AppButton size="sm" variant="secondary" onClick={onOpenSettings}>
                    <Settings className="h-4 w-4" />
                    {t(locale, "navSettings")}
                  </AppButton>
                }
              >
                {t(locale, "noSavedConfig")}
              </StatusMessage>
            </div>
          )}
          {error && (
            <div className="mt-5">
              <StatusMessage kind="error">{error}</StatusMessage>
            </div>
          )}
        </div>
      ) : (
        <div className="mx-auto w-full max-w-6xl">
          <div className="flex flex-wrap items-center justify-between gap-4">
            <div className="min-w-0">
              <h1 className="text-[28px] font-semibold tracking-normal">{t(locale, "signTitle")}</h1>
              <p className="mt-1 truncate text-sm text-muted-foreground">{basename(apkPath)}</p>
            </div>
            <div className="flex flex-wrap gap-2">
              <AppButton disabled={!apkPath || !signConfigLoaded || !savedReady || state === "signing"} onClick={sign}>
                {state === "signing" ? <Loader2 className="h-4 w-4 animate-spin" /> : <KeyRound className="h-4 w-4" />}
                {state === "signing" ? t(locale, "signing") : t(locale, "startSign")}
              </AppButton>
              {(state === "done" || state === "failed") && (
                <AppButton variant="secondary" onClick={() => { setApkPath(""); setOutputPath(""); setState("idle"); setError(""); }}>
                  <RotateCcw className="h-4 w-4" />
                  {t(locale, "signAnother")}
                </AppButton>
              )}
            </div>
          </div>

          <div className="mt-8 grid gap-5 lg:grid-cols-[minmax(0,1fr)_320px]">
            <div className="space-y-4">
              <SelectedApkCard
                locale={locale}
                path={apkPath}
                disabled={state === "signing"}
                onChange={browseApk}
              />
              <div className="rounded-[14px] border bg-card p-4">
                <label className="field-label" htmlFor="sign-output">{t(locale, "outputPath")}</label>
                <TextInput id="sign-output" className="mt-2 font-mono text-xs" value={outputPath} onChange={(e) => setOutputPath(e.target.value)} />
              </div>
              {state === "done" && (
                <StatusMessage kind="success" action={<AppButton size="sm" variant="secondary" onClick={() => void api.showInFolder(outputPath)}><FolderOpen className="h-4 w-4" />{t(locale, "showInFolder")}</AppButton>}>
                  {t(locale, "signDone")}
                </StatusMessage>
              )}
              {error && (
                <StatusMessage kind="error" action={<AppButton size="sm" variant="secondary" onClick={() => void copy(error)}><Clipboard className="h-4 w-4" />{copiedLabel}</AppButton>}>
                  {error}
                </StatusMessage>
              )}
            </div>

            <aside className="space-y-4 rounded-[14px] border bg-card p-4">
              <div>
                <h2 className="flex items-center gap-2 text-sm font-semibold"><KeyRound className="h-4 w-4" />{t(locale, "signConfig")}</h2>
                <p className="mt-1 text-sm text-muted-foreground">{t(locale, "signConfigSource")}</p>
              </div>
              <div className="divide-y rounded-xl bg-muted/50">
                <SummaryRow label={t(locale, "keystore")} value={signConfig.keystore_path || t(locale, "unknown")} muted={!signConfig.keystore_path} />
                <SummaryRow label={t(locale, "keyAlias")} value={signConfig.key_alias || t(locale, "unknown")} muted={!signConfig.key_alias} />
                <SummaryRow label={t(locale, "signVersions")} value={enabledVersions || "-"} />
              </div>
              {signConfigLoaded && !savedReady && (
                <StatusMessage
                  kind="warning"
                  action={
                    <AppButton size="sm" variant="secondary" onClick={onOpenSettings}>
                      <Settings className="h-4 w-4" />
                      {t(locale, "navSettings")}
                    </AppButton>
                  }
                >
                  {t(locale, "noSavedConfig")}
                </StatusMessage>
              )}
            </aside>
          </div>
        </div>
      )}
    </section>
  );
}

function PasswordControl({ id, label, value, onChange, show, setShow }: { id: string; label: string; value: string; onChange: (value: string) => void; show: boolean; setShow: (show: boolean) => void }) {
  return (
    <div className="flex min-w-0 gap-2">
      <TextInput id={id} className="flex-1" type={show ? "text" : "password"} value={value} onChange={(e) => onChange(e.target.value)} />
      <button type="button" className="icon-button shrink-0" onClick={() => setShow(!show)} aria-label={label}>
        {show ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
      </button>
    </div>
  );
}

function SettingsFieldRow({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="grid min-h-[56px] gap-2 border-b border-border/70 px-5 py-3 last:border-b-0 sm:grid-cols-[190px_minmax(0,1fr)] sm:items-center">
      <div className="min-w-0">
        <div className="text-[15px] font-medium text-foreground">{label}</div>
        {hint && <div className="field-hint mt-1">{hint}</div>}
      </div>
      <div className="min-w-0">{children}</div>
    </div>
  );
}

function SettingsGroup({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <section>
      <h2 className="mb-3 text-[17px] font-semibold">{title}</h2>
      <div className="overflow-hidden rounded-[18px] border bg-card">
        {children}
      </div>
    </section>
  );
}

function PillSegment<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: Array<{ value: T; label: string }>;
  onChange: (value: T) => void;
}) {
  return (
    <div className="inline-grid rounded-xl bg-muted p-1" style={{ gridTemplateColumns: `repeat(${options.length}, minmax(0, 1fr))` }}>
      {options.map((item) => {
        const active = value === item.value;
        return (
          <button
            key={item.value}
            type="button"
            className={cn(
              "min-h-8 rounded-lg px-5 text-sm font-semibold transition-colors",
              active ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:text-foreground",
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

function SettingsPage({
  locale,
  setLocale,
  themeMode,
  setThemeMode,
  signConfig,
  onSignConfigSaved,
}: {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  themeMode: ThemeMode;
  setThemeMode: (mode: ThemeMode) => void;
  signConfig: SignConfig;
  onSignConfigSaved: (config: SignConfig) => void;
}) {
  const [config, setConfig] = useState<SignConfig>({ ...defaultSignConfig, ...signConfig, ks_type: signConfig.ks_type || "JKS" });
  const [ksPass, setKsPass] = useState("");
  const [keyPass, setKeyPass] = useState("");
  const [showPass, setShowPass] = useState(false);
  const [aliases, setAliases] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const [detecting, setDetecting] = useState(false);
  const [status, setStatus] = useState<"idle" | "saved" | "failed">("idle");
  const [error, setError] = useState("");

  useEffect(() => {
    api.getSignPasswords().then(([ks, key]) => { setKsPass(ks); setKeyPass(key); }).catch(() => undefined);
  }, []);

  useEffect(() => {
    setConfig({ ...defaultSignConfig, ...signConfig, ks_type: signConfig.ks_type || "JKS" });
  }, [signConfig]);

  async function browseKeystore() {
    const path = await openFileDialog("Keystore", ["jks", "keystore", "p12", "pfx", "bks"]);
    if (path) {
      setConfig((old) => ({
        ...old,
        keystore_path: path,
        ks_type: path.toLowerCase().endsWith(".p12") || path.toLowerCase().endsWith(".pfx") ? "PKCS12" : "JKS",
      }));
      setAliases([]);
      setStatus("idle");
    }
  }

  async function detectAlias() {
    if (!config.keystore_path || !ksPass) {
      return;
    }
    setDetecting(true);
    setError("");
    try {
      const list = await api.listKeystoreAliases(config.keystore_path, ksPass, config.ks_type || "JKS");
      setAliases(list);
      if (list.length === 1) {
        setConfig((old) => ({ ...old, key_alias: list[0] }));
      }
    } catch (err) {
      setError(`${t(locale, "aliasFailed")}: ${String(err)}`);
    } finally {
      setDetecting(false);
    }
  }

  async function save() {
    setSaving(true);
    setError("");
    setStatus("idle");
    try {
      if (config.auto_sign_enabled && (!config.keystore_path || !config.key_alias || !ksPass)) {
        throw new Error(t(locale, "saveSignFirst"));
      }
      await api.saveSignConfig(config);
      await api.saveSignPasswords(ksPass, keyPass);
      onSignConfigSaved(config);
      setStatus("saved");
    } catch (err) {
      setError(String(err));
      setStatus("failed");
    } finally {
      setSaving(false);
    }
  }

  function updateLocale(next: Locale) {
    setLocale(next);
    void api.saveLocale(next);
  }

  const signingConfigured = Boolean(config.keystore_path && config.key_alias);
  return (
    <section className="mx-auto max-w-[820px] px-8 py-9">
      <h1 className="mb-8 text-[22px] font-semibold">{t(locale, "settingsTitle")}</h1>
      <div className="space-y-8">
        <SettingsGroup title={t(locale, "appearance")}>
          <SettingsFieldRow label={t(locale, "theme")}>
            <div className="flex justify-end">
              <PillSegment
                value={themeMode}
                onChange={setThemeMode}
                options={[
                  { value: "system", label: t(locale, "system") },
                  { value: "light", label: t(locale, "light") },
                  { value: "dark", label: t(locale, "dark") },
                ]}
              />
            </div>
          </SettingsFieldRow>
          <SettingsFieldRow label={t(locale, "language")}>
            <div className="flex justify-end">
              <PillSegment
                value={locale}
                onChange={updateLocale}
                options={[
                  { value: "zh", label: "中文" },
                  { value: "en", label: "English" },
                ]}
              />
            </div>
          </SettingsFieldRow>
        </SettingsGroup>

        <SettingsGroup title={t(locale, "defaultSignConfig")}>
            <SettingsFieldRow label={t(locale, "keystore")}>
              <div className="flex min-w-0 items-center justify-end gap-3">
                <span className="min-w-0 truncate font-mono text-sm text-foreground">
                  {config.keystore_path ? basename(config.keystore_path) : t(locale, "unknown")}
                </span>
                {config.keystore_path && <Check className="h-5 w-5 shrink-0 text-success" />}
                <AppButton className="shrink-0" size="sm" variant="secondary" onClick={browseKeystore}>
                  {t(locale, "browse")}...
                </AppButton>
              </div>
            </SettingsFieldRow>

            <SettingsFieldRow label={t(locale, "keystoreType")}>
              <SelectInput
                id="settings-kstype"
                className="sm:max-w-[220px]"
                value={config.ks_type ?? "JKS"}
                onChange={(e) => setConfig((old) => ({ ...old, ks_type: e.target.value }))}
              >
                <option value="JKS">JKS</option>
                <option value="PKCS12">PKCS12</option>
              </SelectInput>
            </SettingsFieldRow>

            <SettingsFieldRow label={t(locale, "keyAlias")}>
              <div className="grid min-w-0 gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
                <TextInput
                  id="settings-alias"
                  value={config.key_alias ?? ""}
                  onChange={(e) => setConfig((old) => ({ ...old, key_alias: e.target.value }))}
                />
                <AppButton className="shrink-0" variant="secondary" onClick={detectAlias} disabled={detecting}>
                  {detecting ? <Loader2 className="h-4 w-4 animate-spin" /> : <BadgeCheck className="h-4 w-4" />}
                  {detecting ? t(locale, "detecting") : t(locale, "detectAlias")}
                </AppButton>
              </div>
            </SettingsFieldRow>

            {aliases.length > 1 && (
              <SettingsFieldRow label={t(locale, "keyAlias")}>
                <SelectInput value={config.key_alias ?? ""} onChange={(e) => setConfig((old) => ({ ...old, key_alias: e.target.value }))}>
                  <option value="">{t(locale, "keyAlias")}</option>
                  {aliases.map((item) => <option key={item} value={item}>{item}</option>)}
                </SelectInput>
              </SettingsFieldRow>
            )}

            <SettingsFieldRow label={t(locale, "keystorePassword")}>
              <PasswordControl id="settings-kspass" label={t(locale, "keystorePassword")} value={ksPass} onChange={setKsPass} show={showPass} setShow={setShowPass} />
            </SettingsFieldRow>
            <SettingsFieldRow label={t(locale, "keyPassword")} hint={t(locale, "keyPasswordHint")}>
              <PasswordControl id="settings-keypass" label={t(locale, "keyPassword")} value={keyPass} onChange={setKeyPass} show={showPass} setShow={setShowPass} />
            </SettingsFieldRow>

            <SettingsFieldRow label={t(locale, "signVersions")}>
              <div className="flex flex-wrap justify-end gap-4">
                {(["v1", "v2", "v3", "v4"] as const).map((version) => {
                  const key = `sign_${version}` as keyof SignConfig;
                  return (
                    <label key={version} className="flex min-h-8 items-center gap-2 text-sm font-semibold">
                      <input type="checkbox" className="h-5 w-5 accent-primary" checked={Boolean(config[key])} onChange={(e) => setConfig((old) => ({ ...old, [key]: e.target.checked }))} />
                      {version.toUpperCase()}
                    </label>
                  );
                })}
              </div>
            </SettingsFieldRow>

        </SettingsGroup>

        <SettingsGroup title={t(locale, "protectSection")}>
          <SettingsFieldRow label={t(locale, "autoSignAfterProtect")} hint={signingConfigured ? t(locale, "ready") : t(locale, "noSavedConfig")}>
            <div className="flex justify-end">
              <Switch
                checked={Boolean(config.auto_sign_enabled)}
                onCheckedChange={(checked) => setConfig((old) => ({ ...old, auto_sign_enabled: checked }))}
                aria-label={t(locale, "autoSignAfterProtect")}
              />
            </div>
          </SettingsFieldRow>
        </SettingsGroup>

        <div className="space-y-3">
          {status === "saved" && <StatusMessage kind="success">{t(locale, "saved")}</StatusMessage>}
          {status === "failed" && <StatusMessage kind="error">{error || t(locale, "saveFailed")}</StatusMessage>}
          {error && status !== "failed" && <StatusMessage kind="error">{error}</StatusMessage>}
          <div className="flex justify-end">
            <AppButton onClick={save} disabled={saving}>
              {saving ? <Loader2 className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}
              {t(locale, "save")}
            </AppButton>
          </div>
        </div>
      </div>
    </section>
  );
}

function SummaryRow({ label, value, muted = false }: { label: string; value: string; muted?: boolean }) {
  return (
    <div className="grid gap-1 px-3 py-3 sm:grid-cols-[160px_minmax(0,1fr)] sm:gap-4">
      <div className="text-sm text-muted-foreground">{label}</div>
      <div className={cn("min-w-0 break-all text-sm", muted ? "text-muted-foreground" : "text-foreground")}>{value}</div>
    </div>
  );
}

function AboutPage({
  locale,
  setUpdateInfo,
}: {
  locale: Locale;
  setUpdateInfo: (result: UpdateCheckResult | null) => void;
}) {
  const [appInfo, setAppInfo] = useState({ version: "1.2.0-rc.1", git_hash: "dev", build_date: "unknown" });
  const [buildInfo, setBuildInfo] = useState<BuildInfo | null>(null);
  const [checking, setChecking] = useState(false);
  const [message, setMessage] = useState("");

  useEffect(() => {
    api.getAppInfo().then(setAppInfo).catch(() => undefined);
    api.getBuildInfo().then(setBuildInfo).catch(() => undefined);
  }, []);

  async function checkUpdate() {
    setChecking(true);
    setMessage("");
    try {
      const result = await api.checkUpdate(true);
      if (result.has_update && result.latest_version) {
        setUpdateInfo(result);
        setMessage(`${t(locale, "updateAvailable")} v${result.latest_version}`);
      } else {
        setMessage(t(locale, "upToDate"));
      }
    } catch {
      setMessage(t(locale, "updateFailed"));
    } finally {
      setChecking(false);
    }
  }

  return (
    <section className="flex min-h-full items-center justify-center px-8 py-10">
      <div className="w-full max-w-[640px] rounded-[28px] border bg-card px-10 py-12 text-center shadow-panel">
        <img src={logoSvg} alt="Mocika Shield" className="mx-auto h-24 w-24 rounded-[22px]" />
        <h1 className="mt-8 text-[34px] font-semibold tracking-normal">Mocika Shield</h1>
        <p className="mt-3 text-base font-medium text-muted-foreground">v{appInfo.version}</p>
        <p className="mt-4 text-[15px] font-medium text-muted-foreground">{t(locale, "appSubtitle")}</p>
        <p className="mt-4 font-mono text-sm text-muted-foreground">
          apktool {buildInfo?.apktool_version ?? t(locale, "unknown")}
          <span className="mx-3">·</span>
          apksigner {buildInfo?.apksigner_version ?? t(locale, "unknown")}
        </p>
        <div className="mt-8 flex justify-center">
          <AppButton variant="secondary" onClick={checkUpdate} disabled={checking}>
            {checking ? <Loader2 className="h-4 w-4 animate-spin" /> : <Download className="h-4 w-4" />}
            {checking ? t(locale, "checkingUpdate") : t(locale, "checkUpdate")}
          </AppButton>
        </div>
        {message && <p className="mt-5 text-sm font-medium text-muted-foreground">{message}</p>}
        <div className="mt-8 grid gap-2 rounded-2xl bg-muted/50 p-4 text-left">
          <SummaryRow label="Git" value={appInfo.git_hash || t(locale, "unknown")} muted={!appInfo.git_hash} />
          <SummaryRow label={t(locale, "build")} value={appInfo.build_date || t(locale, "unknown")} muted={!appInfo.build_date} />
        </div>
      </div>
    </section>
  );
}
