import { Download, Loader2, RotateCcw } from "lucide-react";
import { logoSvg } from "@/components/app/branding";
import { AppButton, SummaryRow } from "@/components/app/common";
import { t, type Locale } from "@/lib/i18n";
import { getJavaStatusText } from "@/lib/java";
import type { BuildInfo } from "@/lib/tauri";

type AppInfo = {
  version: string;
  git_hash: string;
  build_date: string;
};

export function AboutInfoCard({
  locale,
  appInfo,
  buildInfo,
  checking,
  message,
  onCheckUpdate,
  runtimeInfoRefreshing,
  onRefreshRuntimeInfo,
}: {
  locale: Locale;
  appInfo: AppInfo;
  buildInfo: BuildInfo | null;
  checking: boolean;
  message: string;
  onCheckUpdate: () => void;
  runtimeInfoRefreshing: boolean;
  onRefreshRuntimeInfo: () => void;
}) {
  const javaStatus = getJavaStatusText(locale, buildInfo);

  return (
    <div className="mx-auto flex w-full max-w-[620px] flex-col items-center text-center">
      <img src={logoSvg} alt="Mocika Shield" className="h-16 w-16 rounded-[18px] shadow-sm" />
      <h1 className="mt-5 text-[30px] font-semibold tracking-normal">Mocika Shield</h1>
      <div className="mt-2 rounded-full border border-border/70 bg-muted/45 px-3 py-1 text-xs font-semibold text-muted-foreground">
        v{appInfo.version}
      </div>

      <div className="mt-6 flex flex-wrap justify-center gap-2">
        <span className="rounded-full border border-border/70 bg-muted/35 px-3 py-1.5 font-mono text-xs text-muted-foreground">
          {t(locale, "java")} {buildInfo?.java_version ?? t(locale, "unknown")}
        </span>
        <span className="rounded-full border border-border/70 bg-muted/35 px-3 py-1.5 font-mono text-xs text-muted-foreground">
          apktool {buildInfo?.apktool_version ?? t(locale, "unknown")}
        </span>
        <span className="rounded-full border border-border/70 bg-muted/35 px-3 py-1.5 font-mono text-xs text-muted-foreground">
          apksigner {buildInfo?.apksigner_version ?? t(locale, "unknown")}
        </span>
      </div>

      <div className="mt-8 w-full max-w-[460px] border-y border-border/60 py-2 text-left">
        <SummaryRow label={t(locale, "java")} value={javaStatus || t(locale, "unknown")} muted={!javaStatus} />
        <div className="border-t border-border/60" />
        <SummaryRow label="Git" value={appInfo.git_hash || t(locale, "unknown")} muted={!appInfo.git_hash} />
        <div className="border-t border-border/60" />
        <SummaryRow
          label={t(locale, "build")}
          value={appInfo.build_date || t(locale, "unknown")}
          muted={!appInfo.build_date}
        />
      </div>

      <div className="mt-6 flex flex-col items-center gap-3">
        <div className="flex flex-wrap justify-center gap-3">
          <AppButton variant="secondary" onClick={onRefreshRuntimeInfo} disabled={runtimeInfoRefreshing}>
            {runtimeInfoRefreshing ? <Loader2 className="h-4 w-4 animate-spin" /> : <RotateCcw className="h-4 w-4" />}
            {runtimeInfoRefreshing ? t(locale, "checkingEnvironment") : t(locale, "refreshEnvironment")}
          </AppButton>
          <AppButton variant="secondary" onClick={onCheckUpdate} disabled={checking}>
            {checking ? <Loader2 className="h-4 w-4 animate-spin" /> : <Download className="h-4 w-4" />}
            {checking ? t(locale, "checkingUpdate") : t(locale, "checkUpdate")}
          </AppButton>
        </div>
        {message && <p className="text-sm text-muted-foreground">{message}</p>}
      </div>
    </div>
  );
}
