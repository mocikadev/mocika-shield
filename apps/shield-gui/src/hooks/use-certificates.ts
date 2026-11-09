import { useCallback, useEffect, useMemo, useState } from "react";
import { notifyError } from "@/lib/notify";
import { PreferenceSync } from "@/lib/preference-sync";
import {
  api,
  type CertificateRecord,
  type CertificateUpsertInput,
  type CertificateValidationInput,
  type CertificateValidationResult,
  type CreateManagedCertificateInput,
} from "@/lib/tauri";

function pickNextSelectedId(certificates: CertificateRecord[], current: string | null) {
  if (current && certificates.some((item) => item.id === current)) {
    return current;
  }
  return certificates.find((item) => item.is_default)?.id ?? certificates[0]?.id ?? null;
}

export function useCertificatesState() {
  const [certificates, setCertificates] = useState<CertificateRecord[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [sync] = useState(() => new PreferenceSync());

  const replaceCertificate = useCallback((next: CertificateRecord) => {
    setCertificates((items) => {
      const replaced = items.some((item) => item.id === next.id)
        ? items.map((item) => (item.id === next.id ? next : item))
        : [next, ...items];
      return replaced.map((item) => ({
        ...item,
        is_default: next.is_default ? item.id === next.id : item.is_default,
      }));
    });
    setSelectedId(next.id);
  }, []);

  const refresh = useCallback(async (silent = false) => {
    const token = sync.beginRead();
    if (token === undefined) return;
    setLoading(true);
    try {
      const next = await api.listCertificates();
      if (!sync.canApply(token)) return;
      setError("");
      setCertificates(next);
      setSelectedId((current) => pickNextSelectedId(next, current));
    } catch (err) {
      if (!sync.canApply(token)) return;
      const message = String(err);
      setError(message);
      if (!silent) notifyError(message);
    } finally {
      if (sync.canApply(token)) { setLoading(false); setLoaded(true); }
    }
  }, [sync]);

  const mutate = useCallback(async <T,>(operation: () => Promise<T>) => {
    try { return await sync.write(operation); }
    finally { if (!sync.busy) await refresh(true); }
  }, [sync, refresh]);

  useEffect(() => {
    void refresh();
    const onFocus = () => { void refresh(true); };
    const onVisibility = () => { if (document.visibilityState === "visible") void refresh(true); };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      sync.invalidate();
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [refresh, sync]);

  const saveCertificate = useCallback(
    async (input: CertificateUpsertInput) => mutate(async () => {
      const saved = await api.saveCertificate(input);
      replaceCertificate(saved);
      if (saved.is_default) {
        setCertificates((items) => items.map((item) => ({ ...item, is_default: item.id === saved.id })));
      }
      return saved;
    }),
    [replaceCertificate, mutate],
  );

  const createManagedCertificate = useCallback(
    async (input: CreateManagedCertificateInput) => mutate(async () => {
      const saved = await api.createManagedCertificate(input);
      replaceCertificate(saved);
      if (saved.is_default) {
        setCertificates((items) => items.map((item) => ({ ...item, is_default: item.id === saved.id })));
      }
      return saved;
    }),
    [replaceCertificate, mutate],
  );

  const setDefaultCertificate = useCallback(async (id: string) => mutate(async () => {
    setError("");
    try {
      await api.setDefaultCertificate(id);
      setCertificates((items) => items.map((item) => ({ ...item, is_default: item.id === id })));
      setSelectedId(id);
    } catch (err) {
      const message = String(err);
      setError(message);
      notifyError(message);
      throw err;
    }
  }), [mutate]);

  const deleteCertificate = useCallback(async (id: string, removeKeystoreFile: boolean) => mutate(async () => {
    const next = await api.deleteCertificate(id, removeKeystoreFile);
    setCertificates(next);
    setSelectedId((current) => {
      if (current !== id) {
        return pickNextSelectedId(next, current);
      }
      return pickNextSelectedId(next, null);
    });
    return next;
  }), [mutate]);

  const verifyCertificate = useCallback(
    async (id: string) => mutate(async () => {
      const verified = await api.verifyCertificate(id);
      replaceCertificate(verified);
      return verified;
    }),
    [replaceCertificate, mutate],
  );

  const validateCertificate = useCallback(
    (input: CertificateValidationInput): Promise<CertificateValidationResult> =>
      api.validateCertificate(input),
    [],
  );

  const selectedCertificate = useMemo(
    () => certificates.find((item) => item.id === selectedId) ?? null,
    [certificates, selectedId],
  );
  const defaultCertificate = useMemo(
    () => certificates.find((item) => item.is_default) ?? null,
    [certificates],
  );

  return {
    certificates,
    selectedId,
    setSelectedId,
    selectedCertificate,
    defaultCertificate,
    loaded,
    loading,
    error,
    refresh,
    saveCertificate,
    createManagedCertificate,
    setDefaultCertificate,
    deleteCertificate,
    verifyCertificate,
    validateCertificate,
  };
}

export type CertificatesState = ReturnType<typeof useCertificatesState>;
