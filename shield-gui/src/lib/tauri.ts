import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

export type SignConfig = {
  keystore_path?: string | null;
  key_alias?: string | null;
  auto_sign_enabled: boolean;
  ks_type?: string | null;
  sign_v1: boolean;
  sign_v2: boolean;
  sign_v3: boolean;
  sign_v4: boolean;
};

export type ThemeMode = "system" | "light" | "dark";

export type AppConfig = {
  locale: "zh" | "en" | string;
  theme_mode: ThemeMode | string;
  sign_config: SignConfig;
  keystore_password: string;
  key_password: string;
};

export type ApkCheckResult = {
  already_protected: boolean;
  is_signed: boolean;
  error?: string | null;
};

export type CertCompareResult = {
  matches: boolean;
  apk_fingerprint?: string | null;
  ks_fingerprint?: string | null;
  error?: string | null;
};

export type UpdateCheckResult = {
  has_update: boolean;
  latest_version?: string | null;
  release_url?: string | null;
  update_level?: "patch" | "minor" | "major" | string | null;
};

export type AppInfo = {
  version: string;
  git_hash: string;
  build_date: string;
};

export type BuildInfo = {
  apktool_version: string;
  apksigner_version: string;
};

export type ProtectProgress = {
  step: string;
  message: string;
};

export type DragDropPayload = {
  paths?: string[];
};

export const api = {
  checkApk: (path: string) => invoke<ApkCheckResult>("check_apk", { path }),
  protectApk: (input: string, output: string) =>
    invoke<void>("protect_apk", {
      input,
      output,
      apktoolPath: null,
      resourcesPath: null,
    }),
  cancelProtect: () => invoke<void>("cancel_protect"),
  showInFolder: (path: string) => invoke<void>("show_in_folder", { path }),
  deleteFile: (path: string) => invoke<void>("delete_file", { path }),
  getAppConfig: () => invoke<AppConfig>("get_app_config"),
  saveAppConfig: (config: AppConfig) => invoke<void>("save_app_config", { config }),
  signApk: (args: {
    apkPath: string;
    outputPath?: string | null;
    apksignerPath?: string | null;
    keystorePath: string;
    keyAlias: string;
    ksType?: string | null;
    signV1: boolean;
    signV2: boolean;
    signV3: boolean;
    signV4: boolean;
  }) => invoke<void>("sign_apk", args),
  listKeystoreAliases: (keystorePath: string, ksPass: string, ksType: string) =>
    invoke<string[]>("list_keystore_aliases", { keystorePath, ksPass, ksType }),
  compareCertFingerprints: (args: {
    apkPath: string;
    keystorePath: string;
    ksPass: string;
    ksType?: string | null;
    keyAlias: string;
  }) => invoke<CertCompareResult>("compare_cert_fingerprints", args),
  checkUpdate: (force: boolean) => invoke<UpdateCheckResult>("check_update", { force }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  dismissUpdate: (version: string) => invoke<void>("dismiss_update", { version }),
  getDismissedVersion: () => invoke<string | null>("get_dismissed_version"),
  getAppInfo: () => invoke<AppInfo>("get_app_info"),
  getBuildInfo: () => invoke<BuildInfo>("get_build_info"),
};

export async function openFileDialog(
  filterName: string,
  extensions: string[],
  defaultPath?: string,
) {
  const result = await open({
    multiple: false,
    defaultPath,
    filters: [{ name: filterName, extensions }],
  });
  if (Array.isArray(result)) {
    return result[0] ?? null;
  }
  return result ?? null;
}

export function onTauriEvent<T>(event: string, handler: (payload: T) => void) {
  return listen<T>(event, (e) => handler(e.payload));
}

export type { UnlistenFn };
