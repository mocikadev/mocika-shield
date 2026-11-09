import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { URL } from "node:url";
import test from "node:test";
import ts from "typescript";

// 执行 App 实际注册的焦点 effect，替换事件源和查询；不靠测试副本模拟实现。
const source = ts.createSourceFile("App.tsx", readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8"), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
const effects = [];
function visit(node) {
  if (ts.isCallExpression(node) && node.expression.getText(source) === "useEffect") {
    const effect = node.arguments[0];
    if (effect.getText(source).includes('"tauri://focus"')) effects.push(effect);
  }
  ts.forEachChild(node, visit);
}
visit(source);
assert.equal(effects.length, 1, "原生焦点刷新必须只有一处注册");
const compiled = ts.transpileModule(`(${effects[0].getText(source)})`, {
  compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.ESNext },
}).outputText;
function start(registration = Promise.resolve(() => {})) {
  const calls = []; let focus;
  const effect = new Function("getCurrentWindow", "refreshConfig", "refreshCertificates", "notifyError", `return ${compiled}`)(
    () => ({ listen: (event, handler) => { assert.equal(event, "tauri://focus"); focus = handler; return registration; } }),
    () => { calls.push("配置"); },
    (silent) => { assert.equal(silent, true); calls.push("证书"); },
    () => { calls.push("注册失败提示"); },
  );
  return { calls, cleanup: effect(), focus: () => focus() };
}

test("原生窗口恢复焦点同时刷新配置及证书，卸载后迟到事件不查询", async () => {
  let stopped = 0;
  const run = start(Promise.resolve(() => { ++stopped; }));
  run.focus(); assert.deepEqual(run.calls, ["配置", "证书"]);
  run.cleanup(); run.focus(); await Promise.resolve(); await Promise.resolve();
  assert.deepEqual(run.calls, ["配置", "证书"]); assert.equal(stopped, 1);
});

test("卸载先于异步注册完成时仍释放原生监听", async () => {
  let finish; let stopped = 0;
  const registration = new Promise((resolve) => { finish = resolve; });
  const run = start(registration); run.cleanup();
  finish(() => { ++stopped; }); await Promise.resolve(); await Promise.resolve();
  run.focus(); assert.deepEqual(run.calls, []); assert.equal(stopped, 1);
});

test("注册失败被处理，卸载后不再弹出迟到错误", async () => {
  const run = start(Promise.reject(new Error("测试注册失败")));
  await Promise.resolve(); assert.deepEqual(run.calls, ["注册失败提示"]); run.cleanup();
  const late = start(Promise.reject(new Error("测试迟到失败"))); late.cleanup();
  await Promise.resolve(); assert.deepEqual(late.calls, []);
});
