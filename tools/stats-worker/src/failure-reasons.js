export const FAILURE_CODES = new Set([
  "TOOL_NOT_FOUND", "JAVA_UNSUPPORTED", "FILE_NOT_FOUND", "PERMISSION_DENIED",
  "DISK_FULL", "UNSUPPORTED_ABI", "KEYSTORE_PASSWORD_INVALID", "KEY_ALIAS_NOT_FOUND",
  "KEYSTORE_FORMAT_INVALID", "TOOL_OUTPUT_ENCODING_INVALID", "MANIFEST_REBUILD_FAILED",
  "APK_REPACK_FAILED", "RUNTIME_INJECTION_FAILED", "ALIGNMENT_FAILED", "SIGNING_FAILED",
  "TOOL_PROCESS_FAILED", "UNKNOWN",
]);

const ALLOWED_FIELDS = new Set([
  "flow", "operation", "stage", "code", "classifier_version", "count",
]);

const COMBINATIONS = new Map([
  ["protect:protect", new Set(["prepare", "unpack", "manifest", "dex_runtime", "align", "unknown"])],
  ["protect_with_sign:protect", new Set(["prepare", "unpack", "manifest", "dex_runtime", "align", "unknown"])],
  ["protect_with_sign:sign", new Set(["prepare", "align", "execute", "sign", "unknown"])],
  ["sign:sign", new Set(["prepare", "align", "execute", "sign", "unknown"])],
  ["certificate:certificate", new Set(["prepare", "execute", "unknown"])],
]);

export function validFailureDimensions(flow, operation, stage) {
  return typeof flow === "string"
    && typeof operation === "string"
    && typeof stage === "string"
    && Boolean(COMBINATIONS.get(`${flow}:${operation}`)?.has(stage));
}

export function normalizeFailureReasonCounts(value) {
  if (value === undefined) return undefined;
  if (!Array.isArray(value)) throw new Error("失败原因必须为数组");
  if (value.length > 128) throw new Error("失败原因最多128条");

  const grouped = new Map();
  for (const item of value) {
    if (!item || typeof item !== "object" || Array.isArray(item)
      || Object.keys(item).some((key) => !ALLOWED_FIELDS.has(key))
      || Object.keys(item).length !== ALLOWED_FIELDS.size) {
      throw new Error("失败原因字段无效");
    }
    if (!validFailureDimensions(item.flow, item.operation, item.stage)) {
      throw new Error("失败原因维度组合无效");
    }
    if (!FAILURE_CODES.has(item.code)) throw new Error("失败原因错误码无效");
    if (item.classifier_version !== 1) throw new Error("失败原因分类版本无效");
    if (!Number.isInteger(item.count) || item.count < 0 || item.count > 10000) {
      throw new Error("失败原因计数无效");
    }
    const key = [item.flow, item.operation, item.stage, item.code, item.classifier_version].join(":");
    const previous = grouped.get(key);
    if (!previous || item.count > previous.count) grouped.set(key, { ...item });
  }
  return [...grouped.values()];
}
