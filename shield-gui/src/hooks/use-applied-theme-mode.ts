import { useEffect } from "react";
import type { ThemeMode } from "@/lib/tauri";

export function normalizeThemeMode(value: string): ThemeMode {
  return value === "light" || value === "dark" || value === "system" ? value : "system";
}

export function useAppliedThemeMode(mode: ThemeMode) {
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const resolved = mode === "system" ? (media.matches ? "dark" : "light") : mode;
      document.documentElement.setAttribute("data-theme", resolved);
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [mode]);
}
