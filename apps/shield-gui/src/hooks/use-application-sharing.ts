import { useEffect, useMemo, useSyncExternalStore } from "react";
import { SharingController, SharingHub } from "@/lib/application-sharing-state";
import { api, onTauriEvent } from "@/lib/tauri";
import { notifyError } from "@/lib/notify";

const hub = new SharingHub();
const sharingApi = { inspect: api.inspectApplicationSharing, release: api.releaseApplicationInspection, save: api.saveApplicationSharing };

export function useApplicationSharing(path: string) {
  const controller = useMemo(() => new SharingController(hub, sharingApi), []);
  const view = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
  useEffect(() => {
    void controller.select(path);
  }, [controller, path]);
  useEffect(() => {
    const timer = window.setInterval(() => { void controller.refresh(); }, 240_000);
    const unlisten = onTauriEvent<{ package_name: string; enabled: boolean }>("application-sharing-changed", event => hub.receive(event.package_name, event.enabled));
    return () => { window.clearInterval(timer); controller.dispose(); void unlisten.then(fn => fn()); };
  }, [controller]);
  return {
    ...view,
    disabled: !view.inspection || view.loading || view.pending || view.frozen,
    change: (enabled: boolean) => { void controller.change(enabled).catch(error => notifyError(String(error))); },
    freeze: () => controller.freeze(),
    isPending: () => controller.getSnapshot().pending,
  };
}
