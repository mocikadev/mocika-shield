import { KeyRound, Settings } from "lucide-react";
import { AppButton, StatusMessage, SummaryRow } from "@/components/app/common";
import { t, type Locale } from "@/lib/i18n";
import type { SignConfig } from "@/lib/tauri";

export function SignConfigSummaryCard({
  locale,
  signConfig,
  signConfigLoaded,
  savedReady,
  enabledVersions,
  onOpenSettings,
}: {
  locale: Locale;
  signConfig: SignConfig;
  signConfigLoaded: boolean;
  savedReady: boolean;
  enabledVersions: string;
  onOpenSettings: () => void;
}) {
  return (
    <aside className="space-y-4 rounded-[14px] border bg-card p-4">
      <div>
        <h2 className="flex items-center gap-2 text-sm font-semibold">
          <KeyRound className="h-4 w-4" />
          {t(locale, "signConfig")}
        </h2>
        <p className="mt-1 text-sm text-muted-foreground">{t(locale, "signConfigSource")}</p>
      </div>
      <div className="divide-y rounded-xl bg-muted/50">
        <SummaryRow
          label={t(locale, "keystore")}
          value={signConfig.keystore_path || t(locale, "unknown")}
          muted={!signConfig.keystore_path}
        />
        <SummaryRow
          label={t(locale, "keyAlias")}
          value={signConfig.key_alias || t(locale, "unknown")}
          muted={!signConfig.key_alias}
        />
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
  );
}
