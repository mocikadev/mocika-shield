import { useEffect, useRef, useState } from "react";
import { PillSegment, SettingsFieldRow, SettingsGroup, StatusMessage } from "@/components/app/common";
import { t, type Locale } from "@/lib/i18n";
import { notifyError } from "@/lib/notify";
import { api, type ThemeMode } from "@/lib/tauri";

export function SettingsPage({
  locale,
  setLocale,
  themeMode,
  setThemeMode,
}: {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  themeMode: ThemeMode;
  setThemeMode: (mode: ThemeMode) => void;
}) {
  const [selectedLocale, setSelectedLocale] = useState<Locale>(locale);
  const [selectedThemeMode, setSelectedThemeMode] = useState<ThemeMode>(themeMode);
  const [saving, setSaving] = useState(false);
  const [status, setStatus] = useState<"idle" | "saved" | "failed">("idle");
  const [error, setError] = useState("");
  const timerRef = useRef<number | null>(null);

  useEffect(() => {
    setSelectedLocale(locale);
  }, [locale]);

  useEffect(() => {
    setSelectedThemeMode(themeMode);
  }, [themeMode]);

  useEffect(() => () => {
    if (timerRef.current) {
      window.clearTimeout(timerRef.current);
    }
  }, []);

  async function persist(nextLocale: Locale, nextThemeMode: ThemeMode) {
    setSaving(true);
    setError("");
    try {
      await api.saveAppConfig({
        locale: nextLocale,
        theme_mode: nextThemeMode,
      });
      setLocale(nextLocale);
      setThemeMode(nextThemeMode);
      setStatus("saved");
      if (timerRef.current) {
        window.clearTimeout(timerRef.current);
      }
      timerRef.current = window.setTimeout(() => {
        setStatus("idle");
        timerRef.current = null;
      }, 1400);
    } catch (err) {
      const message = String(err);
      setStatus("failed");
      setError(message);
      notifyError(message);
    } finally {
      setSaving(false);
    }
  }

  return (
    <section className="mx-auto max-w-[920px] px-8 py-10">
      <header className="mb-9">
        <h1 className="text-[24px] font-semibold tracking-normal">{t(locale, "settingsTitle")}</h1>
      </header>

      <div className="space-y-10">
        <SettingsGroup title={t(locale, "appearance")}>
          <SettingsFieldRow label={t(locale, "theme")}>
            <div className="flex flex-wrap justify-end">
              <PillSegment
                value={selectedThemeMode}
                onChange={(value) => {
                  setSelectedThemeMode(value);
                  void persist(selectedLocale, value);
                }}
                options={[
                  { value: "system", label: t(locale, "system") },
                  { value: "light", label: t(locale, "light") },
                  { value: "dark", label: t(locale, "dark") },
                ]}
              />
            </div>
          </SettingsFieldRow>
          <SettingsFieldRow label={t(locale, "language")}>
            <div className="flex flex-wrap justify-end">
              <PillSegment
                value={selectedLocale}
                onChange={(value) => {
                  setSelectedLocale(value);
                  void persist(value, selectedThemeMode);
                }}
                options={[
                  { value: "zh", label: "中文" },
                  { value: "en", label: "English" },
                ]}
              />
            </div>
          </SettingsFieldRow>
        </SettingsGroup>
        {status === "saved" && (
          <StatusMessage kind="success">
            {saving ? t(locale, "saving") : t(locale, "saved")}
          </StatusMessage>
        )}
        {status === "failed" && error && <StatusMessage kind="error">{error}</StatusMessage>}
      </div>
    </section>
  );
}
