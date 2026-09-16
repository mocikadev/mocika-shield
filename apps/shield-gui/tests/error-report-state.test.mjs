import assert from "node:assert/strict";
import test from "node:test";
import { initialReportState, reportReducer } from "../src/lib/error-report-state.ts";
import { readFileSync } from "node:fs";
import { URL } from "node:url";

const preview = (id, prompt = true) => ({ report_id: id, digest: "digest", payload_json: "{}", should_prompt: prompt, sent: false });

test("关闭或拒绝不会产生发送状态", () => {
  const ready = reportReducer(initialReportState, { type: "receive", preview: preview("一") });
  assert.equal(ready.open, true);
  const closed = reportReducer(ready, { type: "close" });
  assert.equal(closed.open, false);
  assert.equal(closed.sending, false);
});

test("正在确认的预览不会被新错误替换", () => {
  const ready = reportReducer(initialReportState, { type: "receive", preview: preview("一") });
  const sending = reportReducer(ready, { type: "send" });
  const next = reportReducer(sending, { type: "receive", preview: preview("二") });
  assert.equal(next.preview.report_id, "一");
  assert.equal(next.latest.report_id, "二");
  assert.equal(next.sending, true);
});

test("不主动提示的错误保留手动入口且回执不属于新任务", () => {
  const state = reportReducer(initialReportState, { type: "receive", preview: preview("一", false) });
  assert.equal(state.open, false);
  assert.equal(state.latest.report_id, "一");
  const open = reportReducer(state, { type: "open", preview: preview("一", false) });
  const success = reportReducer(reportReducer(open, { type: "send" }), { type: "success", id: "一" });
  assert.equal(success.receipt, "一");
  assert.equal(success.sending, false);
});

test("错误报告使用非模态面板且不遮挡原错误", () => {
  const source = readFileSync(new URL("../src/components/app/error-report-dialog.tsx", import.meta.url), "utf8");
  assert.doesNotMatch(source, /Dialog\.Overlay/);
  assert.doesNotMatch(source, /fixed inset-0/);
  assert.match(source, /role="complementary"/);
});
