import { Download, Loader2 } from "lucide-react";
import { logoSvg } from "@/components/app/branding";
import { AppButton, SummaryRow } from "@/components/app/common";
import { t, type Locale } from "@/lib/i18n";
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
}: {
  locale: Locale;
  appInfo: AppInfo;
  buildInfo: BuildInfo | null;
  checking: boolean;
  message: string;
  onCheckUpdate: () => void;
}) {
  return (
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
        <AppButton variant="secondary" onClick={onCheckUpdate} disabled={checking}>
          {checking ? <Loader2 className="h-4 w-4 animate-spin" /> : <Download className="h-4 w-4" />}
          {checking ? t(locale, "checkingUpdate") : t(locale, "checkUpdate")}
        </AppButton>
      </div>
      {message && <p className="mt-5 text-sm font-medium text-muted-foreground">{message}</p>}
      <div className="mt-8 grid gap-2 rounded-2xl bg-muted/50 p-4 text-left">
        <SummaryRow label="Git" value={appInfo.git_hash || t(locale, "unknown")} muted={!appInfo.git_hash} />
        <SummaryRow
          label={t(locale, "build")}
          value={appInfo.build_date || t(locale, "unknown")}
          muted={!appInfo.build_date}
        />
      </div>
    </div>
  );
}
