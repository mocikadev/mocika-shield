import { toast } from "sonner";

function normalizeMessage(message: unknown) {
  const text = String(message ?? "").trim();
  return text || "操作失败";
}

export function notifyError(message: unknown) {
  toast.error(normalizeMessage(message));
}

export function notifySuccess(message: unknown) {
  toast.success(normalizeMessage(message));
}

export function notifyWarning(message: unknown) {
  toast.warning(normalizeMessage(message));
}
