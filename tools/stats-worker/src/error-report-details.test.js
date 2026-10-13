import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { normalizeErrorReport } from "./error-reports.js";

const reportV2 = JSON.parse(readFileSync(new URL("../../../tests/fixtures/diagnostics/error-report-v2.json", import.meta.url), "utf8"));

test("第二版共享样本完整保留摘要与路径布尔特征", () => {
  const normalized = normalizeErrorReport(reportV2);
  assert.deepEqual(normalized.report, reportV2);
  assert.deepEqual(JSON.parse(normalized.canonicalJson), reportV2);
});

test("第二版拒绝任意文本和疑似敏感内容而不是替用户猜测脱敏", () => {
  for (const summary of ["/Users/private/a.apk", "C:\\Users\\秘密.jks", "pass:PRIVATE", "CN=秘密 alias=签名", "com.private.app", "https://host/?token=PRIVATE"]) {
    assert.throws(() => normalizeErrorReport({ ...reportV2, evidence: [{ kind: "resource_not_found", summary }] }));
  }
  for (const change of [
    { raw_log: "PRIVATE" },
    { java_version: "17.0.1 PRIVATE" },
    { os_version: "user@host" },
    { input_path: { ...reportV2.details.input_path, path: "PRIVATE" } },
    { input_path: { ...reportV2.details.input_path, non_ascii: "true" } },
    { execution_step: "certificate" },
    { evidence_status: "unavailable" },
  ]) {
    assert.throws(() => normalizeErrorReport({ ...reportV2, details: { ...reportV2.details, ...change } }));
  }
});

test("第二版拒绝版本错配重复摘要超量证据和摘要附加字段", () => {
  for (const patch of [
    { sanitizer_version: 1 }, { schema_version: 3 },
    { evidence: [reportV2.evidence[0], reportV2.evidence[0]] },
    { evidence: Array(9).fill(reportV2.evidence[0]) },
    { evidence: [{ ...reportV2.evidence[0], path: "PRIVATE" }] },
  ]) assert.throws(() => normalizeErrorReport({ ...reportV2, ...patch }));
});

test("第一版仍然不接受第二版详情或非空证据", () => {
  assert.throws(() => normalizeErrorReport({ ...reportV2, schema_version: 1, sanitizer_version: 1 }));
  const { details: _details, ...legacy } = reportV2;
  assert.throws(() => normalizeErrorReport({ ...legacy, schema_version: 1, sanitizer_version: 1 }));
});
