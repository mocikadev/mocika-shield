import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { normalizeErrorReport } from "./error-reports.js";

const sharedFixture = JSON.parse(readFileSync(
  new URL("../../../tests/fixtures/diagnostics/error-report-v1.json", import.meta.url),
  "utf8",
));

const validReport = {
  report_id: "123e4567-e89b-42d3-a456-426614174000",
  schema_version: 1,
  classifier_version: 1,
  sanitizer_version: 1,
  app_version: "1.4.0-beta.5",
  build_revision: "abcdef0",
  build_kind: "release",
  occurred_at: 1789516800,
  flow: "protect_with_sign",
  operation: "sign",
  stage: "execute",
  code: "SIGNING_FAILED",
  platform: "macos",
  arch: "aarch64",
  java_major: 21,
  java_vendor: "adoptium",
  tool_name: "apksigner",
  tool_version: "35.0.1",
  exit_code: 1,
  evidence: [],
};

test("错误报告严格协议产生稳定规范载荷", () => {
  const result = normalizeErrorReport(validReport);
  assert.equal(result.report.report_id, validReport.report_id);
  assert.equal(JSON.parse(result.canonicalJson).tool_version, "35.0.1");
  assert.deepEqual(Object.keys(JSON.parse(result.canonicalJson)), Object.keys(validReport));
});

test("共享错误报告fixture与服务端协议逐字段一致", () => {
  const result = normalizeErrorReport(sharedFixture);
  assert.deepEqual(result.report, sharedFixture);
  assert.deepEqual(JSON.parse(result.canonicalJson), sharedFixture);
});

test("错误报告拒绝额外字段、非法空值和非空证据", () => {
  assert.throws(() => normalizeErrorReport({ ...validReport, raw_error: "密码" }), /字段/);
  assert.throws(() => normalizeErrorReport({ ...validReport, build_revision: "ABCDEF0" }), /build_revision/);
  assert.throws(() => normalizeErrorReport({ ...validReport, evidence: ["text"] }), /evidence/);
  assert.throws(() => normalizeErrorReport({ ...validReport, java_major: 7 }), /java_major/);
});

test("错误报告拒绝超过8KiB的原始请求字节", () => {
  assert.throws(() => normalizeErrorReport(validReport, 8193), /8192/);
});

test("应用版本只接受完整正式或受限预发布语义版本", () => {
  assert.equal(normalizeErrorReport({ ...validReport, app_version: "1.4.0" }).report.app_version, "1.4.0");
  assert.equal(normalizeErrorReport({ ...validReport, app_version: "1.4.0-beta.5" }).report.app_version, "1.4.0-beta.5");
  assert.equal(normalizeErrorReport({ ...validReport, app_version: "1.4.0-preview-x.7" }).report.app_version, "1.4.0-preview-x.7");
});

test("应用版本拒绝路径、自由文本、特殊字符和负发生时间", () => {
  for (const appVersion of [
    "/Users/alice/secret", "C:\\Users\\alice\\secret", "内部测试 密码", "user@example.com",
    "1.4", "01.4.0", "1.4.0-beta..5", "1.4.0+private",
  ]) {
    assert.throws(() => normalizeErrorReport({ ...validReport, app_version: appVersion }), /app_version/);
  }
  assert.throws(() => normalizeErrorReport({ ...validReport, occurred_at: -1 }), /occurred_at/);
});

export { validReport };
