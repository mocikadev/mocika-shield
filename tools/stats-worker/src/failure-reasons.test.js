import assert from "node:assert/strict";
import test from "node:test";

import { normalizeFailureReasonCounts } from "./failure-reasons.js";

test("失败原因快照接受固定维度并以同键最大值去重", () => {
  assert.deepEqual(normalizeFailureReasonCounts([
    { flow: "protect", operation: "protect", stage: "manifest", code: "MANIFEST_REBUILD_FAILED", classifier_version: 1, count: 2 },
    { flow: "protect", operation: "protect", stage: "manifest", code: "MANIFEST_REBUILD_FAILED", classifier_version: 1, count: 1 },
  ]), [
    { flow: "protect", operation: "protect", stage: "manifest", code: "MANIFEST_REBUILD_FAILED", classifier_version: 1, count: 2 },
  ]);
});

test("失败原因快照拒绝非法组合、字段、计数和超量项目", () => {
  assert.throws(() => normalizeFailureReasonCounts([
    { flow: "sign", operation: "protect", stage: "manifest", code: "UNKNOWN", classifier_version: 1, count: 1 },
  ]), /维度组合/);
  assert.throws(() => normalizeFailureReasonCounts([
    { flow: "sign", operation: "sign", stage: "execute", code: "NOT_A_CODE", classifier_version: 1, count: 1 },
  ]), /错误码/);
  assert.throws(() => normalizeFailureReasonCounts([
    { flow: "sign", operation: "sign", stage: "execute", code: "SIGNING_FAILED", classifier_version: 1, count: 10001 },
  ]), /计数/);
  assert.throws(() => normalizeFailureReasonCounts(Array.from({ length: 129 }, (_, count) => ({
    flow: "sign", operation: "sign", stage: "execute", code: "SIGNING_FAILED", classifier_version: 1, count,
  }))), /最多/);
  assert.throws(() => normalizeFailureReasonCounts([
    { flow: "sign", operation: "sign", stage: "execute", code: "SIGNING_FAILED", classifier_version: 1, count: 1, raw_error: "secret" },
  ]), /字段/);
});
