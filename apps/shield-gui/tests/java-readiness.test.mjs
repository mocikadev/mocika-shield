import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { Buffer } from "node:buffer";
import { URL } from "node:url";
import test from "node:test";
import ts from "typescript";

// 与现有 Node 测试共用执行方式，不为前端纯函数引入测试框架或启动桌面程序。
function moduleUrl(source) {
  const code = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 },
  }).outputText;
  return `data:text/javascript;base64,${Buffer.from(code).toString("base64")}`;
}

const translations = moduleUrl(readFileSync(new URL("../src/lib/i18n.ts", import.meta.url), "utf8"));
const source = readFileSync(new URL("../src/lib/java.ts", import.meta.url), "utf8")
  .replace('from "@/lib/i18n"', `from "${translations}"`);
const { getProtectJavaError, getSignJavaError, getAliasJavaError } = await import(moduleUrl(source));
const info = {
  java_ready: true, keytool_ready: false, java_major: 8, min_java_major: 8,
  java_version: "1.8.0", apktool_version: "3.0.1", apksigner_version: "0.9",
};

for (const locale of ["zh", "en"]) {
  test(`${locale}：纯加固不因无关 keytool 缺失被拦截`, () => {
    assert.equal(getProtectJavaError(locale, info, false), "");
  });

  test(`${locale}：已保存证书独立签名不要求 keytool`, () => {
    assert.equal(getSignJavaError(locale, info), "");
  });

  test(`${locale}：自动签名与证书识别继续要求 keytool`, () => {
    assert.match(getProtectJavaError(locale, info, true), /keytool/);
    assert.match(getAliasJavaError(locale, info), /keytool/);
    assert.equal(getProtectJavaError(locale, { ...info, keytool_ready: true }, true), "");
  });

  test(`${locale}：缺少或过低 Java 时各流程仍被拦截`, () => {
    for (const bad of [null, { ...info, java_ready: false, java_major: null }, { ...info, java_ready: false, java_major: 7 }]) {
      assert.notEqual(getProtectJavaError(locale, bad, false), "");
      assert.notEqual(getSignJavaError(locale, bad), "");
      assert.notEqual(getAliasJavaError(locale, bad), "");
    }
  });

  test(`${locale}：探测未完成不执行且缺工具提示包含修正引导`, () => {
    assert.notEqual(getProtectJavaError(locale, null, true), "");
    assert.match(getAliasJavaError(locale, info), /JDK/);
    assert.match(getAliasJavaError(locale, info), /关于|About/);
  });
}

test("加固工作流按本次自动签名意图检查工具", () => {
  const workflow = readFileSync(new URL("../src/hooks/use-protect-workflow.ts", import.meta.url), "utf8");
  assert.match(workflow, /getProtectJavaError\(locale, buildInfo, autoSignReady\)/);
});
