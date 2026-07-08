import { useCallback, useState } from "react";
import { t, type Locale } from "@/lib/i18n";

export function useClipboard(locale: Locale) {
  const [copied, setCopied] = useState(false);

  const copy = useCallback(async (text: string) => {
    await navigator.clipboard.writeText(text);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1600);
  }, []);

  return { copiedLabel: copied ? t(locale, "copied") : t(locale, "copy"), copy };
}
