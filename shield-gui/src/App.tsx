import { useState } from "react";
import { Download, Info, PencilLine, Settings, ShieldCheck } from "lucide-react";
import { defaultSignConfig } from "@/components/app/branding";
import { AppButton, AppSidebarHeader, MajorUpdateDialog, UpdateBanner } from "@/components/app/common";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
} from "@/components/ui/sidebar";
import { useAppConfigState, useAutoUpdateNotice } from "@/hooks/use-app-config";
import { useAppliedThemeMode } from "@/hooks/use-applied-theme-mode";
import { t, type Locale } from "@/lib/i18n";
import { api } from "@/lib/tauri";
import { AboutPage } from "@/pages/about-page";
import { ProtectPage } from "@/pages/protect-page";
import { SettingsPage } from "@/pages/settings-page";
import { SignPage } from "@/pages/sign-page";

type Page = "protect" | "sign" | "settings" | "about";

export function App() {
  const [page, setPage] = useState<Page>("protect");
  const {
    locale,
    setLocale,
    themeMode,
    setThemeMode,
    signConfig,
    setSignConfig,
    keystorePassword,
    setKeystorePassword,
    keyPassword,
    setKeyPassword,
    signConfigLoaded,
  } = useAppConfigState();
  const { updateInfo, setUpdateInfo, majorDialogOpen, setMajorDialogOpen } = useAutoUpdateNotice();

  useAppliedThemeMode(themeMode);

  const primaryNavItems = [
    { key: "protect" as const, icon: ShieldCheck, label: t(locale, "navProtect") },
    { key: "sign" as const, icon: PencilLine, label: t(locale, "navSign") },
  ];
  const utilityNavItems = [
    { key: "settings" as const, icon: Settings, label: t(locale, "navSettings") },
    { key: "about" as const, icon: Info, label: t(locale, "navAbout") },
  ];

  async function dismissUpdate(version?: string) {
    if (version) {
      await api.dismissUpdate(version).catch(() => undefined);
    }
    setUpdateInfo(null);
  }

  function openRelease(url: string) {
    void api.openUrl(url);
  }

  return (
    <SidebarProvider>
      <div className="flex h-dvh w-full overflow-hidden bg-background text-foreground">
        <Sidebar collapsible="icon" className="border-r bg-sidebar">
          <AppSidebarHeader locale={locale} />
          <SidebarContent className="scrollbar-none px-0 group-data-[collapsible=icon]:items-center">
            <SidebarGroup className="p-0 group-data-[collapsible=icon]:items-center">
              <SidebarGroupContent>
                <SidebarMenu className="gap-2 group-data-[collapsible=icon]:items-center">
                  {primaryNavItems.map((item) => (
                    <NavItem
                      key={item.key}
                      locale={locale}
                      page={page}
                      item={item}
                      onSelect={() => setPage(item.key)}
                    />
                  ))}
                </SidebarMenu>
              </SidebarGroupContent>
            </SidebarGroup>
          </SidebarContent>
          <SidebarFooter className="gap-0 px-0 pb-4 group-data-[collapsible=icon]:items-center">
            <SidebarMenu className="gap-2 group-data-[collapsible=icon]:items-center">
              {utilityNavItems.map((item) => (
                <NavItem
                  key={item.key}
                  locale={locale}
                  page={page}
                  item={item}
                  onSelect={() => setPage(item.key)}
                />
              ))}
            </SidebarMenu>
          </SidebarFooter>
        </Sidebar>

        <SidebarInset className="flex min-w-0 flex-1 flex-col overflow-hidden">
          <UpdateBanner
            locale={locale}
            updateInfo={updateInfo}
            onDismiss={() => void dismissUpdate(updateInfo?.latest_version ?? undefined)}
            onViewRelease={openRelease}
          />
          <div className="scrollbar-none min-h-0 flex-1 overflow-auto">
            {page === "protect" && (
              <ProtectPage
                locale={locale}
                signConfig={signConfig}
                keystorePassword={keystorePassword}
                signConfigLoaded={signConfigLoaded}
              />
            )}
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
                keystorePassword={keystorePassword}
                keyPassword={keyPassword}
                onConfigSaved={(value) => {
                  setLocale(value.locale);
                  setThemeMode(value.themeMode);
                  setSignConfig({
                    ...defaultSignConfig,
                    ...value.signConfig,
                    ks_type: value.signConfig.ks_type || "JKS",
                  });
                  setKeystorePassword(value.keystorePassword);
                  setKeyPassword(value.keyPassword);
                }}
              />
            )}
            {page === "about" && <AboutPage locale={locale} setUpdateInfo={setUpdateInfo} />}
          </div>
        </SidebarInset>

        <MajorUpdateDialog
          locale={locale}
          open={majorDialogOpen}
          latestVersion={updateInfo?.latest_version ?? undefined}
          releaseUrl={updateInfo?.release_url}
          onClose={() => setMajorDialogOpen(false)}
          onViewRelease={openRelease}
        />
      </div>
    </SidebarProvider>
  );
}

function NavItem({
  locale,
  page,
  item,
  onSelect,
}: {
  locale: Locale;
  page: Page;
  item: {
    key: Page;
    icon: React.ComponentType<{ className?: string }>;
    label: string;
  };
  onSelect: () => void;
}) {
  const Icon = item.icon;

  return (
    <SidebarMenuItem>
      <SidebarMenuButton
        type="button"
        size="lg"
        isActive={page === item.key}
        tooltip={item.label}
        className="mx-3 h-11 rounded-xl pl-[14px] group-data-[collapsible=icon]:mx-auto group-data-[collapsible=icon]:pl-0"
        onClick={onSelect}
      >
        <Icon />
        <span className="group-data-[collapsible=icon]:hidden">{item.label}</span>
      </SidebarMenuButton>
    </SidebarMenuItem>
  );
}
