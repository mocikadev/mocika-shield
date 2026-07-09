import type { Locale } from "@/lib/i18n";

export const logoSvg =
  "data:image/svg+xml,%3Csvg%20xmlns%3D%27http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%27%20viewBox%3D%270%200%201024%201024%27%3E%3Cdefs%3E%3ClinearGradient%20id%3D%27bg%27%20x1%3D%27174%27%20y1%3D%27134%27%20x2%3D%27842%27%20y2%3D%27890%27%20gradientUnits%3D%27userSpaceOnUse%27%3E%3Cstop%20offset%3D%270%27%20stop-color%3D%27%23121823%27%2F%3E%3Cstop%20offset%3D%271%27%20stop-color%3D%27%23087673%27%2F%3E%3C%2FlinearGradient%3E%3ClinearGradient%20id%3D%27shield%27%20x1%3D%27316%27%20y1%3D%27214%27%20x2%3D%27718%27%20y2%3D%27768%27%20gradientUnits%3D%27userSpaceOnUse%27%3E%3Cstop%20offset%3D%270%27%20stop-color%3D%27%2366f4f2%27%2F%3E%3Cstop%20offset%3D%271%27%20stop-color%3D%27%230da8f2%27%2F%3E%3C%2FlinearGradient%3E%3C%2Fdefs%3E%3Crect%20width%3D%271024%27%20height%3D%271024%27%20rx%3D%27224%27%20fill%3D%27url(%23bg)%27%2F%3E%3Cpath%20d%3D%27M512%20190c98%2062%20184%2082%20260%2098v196c0%20170-100%20292-260%20358-160-66-260-188-260-358V288c76-16%20162-36%20260-98Z%27%20fill%3D%27url(%23shield)%27%2F%3E%3Cpath%20d%3D%27M512%20294c55%2031%20107%2047%20158%2058v132c0%2097-58%20172-158%20224-100-52-158-127-158-224V352c51-11%20103-27%20158-58Z%27%20fill%3D%27%23ffffff%27%20opacity%3D%27.96%27%2F%3E%3Cpath%20d%3D%27M512%20382a40%2040%200%201%201%200%2080a40%2040%200%200%201%200-80Zm-26%2088h52v152h-52V470Z%27%20fill%3D%27url(%23bg)%27%2F%3E%3C%2Fsvg%3E";

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
