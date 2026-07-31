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
  telemetry_enabled: boolean;
  protect_defaults?: ProtectDefaults;
};

export type ProtectDefaults = {
  runtime_mode: "standard" | "android_api19";
  environment_policy: "compatible" | "strict";
  sign_after_protect: boolean | null;
  certificate_id: string | null;
  output_directory_mode: "source" | "fixed";
  fixed_output_directory: string;
};

export type CertificateRecord = {
  id: string;
  name: string;
  source_type: "managed" | "external" | string;
  keystore_path: string;
  keystore_password: string;
  key_alias: string;
  key_password: string;
  ks_type: "JKS" | "PKCS12" | string;
  sign_v1: boolean;
  sign_v2: boolean;
  sign_v3: boolean;
  sign_v4: boolean;
  auto_sign_enabled: boolean;
  note: string;
  is_default: boolean;
  created_at: number;
  updated_at: number;
  last_verified_at?: number | null;
  last_verify_status: "unknown" | "success" | "failed" | string;
  last_verify_message?: string | null;
};

export type CertificateUpsertInput = {
  id?: string | null;
  name: string;
  source_type: "managed" | "external" | string;
  keystore_path: string;
  keystore_password: string;
  key_alias: string;
  key_password: string;
  ks_type?: "JKS" | "PKCS12" | string | null;
  sign_v1: boolean;
  sign_v2: boolean;
  sign_v3: boolean;
  sign_v4: boolean;
  auto_sign_enabled: boolean;
  note: string;
  set_as_default: boolean;
  copy_keystore_to_managed: boolean;
  managed_file_name?: string | null;
};

export type CertificateValidationInput = {
  keystore_path: string;
  keystore_password: string;
  key_alias: string;
  ks_type?: "JKS" | "PKCS12" | string | null;
};

export type CertificateValidationResult = {
  valid: boolean;
  aliases: string[];
  resolved_alias?: string | null;
  message?: string | null;
};

export type CreateManagedCertificateInput = {
  name: string;
  file_name: string;
  key_alias: string;
  keystore_password: string;
  key_password: string;
  ks_type?: "JKS" | "PKCS12" | string | null;
  sign_v1: boolean;
  sign_v2: boolean;
  sign_v3: boolean;
  sign_v4: boolean;
  auto_sign_enabled: boolean;
  note: string;
  set_as_default: boolean;
  dname: string;
  validity_days: number;
  key_size: number;
};

export type ApkCheckResult = {
  already_protected: boolean;
  is_signed: boolean;
  native_abis: string[];
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
  java_version: string;
  java_ready: boolean;
  keytool_ready: boolean;
  java_major?: number | null;
  min_java_major: number;
};

export type TaskKind = "protect" | "sign";
export type TaskStatus = "running" | "succeeded" | "failed" | "cancelled";

export type TaskLog = {
  timestamp_ms: number;
  step: string;
  level: "info" | "error";
  message: string;
};

export type TaskSnapshot = {
  task_id: string;
  kind: TaskKind;
  status: TaskStatus;
  current_step: string;
  input_path: string;
  output_path: string;
  started_at_ms: number;
  finished_at_ms?: number | null;
  logs: TaskLog[];
  error?: string | null;
};

export type DragDropPayload = {
  paths?: string[];
};

export const api = {
  checkApk: (path: string) => invoke<ApkCheckResult>("check_apk", { path }),
  protectApk: (taskId: string, input: string, output: string, runtimeMode: "standard" | "android_api19", environmentPolicy: "compatible" | "strict", signedOutput?: string | null, certificateId?: string | null) =>
    invoke<void>("protect_apk", {
      request: {
        taskId,
        input,
        output,
        signedOutput: signedOutput ?? null,
        apktoolPath: null,
        runtimeMode,
        environmentPolicy,
        certificateId: certificateId ?? null,
      },
    }),
  cancelProtect: () => invoke<void>("cancel_protect"),
  checkFileExists: (path: string) => invoke<boolean>("check_file_exists", { path }),
  showInFolder: (path: string) => invoke<void>("show_in_folder", { path }),
  deleteFile: (path: string) => invoke<void>("delete_file", { path }),
  getAppConfig: () => invoke<AppConfig>("get_app_config"),
  saveAppConfig: (config: AppConfig) => invoke<void>("save_app_config", { config }),
  saveProtectDefaults: (defaults: ProtectDefaults) => invoke<void>("save_protect_defaults", { defaults }),
  listCertificates: () => invoke<CertificateRecord[]>("list_certificates"),
  saveCertificate: (input: CertificateUpsertInput) =>
    invoke<CertificateRecord>("save_certificate", { input }),
  validateCertificate: (input: CertificateValidationInput) =>
    invoke<CertificateValidationResult>("validate_certificate", { input }),
  setDefaultCertificate: (id: string) =>
    invoke<void>("set_default_certificate", { id }),
  deleteCertificate: (id: string, removeKeystoreFile: boolean) =>
    invoke<CertificateRecord[]>("delete_certificate", { id, removeKeystoreFile }),
  verifyCertificate: (id: string) =>
    invoke<CertificateRecord>("verify_certificate", { id }),
  createManagedCertificate: (input: CreateManagedCertificateInput) =>
    invoke<CertificateRecord>("create_managed_certificate_command", { input }),
	  signApk: (args: {
	    taskId: string;
	    apkPath: string;
	    outputPath?: string | null;
	    apksignerPath?: string | null;
	    certificateId: string;
	  }) => invoke<void>("sign_apk", { request: args }),
  getLatestTask: (kind: TaskKind) => invoke<TaskSnapshot | null>("get_latest_task", { kind }),
  listKeystoreAliases: (keystorePath: string, ksPass: string, ksType: string) =>
    invoke<string[]>("list_keystore_aliases", { keystorePath, ksPass, ksType }),
	  compareCertFingerprints: (args: {
	    apkPath: string;
	    certificateId: string;
	  }) => invoke<CertCompareResult>("compare_cert_fingerprints", args),
  checkUpdate: (force: boolean) => invoke<UpdateCheckResult>("check_update", { force }),
  syncTelemetry: () => invoke<void>("sync_telemetry"),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  dismissUpdate: (version: string) => invoke<void>("dismiss_update", { version }),
  getDismissedVersion: () => invoke<string | null>("get_dismissed_version"),
  getAppInfo: () => invoke<AppInfo>("get_app_info"),
  getBuildInfo: () => invoke<BuildInfo>("get_build_info"),
  getDiagnosticInfo: () => invoke<string>("get_diagnostic_info"),
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

export async function openDirectoryDialog(defaultPath?: string) {
  const result = await open({ directory: true, multiple: false, defaultPath });
  if (Array.isArray(result)) return result[0] ?? null;
  return result ?? null;
}

export function onTauriEvent<T>(event: string, handler: (payload: T) => void) {
  return listen<T>(event, (e) => handler(e.payload));
}

export type { UnlistenFn };
