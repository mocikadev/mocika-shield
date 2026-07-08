import { AboutInfoCard } from "@/components/app/about-info-card";
import { useAboutPage } from "@/hooks/use-about-page";
import type { Locale } from "@/lib/i18n";
import { type UpdateCheckResult } from "@/lib/tauri";

export function AboutPage({
  locale,
  setUpdateInfo,
}: {
  locale: Locale;
  setUpdateInfo: (result: UpdateCheckResult | null) => void;
}) {
  const { appInfo, buildInfo, checking, message, checkUpdate } = useAboutPage({
    locale,
    setUpdateInfo,
  });

  return (
    <section className="flex min-h-full items-center justify-center px-8 py-10">
      <AboutInfoCard
        locale={locale}
        appInfo={appInfo}
        buildInfo={buildInfo}
        checking={checking}
        message={message}
        onCheckUpdate={checkUpdate}
      />
    </section>
  );
}
