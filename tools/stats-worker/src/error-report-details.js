// 第二版只接受有限摘要和结构化特征，不接收“脱敏后”的任意文本。
const TEMPLATES = new Map([
  ["encoding_invalid", "工具输出不是有效 UTF-8，未保留原文"],
  ["resource_not_found", "error: resource <资源标识已移除> not found"],
  ["attribute_not_found", "error: attribute <属性标识已移除> not found"],
  ["duplicate_resource", "duplicate resource <资源标识已移除>"],
  ["resource_compile_failed", "资源编译失败（文件路径与资源标识已移除）"],
  ["resource_link_failed", "资源链接失败（文件路径与资源标识已移除）"],
  ["xml_parse_failed", "XML 解析失败（文件路径与 XML 内容已移除）"],
  ["invalid_png", "PNG 格式或签名无效（文件名已移除）"],
  ["java_class_version", "Java 字节码版本不受当前 JVM 支持"],
  ["java_out_of_memory", "java.lang.OutOfMemoryError（异常详情已移除）"],
  ["invalid_zip", "java.util.zip.ZipException（异常详情已移除）"],
  ["access_denied", "文件访问被拒绝（路径已移除）"],
  ["disk_full", "磁盘空间不足（路径已移除）"],
  ["file_not_found", "文件不存在或无法打开（路径已移除）"],
  ["keystore_password", "证书口令或私钥解锁失败（口令与证书信息已移除）"],
  ["key_alias_missing", "证书中未找到指定别名（别名已移除）"],
]);
const DETAILS_FIELDS = ["execution_step", "os_version", "java_version", "input_path", "output_path", "evidence_status"];
const PATH_FIELDS = ["non_ascii", "spaces", "special_characters", "long_path"];
const STEPS = {
  protect: new Set(["prepare", "unpack", "modify_manifest", "process_dex", "inject_runtime", "repack", "align", "unknown"]),
  sign: new Set(["prepare", "align", "sign", "cleanup", "unknown"]),
  certificate: new Set(["certificate", "unknown"]),
};

function exactFields(value, fields) {
  if (!value || typeof value !== "object" || Array.isArray(value)
    || Object.keys(value).length !== fields.length || Object.keys(value).some((key) => !fields.includes(key))) {
    throw new Error("报告详情字段无效");
  }
}
function version(value) {
  if (value !== null && (typeof value !== "string" || value.length > 32 || !/^\d+(?:\.\d+)*$/.test(value))) {
    throw new Error("报告详情版本无效");
  }
  return value;
}
function path(value) {
  if (value === null) return null;
  exactFields(value, PATH_FIELDS);
  if (PATH_FIELDS.some((key) => typeof value[key] !== "boolean")) throw new Error("路径特征无效");
  return Object.fromEntries(PATH_FIELDS.map((key) => [key, value[key]]));
}

export function normalizeDetails(report) {
  exactFields(report.details, DETAILS_FIELDS);
  const value = report.details;
  if (!STEPS[report.operation]?.has(value.execution_step)) throw new Error("准确步骤无效");
  if (!Array.isArray(report.evidence) || report.evidence.length > 8) throw new Error("evidence无效");
  const kinds = new Set();
  const evidence = report.evidence.map((item) => {
    exactFields(item, ["kind", "summary"]);
    if (!TEMPLATES.has(item.kind) || TEMPLATES.get(item.kind) !== item.summary || kinds.has(item.kind)) {
      throw new Error("evidence模板无效");
    }
    kinds.add(item.kind);
    return { kind: item.kind, summary: item.summary };
  });
  const status = evidence.length ? "recognized" : "unavailable";
  if (value.evidence_status !== status) throw new Error("证据状态无效");
  return {
    evidence,
    details: {
      execution_step: value.execution_step,
      os_version: version(value.os_version),
      java_version: version(value.java_version),
      input_path: path(value.input_path),
      output_path: path(value.output_path),
      evidence_status: status,
    },
  };
}
