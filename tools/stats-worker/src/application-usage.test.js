import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { normalizeApplicationUsage } from "./application-usage.js";

const fixture = JSON.parse(readFileSync(new URL("../../../tests/fixtures/diagnostics/application-usage-v1.json", import.meta.url), "utf8"));
const now = new Date("2026-09-18T12:00:00.000Z");

test("十字段共享fixture产生稳定规范载荷", () => {
  const normalized = normalizeApplicationUsage(fixture, undefined, now);
  assert.deepEqual(normalized.submission, fixture);
  assert.deepEqual(JSON.parse(normalized.canonicalJson), fixture);
  assert.deepEqual(Object.keys(JSON.parse(normalized.canonicalJson)), Object.keys(fixture));
});

test("协议拒绝缺字段、未知字段和不允许的空值", () => {
  const { app_name: _removed, ...missing } = fixture;
  assert.throws(() => normalizeApplicationUsage(missing, undefined, now), /字段/);
  assert.throws(() => normalizeApplicationUsage({ ...fixture, path: "/secret.apk" }, undefined, now), /字段/);
  assert.throws(() => normalizeApplicationUsage({ ...fixture, package_name: null }, undefined, now), /package_name/);
  assert.equal(normalizeApplicationUsage({ ...fixture, app_name: null, app_version_code: null }, undefined, now).submission.app_name, null);
});

test("包名只接受255字节内的ASCII点分标识", () => {
  for (const packageName of ["com..app", "9com.app", "com.-app", "com.应用", "com.app.", " com.app"])
    assert.throws(() => normalizeApplicationUsage({ ...fixture, package_name: packageName }, undefined, now), /package_name/);
  const valid = `${"a".repeat(126)}.${"b".repeat(128)}`;
  assert.equal(normalizeApplicationUsage({ ...fixture, package_name: valid }, undefined, now).submission.package_name, valid);
  assert.equal(normalizeApplicationUsage({ ...fixture, package_name: "android" }, undefined, now).submission.package_name, "android");
  assert.throws(() => normalizeApplicationUsage({ ...fixture, package_name: `${valid}c` }, undefined, now), /package_name/);
});

test("名称同时限制Unicode字符、UTF8字节、控制字符和非法代理项", () => {
  assert.equal(normalizeApplicationUsage({ ...fixture, app_name: "😀".repeat(256) }, undefined, now).submission.app_name.length, 512);
  for (const appName of ["a".repeat(257), "😀".repeat(257), "a\nname", "bad\uD800name", "bad\uD800"])
    assert.throws(() => normalizeApplicationUsage({ ...fixture, app_name: appName }, undefined, now), /app_name/);
});

test("版本码、工具版本、UUID和协议版本严格校验", () => {
  for (const code of ["", "01", "+1", "-1", "9223372036854775808", 1])
    assert.throws(() => normalizeApplicationUsage({ ...fixture, app_version_code: code }, undefined, now), /app_version_code/);
  assert.equal(normalizeApplicationUsage({ ...fixture, tool_version: "1.4.0+build.1" }, undefined, now).submission.tool_version, "1.4.0+build.1");
  assert.equal(normalizeApplicationUsage({ ...fixture, tool_version: "1.4.0-beta.6+build.1.sha" }, undefined, now).submission.tool_version, "1.4.0-beta.6+build.1.sha");
  for (const version of ["1.4", "01.4.0", "1.4.0+build..1", "1.4.0-beta..6", "1.4.0+"])
    assert.throws(() => normalizeApplicationUsage({ ...fixture, tool_version: version }, undefined, now), /tool_version/);
  assert.throws(() => normalizeApplicationUsage({ ...fixture, submission_id: "not-uuid" }, undefined, now), /submission_id/);
  assert.throws(() => normalizeApplicationUsage({ ...fixture, schema_version: 2 }, undefined, now), /协议版本/);
  assert.throws(() => normalizeApplicationUsage({ ...fixture, notice_version: 2 }, undefined, now), /协议版本/);
});

test("成功日期只接受有效UTC日的过去七天至次日", () => {
  for (const date of ["2026-09-11", "2026-09-19"])
    assert.equal(normalizeApplicationUsage({ ...fixture, success_date: date }, undefined, now).submission.success_date, date);
  for (const date of ["2026-09-10", "2026-09-20", "2026-02-30", "2026-9-18"])
    assert.throws(() => normalizeApplicationUsage({ ...fixture, success_date: date }, undefined, now), /success_date/);
});

test("只接受三种操作与流程组合并拒绝超过8KiB", () => {
  for (const [operation, flow] of [["protect", "protect"], ["protect", "protect_with_sign"], ["sign", "sign"]])
    assert.equal(normalizeApplicationUsage({ ...fixture, operation, flow }, undefined, now).submission.flow, flow);
  for (const [operation, flow] of [["sign", "protect"], ["sign", "protect_with_sign"], ["protect", "sign"]])
    assert.throws(() => normalizeApplicationUsage({ ...fixture, operation, flow }, undefined, now), /组合/);
  assert.throws(() => normalizeApplicationUsage({ ...fixture, operation: ["protect"], flow: ["protect"] }, undefined, now), /组合/);
  assert.throws(() => normalizeApplicationUsage(fixture, 8193, now), /8192/);
});
