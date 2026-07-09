import { type ReactNode, useEffect, useState } from "react";
import { Check, FilePenLine, FolderOpen, Loader2, MoreHorizontal, Plus, Search, ShieldCheck, Star, Trash2, X } from "lucide-react";
import {
  AppButton,
  PasswordControl,
  SelectInput,
  StatusMessage,
  TextInput,
} from "@/components/app/common";
import { Switch } from "@/components/ui/switch";
import type { CertificatesState } from "@/hooks/use-certificates";
import { notifyError, notifySuccess } from "@/lib/notify";
import { basename } from "@/lib/path";
import { api, openFileDialog, type CertificateUpsertInput, type CreateManagedCertificateInput } from "@/lib/tauri";
import { t, type Locale } from "@/lib/i18n";

function emptyImportDraft(): CertificateUpsertInput {
  return {
    id: null,
    name: "",
    source_type: "external",
    keystore_path: "",
    keystore_password: "",
    key_alias: "",
    key_password: "",
    ks_type: "JKS",
    sign_v1: true,
    sign_v2: true,
    sign_v3: true,
    sign_v4: false,
    auto_sign_enabled: true,
    note: "",
    set_as_default: false,
    copy_keystore_to_managed: false,
    managed_file_name: "",
  };
}

function emptyCreateDraft(): CreateManagedCertificateInput {
  return {
    name: "",
    file_name: "release.p12",
    key_alias: "",
    keystore_password: "",
    key_password: "",
    ks_type: "PKCS12",
    sign_v1: true,
    sign_v2: true,
    sign_v3: true,
    sign_v4: false,
    auto_sign_enabled: true,
    note: "",
    set_as_default: false,
    dname: "CN=Mocika Shield, OU=Release, O=Mocika, L=Shanghai, ST=Shanghai, C=CN",
    validity_days: 3650,
    key_size: 2048,
  };
}

function validateCreateDraft(input: CreateManagedCertificateInput, locale: Locale): string | null {
  if (!input.name.trim()) return t(locale, "createCertNameRequired");
  if (!input.key_alias.trim()) return t(locale, "createCertAliasRequired");
  if (!input.keystore_password.trim()) return t(locale, "createCertKeystorePasswordRequired");
  if (input.keystore_password.trim().length < 6) return t(locale, "createCertPasswordTooShort");
  if (input.key_password.trim() && input.key_password.trim().length < 6) return t(locale, "createCertKeyPasswordTooShort");
  if (!input.dname.trim()) return t(locale, "createCertDnameRequired");
  if (!Number.isFinite(input.validity_days) || input.validity_days <= 0) return t(locale, "createCertValidityInvalid");
  if (![2048, 4096].includes(input.key_size)) return t(locale, "createCertKeySizeInvalid");
  return null;
}

function stripKeystoreExtension(fileName: string): string {
  return fileName.replace(/\.(jks|keystore|p12|pfx|bks)$/i, "");
}

function CertificateModal({
  open,
  title,
  onClose,
  children,
  locale,
}: {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
  locale: Locale;
}) {
  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-6">
      <section className="app-panel w-full max-w-2xl overflow-hidden">
        <header className="flex items-start justify-between gap-4 border-b px-6 py-5">
          <div className="min-w-0">
            <h2 className="text-[18px] font-semibold tracking-normal">{title}</h2>
          </div>
          <button
            type="button"
            className="icon-button h-9 w-9 shrink-0"
            onClick={onClose}
            aria-label={t(locale, "cancel")}
          >
            <X className="h-4 w-4" />
          </button>
        </header>
        <div className="max-h-[min(78vh,820px)] overflow-auto px-6 py-5 scrollbar-none">{children}</div>
      </section>
    </div>
  );
}

function FormSection({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="space-y-3">
      <div className="text-[13px] font-semibold text-muted-foreground">{title}</div>
      {children}
    </section>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div className="space-y-2">
      <div className="text-[13px] font-medium text-foreground">{label}</div>
      {children}
      {hint ? <div className="text-xs text-muted-foreground">{hint}</div> : null}
    </div>
  );
}

