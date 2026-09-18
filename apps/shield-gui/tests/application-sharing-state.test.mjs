import assert from "node:assert/strict";
import test from "node:test";
import { SharingHub, SharingController } from "../src/lib/application-sharing-state.ts";

const preview = (id, name, enabled = true) => ({ inspection_id: id, package_name: name, enabled });
const deferred = () => { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };
function setup() {
  const preferences = new Map(); const packages = new Map(); const released = []; let calls = 0;
  const api = { inspect: async path => { const id = `${++calls}`; packages.set(id, path); return preview(id, path, preferences.get(path) ?? true); }, release: async id => { released.push(id); }, save: async (id, enabled) => { preferences.set(packages.get(id), enabled); } };
  const hub = new SharingHub();
  return { hub, api, released, create: () => new SharingController(hub, api) };
}

test("A取消B默认开启再选A仍关闭并双向跨页同步", async () => {
  const { create } = setup(); const protect = create(), sign = create();
  await protect.select("A"); await sign.select("A");
  await protect.change(false); assert.equal(sign.getSnapshot().enabled, false);
  await protect.select("B"); assert.equal(protect.getSnapshot().enabled, true);
  await protect.select("A"); assert.equal(protect.getSnapshot().enabled, false);
  await sign.change(true); assert.equal(protect.getSnapshot().enabled, true);
});

test("快速切包旧检查不能覆盖新输入且旧引用释放", async () => {
  const { hub, api, released } = setup(); const old = deferred();
  const controller = new SharingController(hub, { ...api, inspect: path => path === "A" ? old.promise : api.inspect(path) });
  const first = controller.select("A"); assert.equal(controller.getSnapshot().enabled, false);
  await controller.select("B"); old.resolve(preview("old", "A")); await first;
  assert.equal(controller.getSnapshot().inspection.package_name, "B"); assert.ok(released.includes("old"));
});

test("保存未完成禁止开始且失败会话关闭并通知调用方", async () => {
  const { hub, api } = setup(); const save = deferred();
  const controller = new SharingController(hub, { ...api, save: () => save.promise });
  await controller.select("A"); const result = controller.change(false);
  assert.equal(controller.getSnapshot().pending, true); assert.equal(controller.freeze(), undefined);
  save.reject(new Error("写盘失败")); await assert.rejects(result);
  assert.equal(controller.getSnapshot().enabled, false); assert.equal(controller.getSnapshot().pending, false);
});

test("任务显示冻结而另一页面仍可撤销且旧保存不污染新包", async () => {
  const { hub, api, create } = setup(); const protect = create(), sign = create();
  await protect.select("A"); await sign.select("A");
  assert.equal(protect.freeze().enabled, true);
  await sign.change(false); assert.equal(protect.getSnapshot().enabled, true); assert.equal(sign.getSnapshot().enabled, false);
  const save = deferred(); const other = new SharingController(hub, { ...api, save: () => save.promise });
  await other.select("C"); const saving = other.change(false); await other.select("D"); save.resolve(); await saving;
  assert.equal(other.getSnapshot().inspection.package_name, "D"); assert.equal(other.getSnapshot().enabled, true);
});

test("复查与重挂载只检查身份不触发任何提交", async () => {
  const { create } = setup(); const first = create();
  await first.select("A"); await first.change(false); await first.refresh();
  assert.equal(first.getSnapshot().enabled, false); first.dispose();
  const second = create(); await second.select("A"); assert.equal(second.getSnapshot().enabled, false);
});

test("取消保存中切换输入必须等保存完成再释放旧授权引用", async () => {
  const { hub, api, released } = setup(); const save = deferred();
  const controller = new SharingController(hub, { ...api, save: () => save.promise });
  await controller.select("A"); const id = controller.getSnapshot().inspection.inspection_id;
  const operation = controller.change(false); await controller.select("B");
  assert.equal(released.includes(id), false);
  save.resolve(); await operation; await Promise.resolve();
  assert.equal(released.includes(id), true);
});

test("空闲复查采用后台最新偏好但迟到检查不能覆盖检查期间的取消", async () => {
  const { hub, api } = setup(); let enabled = true;
  const controller = new SharingController(hub, { ...api, inspect: async () => preview("当前", "A", enabled) });
  const idle = new SharingController(hub, api);
  await controller.select("A"); await idle.select("A"); enabled = false; await controller.refresh();
  assert.equal(controller.getSnapshot().enabled, false);
  assert.equal(idle.getSnapshot().enabled, false);
  const delayed = deferred();
  const other = new SharingController(hub, { ...api, inspect: () => delayed.promise });
  const selecting = other.select("A"); hub.receive("A", false);
  delayed.resolve(preview("旧检查", "A", true)); await selecting;
  assert.equal(other.getSnapshot().enabled, false);
});
