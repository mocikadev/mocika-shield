/** 只管理本地检查、选择与冻结；没有构造或发送上报正文的能力。 */
export type SharingInspection = { inspection_id: string; package_name: string; enabled: boolean };
export type SharingChoice = { inspectionId: string; enabled: boolean };
type SharingApi = { inspect: (path: string) => Promise<SharingInspection | null>; release: (id: string) => Promise<void>; save: (id: string, enabled: boolean) => Promise<void> };
type Preference = { enabled: boolean; pending: number; revision: number };
export type SharingView = { path: string; inspection: SharingInspection | null; enabled: boolean; pending: boolean; loading: boolean; frozen: boolean };
const empty = (): SharingView => ({ path: "", inspection: null, enabled: false, pending: false, loading: false, frozen: false });

export class SharingHub {
  private revision = 0;
  private preferences = new Map<string, Preference>();
  private listeners = new Set<() => void>();
  private saves = new Map<string, Promise<void>>();
  subscribe(listener: () => void) { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; }
  private emit() { this.listeners.forEach(listener => listener()); }
  get(packageName: string) { return this.preferences.get(packageName); }
  getRevision() { return this.revision; }
  seed(inspection: SharingInspection, startedRevision: number) {
    const current = this.preferences.get(inspection.package_name);
    if (!current || (!current.pending && current.revision <= startedRevision)) {
      this.preferences.set(inspection.package_name, { enabled: inspection.enabled, pending: 0, revision: ++this.revision });
      this.emit();
    }
  }
  receive(packageName: string, enabled: boolean) {
    const current = this.preferences.get(packageName);
    if (current?.pending) return;
    this.preferences.set(packageName, { enabled, pending: 0, revision: ++this.revision });
    this.emit();
  }
  async change(inspection: SharingInspection, enabled: boolean, save: SharingApi["save"]) {
    const name = inspection.package_name;
    const current = this.preferences.get(name) ?? { enabled: false, pending: 0, revision: 0 };
    const revision = ++this.revision;
    this.preferences.set(name, { enabled, pending: current.pending + 1, revision });
    this.emit();
    const operation = (this.saves.get(name) ?? Promise.resolve()).catch(() => undefined).then(() => save(inspection.inspection_id, enabled));
    this.saves.set(name, operation);
    try { await operation; }
    catch (error) {
      const latest = this.preferences.get(name)!;
      if (latest.revision === revision) latest.enabled = false;
      throw error;
    } finally {
      const latest = this.preferences.get(name)!;
      latest.pending -= 1;
      if (latest.revision === revision) latest.revision = ++this.revision;
      if (this.saves.get(name) === operation) this.saves.delete(name);
      this.emit();
    }
  }
}

export class SharingController {
  private view = empty();
  private revision = 0;
  private listeners = new Set<() => void>();
  private hub: SharingHub;
  private activeSaves = new Map<string, Promise<void>>();
  private api: SharingApi;
  private unsubscribe?: () => void;
  constructor(hub: SharingHub, api: SharingApi) {
    this.hub = hub; this.api = api;
  }
  getSnapshot = () => this.view;
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; };
  private update(patch: Partial<SharingView>) { this.view = { ...this.view, ...patch }; this.listeners.forEach(listener => listener()); }
  private release(id?: string) {
    if (!id) return;
    const pending = this.activeSaves.get(id);
    if (pending) void pending.catch(() => undefined).then(() => this.api.release(id)).catch(() => undefined);
    else void this.api.release(id).catch(() => undefined);
  }
  private sync() {
    if (!this.view.inspection || this.view.frozen) return;
    const preference = this.hub.get(this.view.inspection.package_name);
    if (preference) this.update({ enabled: preference.enabled, pending: preference.pending > 0 });
  }
  async select(path: string) {
    this.unsubscribe ??= this.hub.subscribe(() => this.sync());
    const revision = ++this.revision;
    const preferenceRevision = this.hub.getRevision();
    this.release(this.view.inspection?.inspection_id);
    this.update({ ...empty(), path, loading: Boolean(path) });
    if (!path) return;
    let inspection: SharingInspection | null = null;
    try { inspection = await this.api.inspect(path); } catch { /* 分享检查失败不影响原任务。 */ }
    if (revision !== this.revision || this.view.frozen) { this.release(inspection?.inspection_id); return; }
    if (inspection) this.hub.seed(inspection, preferenceRevision);
    this.update({ inspection, loading: false }); this.sync();
  }
  async refresh() { if (!this.view.frozen && !this.view.pending && !this.view.loading) await this.select(this.view.path); }
  async change(enabled: boolean) {
    if (!this.view.inspection || this.view.frozen || this.view.pending || this.view.loading) return;
    const id = this.view.inspection.inspection_id;
    const operation = this.hub.change(this.view.inspection, enabled, this.api.save);
    this.activeSaves.set(id, operation);
    try { await operation; } finally { this.activeSaves.delete(id); }
  }
  freeze(): SharingChoice | null | undefined {
    if (this.view.pending) return undefined;
    this.update({ frozen: true });
    return this.view.inspection ? { inspectionId: this.view.inspection.inspection_id, enabled: this.view.enabled } : null;
  }
  dispose() { ++this.revision; this.release(this.view.inspection?.inspection_id); this.unsubscribe?.(); this.unsubscribe = undefined; }
}
