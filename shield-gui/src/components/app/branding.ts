import type { Locale } from "@/lib/i18n";
import type { SignConfig } from "@/lib/tauri";

export const defaultSignConfig: SignConfig = {
  auto_sign_enabled: false,
  sign_v1: true,
  sign_v2: true,
  sign_v3: true,
  sign_v4: false,
  ks_type: "JKS",
};

export const logoSvg =
  "data:image/svg+xml,%3Csvg%20xmlns%3D'http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg'%20viewBox%3D'0%200%201024%201024'%3E%20%3Cdefs%3E%20%3ClinearGradient%20id%3D'bg'%20x1%3D'180'%20y1%3D'120'%20x2%3D'860'%20y2%3D'900'%20gradientUnits%3D'userSpaceOnUse'%3E%20%3Cstop%20offset%3D'0'%20stop-color%3D'%2310141d'%2F%3E%20%3Cstop%20offset%3D'1'%20stop-color%3D'%230b6b66'%2F%3E%20%3C%2FlinearGradient%3E%20%3ClinearGradient%20id%3D'shield'%20x1%3D'300'%20y1%3D'180'%20x2%3D'760'%20y2%3D'830'%20gradientUnits%3D'userSpaceOnUse'%3E%20%3Cstop%20offset%3D'0'%20stop-color%3D'%232ee6f2'%2F%3E%20%3Cstop%20offset%3D'1'%20stop-color%3D'%230d8cf0'%2F%3E%20%3C%2FlinearGradient%3E%20%3C%2Fdefs%3E%20%3Crect%20width%3D'1024'%20height%3D'1024'%20rx%3D'224'%20fill%3D'url(%23bg)'%2F%3E%20%3Cpath%20d%3D'M512%20176c106%2070%20201%2090%20286%20109v212c0%20176-109%20300-286%20381-177-81-286-205-286-381V285c85-19%20180-39%20286-109z'%20fill%3D'url(%23shield)'%20%2F%3E%20%3Cpath%20d%3D'M512%20252c70%2042%20136%2060%20198%2075v164c0%20121-72%20209-198%20272V252z'%20fill%3D'%23ffffff'%20opacity%3D'.9'%20%2F%3E%20%3Cpath%20d%3D'M512%20252c70%2042%20136%2060%20198%2075v164c0%20121-72%20209-198%20272V252z'%20fill%3D'%23dffaff'%20opacity%3D'.35'%20%2F%3E%20%3C%2Fsvg%3E";

export const stepLabels: Record<string, Record<Locale, string>> = {
  CheckTools: { zh: "检查工具", en: "Check tools" },
  Unpack: { zh: "解包 APK", en: "Unpack APK" },
  ModifyManifest: { zh: "修改 Manifest", en: "Modify Manifest" },
  ProcessDex: { zh: "处理 DEX", en: "Process DEX" },
  InjectRuntime: { zh: "注入 Runtime", en: "Inject runtime" },
  Repack: { zh: "重打包", en: "Repack" },
  AlignApk: { zh: "对齐 APK", en: "Align APK" },
  Sign: { zh: "自动签名", en: "Auto sign" },
};
