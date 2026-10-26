import assert from "node:assert/strict";
import test from "node:test";
import worker from "./index.js";

function setup(t, { githubFailure = false, databaseFailure = false } = {}) {
  const originalCaches = Object.getOwnPropertyDescriptor(globalThis, "caches");
  Object.defineProperty(globalThis, "caches", { configurable: true, value: {
    default: { async match() {}, async put() {} },
  } });
  t.after(() => {
    if (originalCaches) Object.defineProperty(globalThis, "caches", originalCaches);
    else delete globalThis.caches;
  });
  t.mock.method(globalThis, "fetch", async (url, options) => {
    if (githubFailure === true || (githubFailure === "repository" && !url.endsWith("per_page=100"))) {
      return new Response("禁止公开的上游错误正文", { status: 403 });
    }
    if (githubFailure === "timeout") throw new DOMException("禁止公开的超时细节", "TimeoutError");
    assert.ok(options.signal instanceof AbortSignal);
    return Response.json(url.endsWith("per_page=100") ? [
      { draft: false, assets: [{ name: "gui.exe", download_count: 4 }, { name: "checksums.txt", download_count: 9 }] },
      { draft: true, assets: [{ name: "private.exe", download_count: 99 }] },
    ] : { stargazers_count: 101, forks_count: 23, open_issues_count: 4 });
  });
  t.mock.method(console, "error", () => {});
  return {
    DB: { prepare() { return { bind() { return { async all() {
      if (databaseFailure) throw new Error("禁止公开的数据库内容");
      return { results: [{ usage_date: new Date().toISOString().slice(0, 10), active_devices: 7 }] };
    } }; } }; } },
  };
}

const request = path => new Request(`https://stats.invalid/stats/${path}`);
const ctx = { waitUntil(promise) { return promise; } };

test("GitHub限流仅降级关注下载字段仍返回可用统计且不暴露上游正文", async (t) => {
  const env = setup(t, { githubFailure: true });
  const response = await worker.fetch(request("summary"), env, ctx);
  assert.equal(response.status, 200);
  const body = await response.json();
  assert.equal(body.repository, null);
  assert.equal(body.downloads, null);
  assert.deepEqual(body.availability, { repository: false, downloads: false, usage: true });
  assert.equal(body.usage.today.active_devices, 7);
  assert.equal(JSON.stringify(body).includes("上游错误正文"), false);
});

test("汇总成功保持旧字段不计草稿并对GitHub请求设置超时", async (t) => {
  const env = setup(t);
  const response = await worker.fetch(request("summary"), env, ctx);
  assert.equal(response.status, 200);
  const body = await response.json();
  assert.equal(body.repository.stars, 101);
  assert.equal(body.downloads.total, 4);
  assert.equal(body.usage.today.active_devices, 7);
  assert.deepEqual(body.availability, { repository: true, downloads: true, usage: true });
});

test("仓库信息失败不丢弃可用下载数据", async (t) => {
  const response = await worker.fetch(request("summary"), setup(t, { githubFailure: "repository" }), ctx);
  const body = await response.json();
  assert.equal(response.status, 200);
  assert.equal(body.repository, null);
  assert.equal(body.downloads.total, 4);
  assert.deepEqual(body.availability, { repository: false, downloads: true, usage: true });
});

test("GitHub超时仍能读取使用数据", async (t) => {
  const response = await worker.fetch(request("summary"), setup(t, { githubFailure: "timeout" }), ctx);
  const body = await response.json();
  assert.equal(response.status, 200);
  assert.equal(body.usage.today.active_devices, 7);
  assert.equal(body.repository, null);
  assert.equal(JSON.stringify(body).includes("超时细节"), false);
});

test("汇总数据库异步失败由入口捕获并返回受控503", async (t) => {
  const response = await worker.fetch(request("summary"), setup(t, { databaseFailure: true }), ctx);
  assert.equal(response.status, 503);
  assert.equal((await response.text()).includes("数据库内容"), false);
});

test("趋势数据库异步失败由入口捕获并返回受控503", async (t) => {
  const response = await worker.fetch(request("trend"), setup(t, { databaseFailure: true }), ctx);
  assert.equal(response.status, 503);
});
