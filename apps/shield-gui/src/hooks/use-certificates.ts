import { useCallback, useEffect, useMemo, useState } from "react";
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

  const refresh = useCallback(async () => {
    setLoading(true);
    setError("");
    try {
      const next = await api.listCertificates();
      setCertificates(next);
      setSelectedId((current) => pickNextSelectedId(next, current));
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
      setLoaded(true);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const saveCertificate = useCallback(
    async (input: CertificateUpsertInput) => {
      const saved = await api.saveCertificate(input);
      replaceCertificate(saved);
      if (saved.is_default) {
        setCertificates((items) => items.map((item) => ({ ...item, is_default: item.id === saved.id })));
      }
      return saved;
    },
    [replaceCertificate],
  );

  const createManagedCertificate = useCallback(
    async (input: CreateManagedCertificateInput) => {
      const saved = await api.createManagedCertificate(input);
      replaceCertificate(saved);
      if (saved.is_default) {
        setCertificates((items) => items.map((item) => ({ ...item, is_default: item.id === saved.id })));
      }
      return saved;
    },
    [replaceCertificate],
  );

  const setDefaultCertificate = useCallback(async (id: string) => {
    const next = await api.setDefaultCertificate(id);
    setCertificates(next);
    setSelectedId(id);
    return next;
  }, []);

  const deleteCertificate = useCallback(async (id: string, removeKeystoreFile: boolean) => {
    const next = await api.deleteCertificate(id, removeKeystoreFile);
    setCertificates(next);
    setSelectedId((current) => {
      if (current !== id) {
        return pickNextSelectedId(next, current);
      }
      return pickNextSelectedId(next, null);
    });
    return next;
  }, []);

  const verifyCertificate = useCallback(
    async (id: string) => {
      const verified = await api.verifyCertificate(id);
      replaceCertificate(verified);
      return verified;
    },
    [replaceCertificate],
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
