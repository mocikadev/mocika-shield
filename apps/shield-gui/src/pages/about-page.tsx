import { AboutInfoCard } from "@/components/app/about-info-card";
import { useAboutPage } from "@/hooks/use-about-page";
import type { Locale } from "@/lib/i18n";
import { type BuildInfo, type UpdateCheckResult } from "@/lib/tauri";

export function AboutPage({
  locale,
  setUpdateInfo,
  buildInfo,
  runtimeInfoRefreshing,
  onRefreshRuntimeInfo,
}: {
  locale: Locale;
  setUpdateInfo: (result: UpdateCheckResult | null) => void;
  buildInfo: BuildInfo | null;
  runtimeInfoRefreshing: boolean;
  onRefreshRuntimeInfo: () => void;
}) {
  const { appInfo, checking, copyingDiagnostic, message, checkUpdate, copyDiagnosticInfo } = useAboutPage({
    locale,
    setUpdateInfo,
  });

  return (
    <section className="mx-auto flex min-h-full w-full items-center justify-center px-8 py-12">
      <AboutInfoCard
        locale={locale}
        appInfo={appInfo}
        buildInfo={buildInfo}
        checking={checking}
        message={message}
        onCheckUpdate={checkUpdate}
        copyingDiagnostic={copyingDiagnostic}
        onCopyDiagnosticInfo={copyDiagnosticInfo}
        runtimeInfoRefreshing={runtimeInfoRefreshing}
        onRefreshRuntimeInfo={onRefreshRuntimeInfo}
      />
    </section>
  );
}
