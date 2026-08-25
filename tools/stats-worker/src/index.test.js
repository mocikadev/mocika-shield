import assert from "node:assert/strict";
import test from "node:test";

import { buildSummary, classifyPlatform, isDownloadAsset } from "./github-summary.js";
import { formatTrendResponse, normalizeFailureCounts } from "./index.js";

test("失败阶段只接受固定白名单并合并重复项", () => {
  assert.deepEqual(
    normalizeFailureCounts([
      { operation: "protect", stage: "manifest", count: 1 },
      { operation: "protect", stage: "manifest", count: 2 },
      { operation: "task", stage: "cancelled", count: 1 },
    ]),
    [
      { operation: "protect", stage: "manifest", count: 3 },
      { operation: "task", stage: "cancelled", count: 1 },
    ],
  );
  assert.throws(
    () => normalizeFailureCounts([{ operation: "protect", stage: "raw_error", count: 1 }]),
    /失败阶段无效/,
  );
});

test("趋势响应保持旧数据数组并提供版本与失败细分", () => {
  assert.deepEqual(
    formatTrendResponse(14, [{ usage_date: "2026-08-25", active_devices: 1 }], [], []),
    {
      schema_version: 2,
      window_days: 14,
      data: [{ usage_date: "2026-08-25", active_devices: 1 }],
      versions: [],
      failure_breakdown: [],
    },
  );
});

test("校验和文件不计入下载量", () => {
  assert.equal(isDownloadAsset("MocikaShield_windows_x64_setup.exe"), true);
  assert.equal(isDownloadAsset("windows-checksums-sha256.txt"), false);
});

test("发布产物按系统平台归类", () => {
  assert.equal(classifyPlatform("MocikaShield_windows_x64_setup.exe"), "Windows");
  assert.equal(classifyPlatform("MocikaShield_macos_universal.dmg"), "macOS");
  assert.equal(classifyPlatform("MocikaShield_linux_amd64.AppImage"), "Linux");
});

test("当前汇总区分今日数据和最近完整日", () => {
  const summary = buildSummary(
    { stargazers_count: 8, forks_count: 2, open_issues_count: 3 },
    [{ assets: [
      { name: "MocikaShield_windows_x64_setup.exe", download_count: 9 },
      { name: "windows-checksums-sha256.txt", download_count: 4 },
    ] }],
    [
      { usage_date: "2026-07-22", active_devices: 4 },
      { usage_date: "2026-07-23", active_devices: 1 },
    ],
    new Date("2026-07-23T08:00:00Z"),
  );

  assert.equal(summary.downloads.total, 9);
  assert.equal(summary.repository.stars, 8);
  assert.equal(summary.usage.today.active_devices, 1);
  assert.equal(summary.usage.latest_complete.active_devices, 4);
});