function ReadonlyItem({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0">
      <div className="text-xs text-muted-foreground">{label}</div>
      <div className="mt-1 truncate text-[13px] font-medium text-foreground">{value}</div>
    </div>
  );
}

export function CertificatesPage({
  locale,
  runtimeInfoLoaded,
  certificatesState,
}: {
  locale: Locale;
  runtimeInfoLoaded: boolean;
  certificatesState: CertificatesState;
}) {
  const {
    certificates,
    selectedId,
    setSelectedId,
    selectedCertificate,
    loading,
    error,
    saveCertificate,
    createManagedCertificate,
    setDefaultCertificate,
    deleteCertificate,
    validateCertificate,
  } = certificatesState;

  const [showPass, setShowPass] = useState(false);
  const [saving, setSaving] = useState(false);
  const [verifying, setVerifying] = useState(false);

  const [modalOpen, setModalOpen] = useState(false);
  const [modalMode, setModalMode] = useState<"import" | "create" | "edit">("import");

  const [importDraft, setImportDraft] = useState<CertificateUpsertInput>(emptyImportDraft());
  const [createDraft, setCreateDraft] = useState<CreateManagedCertificateInput>(emptyCreateDraft());
  const [editDraft, setEditDraft] = useState<CertificateUpsertInput>(emptyImportDraft());

  const [modalError, setModalError] = useState("");
  const [modalStatus, setModalStatus] = useState("");
  const [modalSaved, setModalSaved] = useState(false);
  const [pageError, setPageError] = useState("");
  const [importAliases, setImportAliases] = useState<string[]>([]);
  const [detectingAlias, setDetectingAlias] = useState(false);

  useEffect(() => {
    setModalSaved(false);
  }, [importDraft, createDraft, editDraft]);

  function resetModalState() {
    setModalError("");
    setModalStatus("");
    setModalSaved(false);
    setImportAliases([]);
  }

  function showModalError(message: unknown) {
    const text = String(message);
    setModalError(text);
    notifyError(text);
  }

  function openImportModal() {
    setPageError("");
    resetModalState();
    setImportDraft(emptyImportDraft());
    setModalMode("import");
    setModalOpen(true);
  }

  function openCreateModal() {
    setPageError("");
    resetModalState();
    setCreateDraft(emptyCreateDraft());
    setModalMode("create");
    setModalOpen(true);
  }

  function openEditModal() {
    if (!selectedCertificate) return;
    setPageError("");
    resetModalState();
    setEditDraft({
      id: selectedCertificate.id,
      name: selectedCertificate.name,
      source_type: selectedCertificate.source_type,
      keystore_path: selectedCertificate.keystore_path,
      keystore_password: selectedCertificate.keystore_password,
      key_alias: selectedCertificate.key_alias,
      key_password: selectedCertificate.key_password,
      ks_type: selectedCertificate.ks_type,
      sign_v1: selectedCertificate.sign_v1,
      sign_v2: selectedCertificate.sign_v2,
      sign_v3: selectedCertificate.sign_v3,
      sign_v4: selectedCertificate.sign_v4,
      auto_sign_enabled: selectedCertificate.auto_sign_enabled,
      note: selectedCertificate.note,
      set_as_default: selectedCertificate.is_default,
      copy_keystore_to_managed: false,
      managed_file_name: "",
    });
    setModalMode("edit");
    setModalOpen(true);
  }

  async function browseImportKeystore() {
    const path = await openFileDialog("Keystore", ["jks", "keystore", "p12", "pfx", "bks"]);
    if (!path) return;
    const fileName = basename(path);
    setImportDraft((old) => ({
      ...old,
      keystore_path: path,
      ks_type: path.toLowerCase().endsWith(".p12") || path.toLowerCase().endsWith(".pfx") ? "PKCS12" : "JKS",
      name: old.name.trim() ? old.name : stripKeystoreExtension(fileName),
      managed_file_name: old.managed_file_name?.trim() ? old.managed_file_name : fileName,
    }));
  }

  async function detectAliases() {
    const draft = importDraft;
    if (!draft.keystore_path || !draft.keystore_password) {
      showModalError(t(locale, "missingPassword"));
      return;
    }
    setDetectingAlias(true);
    setModalError("");
    setModalStatus("");
    try {
      const aliases = await api.listKeystoreAliases(
        draft.keystore_path,
        draft.keystore_password,
        draft.ks_type || "JKS",
      );
      setImportAliases(aliases);
      if (aliases.length === 1) {
        setImportDraft((old) => ({ ...old, key_alias: aliases[0] }));
        setModalStatus(t(locale, "validatePassed"));
      } else {
        setModalStatus(`${t(locale, "detectAlias")}：${aliases.length}`);
      }
    } catch (err) {
      showModalError(`${t(locale, "aliasFailed")}: ${String(err)}`);
    } finally {
      setDetectingAlias(false);
    }
  }

  async function validateImportDraft() {
    const draft = importDraft;
    setVerifying(true);
    setModalError("");
    setModalStatus("");
    try {
      const result = await validateCertificate({
        keystore_path: draft.keystore_path,
        keystore_password: draft.keystore_password,
        key_alias: draft.key_alias,
        ks_type: draft.ks_type,
      });
      setImportAliases(result.aliases);
      if (!result.valid) {
        showModalError(result.message || t(locale, "verificationFailed"));
        return;
      }
      if (result.resolved_alias) {
        setImportDraft((old) => ({ ...old, key_alias: result.resolved_alias || old.key_alias }));
      }
      setModalStatus(t(locale, "validatePassed"));
      notifySuccess(t(locale, "validatePassed"));
    } catch (err) {
      showModalError(err);
    } finally {
      setVerifying(false);
    }
  }

  async function saveImportDraft() {
    setSaving(true);
    setModalError("");
    try {
      const saved = await saveCertificate(importDraft);
      setModalSaved(true);
      setSelectedId(saved.id);
      setModalOpen(false);
      notifySuccess(t(locale, "saved"));
    } catch (err) {
      setModalSaved(false);
      showModalError(err);
    } finally {
      setSaving(false);
    }
  }

  async function saveEditDraft() {
    if (!editDraft.id) return;
    setSaving(true);
    setModalError("");
    try {
      const saved = await saveCertificate(editDraft);
      setModalSaved(true);
      setSelectedId(saved.id);
      setModalOpen(false);
      notifySuccess(t(locale, "saved"));
    } catch (err) {
      setModalSaved(false);
      showModalError(err);
    } finally {
      setSaving(false);
    }
  }

  async function saveCreateDraft() {
    const validationError = validateCreateDraft(createDraft, locale);
    if (validationError) {
      setModalSaved(false);
      showModalError(validationError);
      return;
    }
    setSaving(true);
    setModalError("");
    try {
      const saved = await createManagedCertificate(createDraft);
      setModalSaved(true);
      setSelectedId(saved.id);
      setModalOpen(false);
      notifySuccess(t(locale, "saved"));
    } catch (err) {
      setModalSaved(false);
      showModalError(err);
    } finally {
      setSaving(false);
    }
  }

  async function makeDefault(id: string) {
    setPageError("");
    try {
      await setDefaultCertificate(id);
      notifySuccess(t(locale, "defaultCertificateUpdated"));
    } catch (err) {
      const message = String(err);
      setPageError(message);
      notifyError(message);
    }
  }

  async function removeCertificate(id: string, removeKeystoreFile: boolean) {
    setPageError("");
    try {
      await deleteCertificate(id, removeKeystoreFile);
      notifySuccess(t(locale, "certificateDeleted"));
    } catch (err) {
      const message = String(err);
      setPageError(message);
      notifyError(message);
    }
  }

  const modalTitle =
    modalMode === "import"
      ? t(locale, "importCertificate")
      : modalMode === "create"
        ? t(locale, "createCertificate")
        : t(locale, "editConfig");

  return (
    <section className="min-h-full px-8 py-8">
      <div className="mx-auto flex max-w-[1040px] flex-col gap-5">
        <header className="flex items-center justify-between gap-4">
          <h1 className="text-[26px] font-semibold tracking-normal">{t(locale, "certificatesTitle")}</h1>
          <div className="flex gap-2">
            <AppButton variant="secondary" onClick={openImportModal}>
              <FolderOpen className="h-4 w-4" />
              {t(locale, "importCertificate")}
            </AppButton>
            <AppButton variant="secondary" onClick={openCreateModal}>
              <Plus className="h-4 w-4" />
              {t(locale, "createCertificate")}
            </AppButton>
          </div>
        </header>

        {pageError || error ? <StatusMessage kind="error">{pageError || error}</StatusMessage> : null}

        <div className="app-panel overflow-hidden">
          {loading ? (
            <div className="flex min-h-[320px] items-center justify-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="h-4 w-4 animate-spin" />
              {t(locale, "loadingCertificates")}
            </div>
          ) : certificates.length === 0 ? (
            <div className="flex min-h-[320px] items-center justify-center p-10 text-center text-muted-foreground">
              {error || t(locale, "noCertificates")}
            </div>
          ) : (
            <div className="divide-y divide-border/60">
              {certificates.map((item) => {
                const active = item.id === selectedId;
                return (
                  <div
                    key={item.id}
                    className={`px-5 py-4 transition-colors ${active ? "bg-primary/7" : ""}`}
                    onClick={() => setSelectedId(item.id)}
                  >
                    <div className="flex items-center gap-4">
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2">
                          <div className="truncate text-[15px] font-semibold">{item.name}</div>
                          {item.is_default ? (
                            <span className="inline-flex h-5 items-center rounded-full bg-primary/10 px-2 text-[11px] font-semibold text-primary">
                              默认
                            </span>
                          ) : null}
                        </div>
                        <div className="mt-1 flex flex-wrap items-center gap-x-4 gap-y-1 text-[13px] text-muted-foreground">
                          <span>Alias: {item.key_alias || "-"}</span>
                          <span>{basename(item.keystore_path)}</span>
                          <span>{item.ks_type}</span>
                          <span>{item.source_type === "managed" ? t(locale, "managedSource") : t(locale, "externalSource")}</span>
                        </div>
                      </div>

                      {active ? (
                        <div className="flex shrink-0 items-center gap-1">
                          {!item.is_default ? (
                            <button
                              type="button"
                              className="icon-button"
                              title={t(locale, "setDefaultCertificate")}
                              aria-label={t(locale, "setDefaultCertificate")}
                              onClick={(event) => {
                                event.stopPropagation();
                                void makeDefault(item.id);
                              }}
                            >
                              <Star className="h-4 w-4" />
                            </button>
                          ) : null}
                          <button
                            type="button"
                            className="icon-button"
                            title={t(locale, "editConfig")}
                            aria-label={t(locale, "editConfig")}
                            onClick={(event) => {
                              event.stopPropagation();
                              openEditModal();
                            }}
                          >
                            <FilePenLine className="h-4 w-4" />
                          </button>
                          <button
                            type="button"
                            className="icon-button"
                            title={t(locale, "deleteCertificate")}
                            aria-label={t(locale, "deleteCertificate")}
                            onClick={(event) => {
                              event.stopPropagation();
                              void removeCertificate(item.id, item.source_type === "managed");
                            }}
                          >
                            <Trash2 className="h-4 w-4" />
                          </button>
                        </div>
                      ) : (
                        <div className="flex h-9 w-9 shrink-0 items-center justify-center text-muted-foreground/50">
                          <MoreHorizontal className="h-4 w-4" />
                        </div>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>

      <CertificateModal
        open={modalOpen}
        title={modalTitle}
        onClose={() => setModalOpen(false)}
        locale={locale}
      >
        <div className="space-y-6">
          {modalMode === "import" ? (
            <>
              <FormSection title={t(locale, "importCertificate")}>
                <div className="grid gap-4 sm:grid-cols-2">
                  <Field label={t(locale, "certificateName")} hint={t(locale, "certificateNameHint")}>
                    <TextInput value={importDraft.name} onChange={(e) => setImportDraft((old) => ({ ...old, name: e.target.value }))} />
                  </Field>
                  <Field label={t(locale, "keystoreType")}>
                    <SelectInput value={importDraft.ks_type ?? "JKS"} onChange={(e) => setImportDraft((old) => ({ ...old, ks_type: e.target.value }))}>
                      <option value="JKS">JKS</option>
                      <option value="PKCS12">PKCS12</option>
                    </SelectInput>
                  </Field>
                </div>
                <Field label={t(locale, "keystore")}>
                  <div className="grid grid-cols-[minmax(0,1fr)_108px] gap-3">
                    <TextInput value={importDraft.keystore_path} onChange={(e) => setImportDraft((old) => ({ ...old, keystore_path: e.target.value }))} />
                    <AppButton variant="secondary" onClick={() => void browseImportKeystore()}>
                      {t(locale, "browse")}...
                    </AppButton>
                  </div>
                </Field>
                <Field label={t(locale, "keyAlias")}>
                  <div className="grid grid-cols-[minmax(0,1fr)_132px] gap-3">
                    <TextInput value={importDraft.key_alias} onChange={(e) => setImportDraft((old) => ({ ...old, key_alias: e.target.value }))} />
                    <AppButton variant="secondary" disabled={detectingAlias || !runtimeInfoLoaded} onClick={() => void detectAliases()}>
                      {detectingAlias ? <Loader2 className="h-4 w-4 animate-spin" /> : <Search className="h-4 w-4" />}
                      {!runtimeInfoLoaded ? t(locale, "checkingEnvironment") : t(locale, "detectAlias")}
                    </AppButton>
                  </div>
                  {importAliases.length > 1 ? (
                    <div className="mt-3">
                      <SelectInput value={importDraft.key_alias} onChange={(e) => setImportDraft((old) => ({ ...old, key_alias: e.target.value }))}>
                        <option value="">{t(locale, "keyAlias")}</option>
                        {importAliases.map((item) => (
                          <option key={item} value={item}>
                            {item}
                          </option>
                        ))}
                      </SelectInput>
                    </div>
                  ) : null}
                </Field>
                <PasswordControl id="import-kspass" label={t(locale, "keystorePassword")} value={importDraft.keystore_password} placeholder={t(locale, "keystorePasswordHint")} onChange={(value) => setImportDraft((old) => ({ ...old, keystore_password: value }))} show={showPass} setShow={setShowPass} />
                <PasswordControl id="import-keypass" label={t(locale, "keyPassword")} value={importDraft.key_password} placeholder={t(locale, "keyPasswordHint")} onChange={(value) => setImportDraft((old) => ({ ...old, key_password: value }))} show={showPass} setShow={setShowPass} />
              </FormSection>

              <FormSection title={t(locale, "certificateSettings")}>
                <label className="flex items-center justify-between rounded-xl border px-4 py-3 text-sm">
                  <span>{t(locale, "importAndManage")}</span>
                  <Switch checked={importDraft.copy_keystore_to_managed} onCheckedChange={(checked) => setImportDraft((old) => ({ ...old, source_type: checked ? "managed" : "external", copy_keystore_to_managed: checked }))} />
                </label>
                {importDraft.copy_keystore_to_managed ? (
                  <Field label={t(locale, "managedFileName")} hint={t(locale, "managedFileNameHint")}>
                    <TextInput value={importDraft.managed_file_name ?? ""} onChange={(e) => setImportDraft((old) => ({ ...old, managed_file_name: e.target.value }))} />
                  </Field>
                ) : null}
                <label className="flex items-center justify-between rounded-xl border px-4 py-3 text-sm">
                  <span>{t(locale, "setDefaultCertificate")}</span>
                  <Switch checked={importDraft.set_as_default} onCheckedChange={(checked) => setImportDraft((old) => ({ ...old, set_as_default: checked }))} />
                </label>
              </FormSection>
            </>
          ) : modalMode === "create" ? (
            <>
              <FormSection title={t(locale, "createCertificate")}>
                <div className="grid gap-4 sm:grid-cols-2">
                  <Field label={t(locale, "certificateName")} hint={t(locale, "certificateNameHint")}>
                    <TextInput value={createDraft.name} onChange={(e) => setCreateDraft((old) => ({ ...old, name: e.target.value }))} />
                  </Field>
                  <Field label={t(locale, "managedFileName")} hint={t(locale, "managedFileNameHint")}>
                    <TextInput value={createDraft.file_name} onChange={(e) => setCreateDraft((old) => ({ ...old, file_name: e.target.value }))} />
                  </Field>
                </div>
                <div className="grid gap-4 sm:grid-cols-[180px_minmax(0,1fr)]">
                  <Field label={t(locale, "keystoreType")}>
                    <SelectInput value={createDraft.ks_type ?? "PKCS12"} onChange={(e) => setCreateDraft((old) => ({ ...old, ks_type: e.target.value, file_name: e.target.value === "PKCS12" ? "release.p12" : "release.jks" }))}>
                      <option value="PKCS12">PKCS12</option>
                      <option value="JKS">JKS</option>
                    </SelectInput>
                  </Field>
                  <Field label={t(locale, "keyAlias")}>
                    <TextInput value={createDraft.key_alias} onChange={(e) => setCreateDraft((old) => ({ ...old, key_alias: e.target.value }))} />
                  </Field>
                </div>
                <Field label={t(locale, "keystorePassword")}>
                  <PasswordControl id="create-kspass" label={t(locale, "keystorePassword")} value={createDraft.keystore_password} placeholder={t(locale, "createKeystorePasswordHint")} onChange={(value) => setCreateDraft((old) => ({ ...old, keystore_password: value }))} show={showPass} setShow={setShowPass} />
                </Field>
                <Field label={t(locale, "keyPassword")}>
                  <PasswordControl id="create-keypass" label={t(locale, "keyPassword")} value={createDraft.key_password} placeholder={t(locale, "createKeyPasswordHint")} onChange={(value) => setCreateDraft((old) => ({ ...old, key_password: value }))} show={showPass} setShow={setShowPass} />
                </Field>
                <Field label={t(locale, "distinguishedName")} hint={t(locale, "distinguishedNameHint")}>
                  <TextInput value={createDraft.dname} onChange={(e) => setCreateDraft((old) => ({ ...old, dname: e.target.value }))} />
                </Field>
                <div className="grid gap-4 sm:grid-cols-2">
                  <Field label={t(locale, "validityDays")} hint={t(locale, "validityDaysHint")}>
                    <TextInput value={String(createDraft.validity_days)} onChange={(e) => setCreateDraft((old) => ({ ...old, validity_days: Number(e.target.value) || 3650 }))} />
                  </Field>
                  <Field label={t(locale, "keySize")} hint={t(locale, "keySizeHint")}>
                    <TextInput value={String(createDraft.key_size)} onChange={(e) => setCreateDraft((old) => ({ ...old, key_size: Number(e.target.value) || 2048 }))} />
                  </Field>
                </div>
              </FormSection>

              <FormSection title={t(locale, "certificateSettings")}>
                <label className="flex items-center justify-between rounded-xl border px-4 py-3 text-sm">
                  <span>{t(locale, "setDefaultCertificate")}</span>
                  <Switch checked={createDraft.set_as_default} onCheckedChange={(checked) => setCreateDraft((old) => ({ ...old, set_as_default: checked }))} />
                </label>
              </FormSection>
            </>
          ) : (
            <>
              <FormSection title={t(locale, "editConfig")}>
                <Field label={t(locale, "certificateName")}>
                  <TextInput value={editDraft.name} onChange={(e) => setEditDraft((old) => ({ ...old, name: e.target.value }))} />
                </Field>
                <div className="rounded-xl border border-border/70 bg-muted/25 p-4">
                  <div className="mb-3 text-[13px] font-semibold text-muted-foreground">{t(locale, "certificateMaterial")}</div>
                  <div className="grid gap-3 text-sm sm:grid-cols-2">
                    <ReadonlyItem label={t(locale, "keystore")} value={basename(editDraft.keystore_path || "") || "-"} />
                    <ReadonlyItem label={t(locale, "keyAlias")} value={editDraft.key_alias || "-"} />
                    <ReadonlyItem label={t(locale, "keystoreType")} value={editDraft.ks_type || "-"} />
                    <ReadonlyItem
                      label={t(locale, "certificateStatus")}
                      value={editDraft.source_type === "managed" ? t(locale, "managedSource") : t(locale, "externalSource")}
                    />
                  </div>
                  <div className="mt-3 text-xs leading-5 text-muted-foreground">
                    {t(locale, "certificateMaterialLocked")}
                  </div>
                </div>
              </FormSection>

              <FormSection title={t(locale, "certificateSettings")}>
                <div className="grid gap-4 sm:grid-cols-2">
                  <Field label={t(locale, "signVersions")}>
                    <div className="grid grid-cols-2 gap-2">
                      {(["v1", "v2", "v3", "v4"] as const).map((version) => {
                        const key = `sign_${version}` as keyof CertificateUpsertInput;
                        return (
                          <label key={version} className="flex min-h-10 items-center gap-2 rounded-xl border border-border/70 bg-muted/30 px-3.5 text-sm font-semibold">
                            <input
                              type="checkbox"
                              className="h-5 w-5 accent-primary"
                              checked={Boolean(editDraft[key])}
                              onChange={(e) => setEditDraft((old) => ({ ...old, [key]: e.target.checked }))}
                            />
                            {version.toUpperCase()}
                          </label>
                        );
                      })}
                    </div>
                  </Field>
                  <Field label={t(locale, "certificateNote")}>
                    <TextInput value={editDraft.note} onChange={(e) => setEditDraft((old) => ({ ...old, note: e.target.value }))} />
                  </Field>
                </div>
                <label className="flex items-center justify-between rounded-xl border px-4 py-3 text-sm">
                  <span>{t(locale, "autoSignAfterProtect")}</span>
                  <Switch checked={editDraft.auto_sign_enabled} onCheckedChange={(checked) => setEditDraft((old) => ({ ...old, auto_sign_enabled: checked }))} />
                </label>
              </FormSection>
            </>
          )}

          {modalError ? <StatusMessage kind="error">{modalError}</StatusMessage> : null}

          <div className="flex items-center justify-between gap-3 border-t pt-4">
            <div>
              {modalMode === "import" ? (
                <AppButton
                  variant="secondary"
                  className="min-w-[136px]"
                  disabled={verifying || !runtimeInfoLoaded}
                  onClick={() => void validateImportDraft()}
                >
                  {verifying ? <Loader2 className="h-4 w-4 animate-spin" /> : modalStatus ? <Check className="h-4 w-4" /> : <ShieldCheck className="h-4 w-4" />}
                  {!runtimeInfoLoaded ? t(locale, "checkingEnvironment") : modalStatus ? t(locale, "validatePassed") : t(locale, "validate")}
                </AppButton>
              ) : null}
            </div>

            <div className="flex items-center gap-3">
              <AppButton variant="secondary" className="min-w-[96px]" onClick={() => setModalOpen(false)}>
                {t(locale, "cancel")}
              </AppButton>

              {modalMode === "import" ? (
                <AppButton className="min-w-[136px]" disabled={saving} onClick={() => void saveImportDraft()}>
                  {saving ? <Loader2 className="h-4 w-4 animate-spin" /> : <Check className="h-4 w-4" />}
                  {modalSaved ? t(locale, "saved") : t(locale, "save")}
                </AppButton>
              ) : modalMode === "create" ? (
                <AppButton className="min-w-[136px]" disabled={saving || !runtimeInfoLoaded} onClick={() => void saveCreateDraft()}>
                  {saving ? <Loader2 className="h-4 w-4 animate-spin" /> : <Check className="h-4 w-4" />}
                  {!runtimeInfoLoaded ? t(locale, "checkingEnvironment") : modalSaved ? t(locale, "saved") : t(locale, "createCertificate")}
                </AppButton>
              ) : (
                <AppButton className="min-w-[136px]" disabled={saving} onClick={() => void saveEditDraft()}>
                  {saving ? <Loader2 className="h-4 w-4 animate-spin" /> : <Check className="h-4 w-4" />}
                  {modalSaved ? t(locale, "saved") : t(locale, "save")}
                </AppButton>
              )}
            </div>
          </div>
        </div>
      </CertificateModal>
    </section>
  );
}
