import { useState } from "react";
import { FolderKey, Info, PencilLine, Settings, ShieldCheck } from "lucide-react";
import { Toaster } from "sonner";
import { AppSidebarHeader, MajorUpdateDialog, UpdateBanner } from "@/components/app/common";
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
  SidebarSeparator,
} from "@/components/ui/sidebar";
import { useAppConfigState, useAutoUpdateNotice } from "@/hooks/use-app-config";
import { useAppliedThemeMode } from "@/hooks/use-applied-theme-mode";
import { useCertificatesState } from "@/hooks/use-certificates";
import { useRuntimeInfo } from "@/hooks/use-runtime-info";
import { t } from "@/lib/i18n";
import { api, type BuildInfo } from "@/lib/tauri";
import { AboutPage } from "@/pages/about-page";
import { CertificatesPage } from "@/pages/certificates-page";
import { ProtectPage } from "@/pages/protect-page";
import { SettingsPage } from "@/pages/settings-page";
import { SignPage } from "@/pages/sign-page";

type Page = "protect" | "sign" | "certificates" | "settings" | "about";

export function App() {
  const [page, setPage] = useState<Page>("protect");
  const { locale, setLocale, themeMode, setThemeMode } = useAppConfigState();
  const certificatesState = useCertificatesState();
  const { updateInfo, setUpdateInfo, majorDialogOpen, setMajorDialogOpen } = useAutoUpdateNotice();
  const { buildInfo, runtimeInfoLoaded, runtimeInfoRefreshing, refreshRuntimeInfo } = useRuntimeInfo();

  useAppliedThemeMode(themeMode);

  const primaryNavItems = [
    { key: "protect" as const, icon: ShieldCheck, label: t(locale, "navProtect") },
    { key: "sign" as const, icon: PencilLine, label: t(locale, "navSign") },
    { key: "certificates" as const, icon: FolderKey, label: t(locale, "navCertificates") },
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
        <Sidebar
          collapsible="icon"
          className="border-r border-sidebar-border/80 bg-sidebar/95 backdrop-blur supports-[backdrop-filter]:bg-sidebar/88"
        >
          <AppSidebarHeader locale={locale} />
          <SidebarContent className="scrollbar-none px-0 py-3 group-data-[collapsible=icon]:items-center">
            <SidebarGroup className="px-2 py-0 group-data-[collapsible=icon]:items-center">
              <SidebarGroupContent>
                <SidebarMenu className="gap-2 group-data-[collapsible=icon]:items-center">
                  {primaryNavItems.map((item) => (
                    <NavItem
                      key={item.key}
                      page={page}
                      item={item}
                      onSelect={() => setPage(item.key)}
                    />
                  ))}
                </SidebarMenu>
              </SidebarGroupContent>
            </SidebarGroup>
          </SidebarContent>
          <SidebarFooter className="gap-0 px-0 pb-5 pt-2 group-data-[collapsible=icon]:items-center">
            <div className="px-4 group-data-[collapsible=icon]:px-3">
              <SidebarSeparator className="mx-0 opacity-80" />
            </div>
            <SidebarMenu className="gap-2 px-2 pt-3 group-data-[collapsible=icon]:items-center">
              {utilityNavItems.map((item) => (
                <NavItem
                  key={item.key}
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
                defaultCertificate={certificatesState.defaultCertificate}
                certificatesLoaded={certificatesState.loaded}
                buildInfo={buildInfo}
                runtimeInfoLoaded={runtimeInfoLoaded}
                onOpenCertificates={() => setPage("certificates")}
              />
            )}
            {page === "sign" && (
              <SignPage
                locale={locale}
                certificates={certificatesState.certificates}
                certificatesLoaded={certificatesState.loaded}
                buildInfo={buildInfo}
                runtimeInfoLoaded={runtimeInfoLoaded}
                onOpenCertificates={() => setPage("certificates")}
              />
            )}
            {page === "certificates" && (
              <CertificatesPage
                locale={locale}
                runtimeInfoLoaded={runtimeInfoLoaded}
                certificatesState={certificatesState}
              />
            )}
            {page === "settings" && (
              <SettingsPage
                locale={locale}
                setLocale={setLocale}
                themeMode={themeMode}
                setThemeMode={setThemeMode}
              />
            )}
            {page === "about" && (
              <AboutPage
                locale={locale}
                setUpdateInfo={setUpdateInfo}
                buildInfo={buildInfo}
                runtimeInfoRefreshing={runtimeInfoRefreshing}
                onRefreshRuntimeInfo={() => void refreshRuntimeInfo()}
              />
            )}
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
        <Toaster
          position="top-right"
          richColors
          closeButton
          toastOptions={{
            classNames: {
              toast: "font-sans",
              title: "text-sm font-medium",
              description: "text-xs",
            },
          }}
        />
      </div>
    </SidebarProvider>
  );
}

export type RuntimeInfoProps = {
  buildInfo: BuildInfo | null;
  runtimeInfoLoaded: boolean;
};

function NavItem({
  page,
  item,
  onSelect,
}: {
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
        className="h-12 rounded-[18px] px-3.5 text-[15px] font-medium shadow-none transition-[background-color,color,transform] duration-200 data-[active=true]:bg-sidebar-accent data-[active=true]:shadow-sm group-data-[collapsible=icon]:!mx-auto group-data-[collapsible=icon]:!size-12 group-data-[collapsible=icon]:!justify-center group-data-[collapsible=icon]:!gap-0 group-data-[collapsible=icon]:!p-0"
        onClick={onSelect}
      >
        <Icon className="h-[22px] w-[22px] shrink-0" />
        <span className="min-w-0 flex-1 truncate transition-[opacity,transform,width] duration-200 group-data-[collapsible=icon]:hidden">
          {item.label}
        </span>
      </SidebarMenuButton>
    </SidebarMenuItem>
  );
}
