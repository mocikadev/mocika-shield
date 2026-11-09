import assert from "node:assert/strict";
import test from "node:test";
import { setImmediate } from "node:timers";
import { PreferenceSync } from "../src/lib/preference-sync.ts";

const deferred = () => { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };

test("保存开始立即作废旧刷新且保存期间不接受新刷新", async () => {
  const sync = new PreferenceSync(); const token = sync.beginRead(); const wait = deferred();
  const saving = sync.write(() => wait.promise);
  assert.equal(sync.canApply(token), false); assert.equal(sync.beginRead(), undefined);
  wait.resolve(); await saving;
  assert.equal(sync.canApply(token), false); assert.equal(sync.busy, false);
  assert.equal(sync.canApply(sync.beginRead()), true);
});

test("同一窗口的连续修改按提交顺序保存而不并发反转", async () => {
  const sync = new PreferenceSync(); const first = deferred(); const order = [];
  const a = sync.write(async () => { order.push("第一次"); await first.promise; });
  const b = sync.write(async () => { order.push("第二次"); });
  await new Promise(setImmediate); assert.deepEqual(order, ["第一次"]);
  first.resolve(); await Promise.all([a, b]);
  assert.deepEqual(order, ["第一次", "第二次"]); assert.equal(sync.busy, false);
});

test("保存失败仍返回原错误但不会阻塞后面的独立修改", async () => {
  const sync = new PreferenceSync(); let changed = false;
  const first = sync.write(async () => { throw new Error("写入失败"); });
  const second = sync.write(async () => { changed = true; });
  await assert.rejects(first, /写入失败/); await second;
  assert.equal(changed, true); assert.equal(sync.busy, false);
});

test("晚到的旧查询和卸载前查询均不能覆盖新状态", () => {
  const sync = new PreferenceSync(); const old = sync.beginRead(); const current = sync.beginRead();
  assert.equal(sync.canApply(old), false); assert.equal(sync.canApply(current), true);
  sync.invalidate(); assert.equal(sync.canApply(current), false);
});
