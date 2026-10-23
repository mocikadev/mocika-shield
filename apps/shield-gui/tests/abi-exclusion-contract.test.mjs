import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

// 防止预检允许后，工控模式又在前端静默丢弃单次排除确认。
test("两种模式均复用预检给出的单次排除清单与确认", () => {
  const source = readFileSync(new URL("../src/hooks/use-protect-workflow.ts", import.meta.url), "utf8");
  assert.match(source, /const exclusions = abiRisk\?\.detail \? abiRisk\.detail\.split\("、"\) : \[\];/);
  assert.match(source, /exclusions\.length > 0 && !await confirm/);
  assert.match(source, /setExcludedAbis\(exclusions\)/);
  assert.doesNotMatch(source, /runtimeMode === "standard" && abiRisk/);
});
