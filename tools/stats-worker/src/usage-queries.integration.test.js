import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { queryActivityWindows, queryUsage } from "./usage-queries.js";
import { SqliteD1 } from "./sqlite-d1.test-support.js";

function database(t) {
  const dir = mkdtempSync(join(tmpdir(), "shield-usage-query-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const db = new SqliteD1(join(dir, "usage.db"));
  db.exec(readFileSync(new URL("../schema.sql", import.meta.url), "utf8"));
  return db;
}

function seed(db) {
  db.exec(`
    INSERT INTO daily_usage_v2(anonymous_id,usage_date,app_version,platform,protect_success_count,created_at) VALUES
      ('a',date('now'),'1.4.2-rc.3','windows',2,datetime('now')),
      ('a',date('now'),'1.4.2-rc.4','windows',3,datetime('now')),
      ('a',date('now','-1 day'),'1.4.2-rc.3','windows',1,datetime('now')),
      ('b',date('now','-6 days'),'1.4.1','macos',1,datetime('now')),
      ('c',date('now','-7 days'),'1.4.1','macos',1,datetime('now')),
      ('d',date('now','-29 days'),'1.4.1','macos',1,datetime('now')),
      ('e',date('now','-30 days'),'1.4.1','macos',1,datetime('now')),
      ('future',date('now','+1 day'),'1.4.1','macos',1,datetime('now'));
    INSERT INTO daily_usage(anonymous_id,usage_date,app_version,platform,protect_success_count,created_at) VALUES
      ('a',date('now'),'1.4.2-rc.3','windows',99,datetime('now')),
      ('f',date('now'),'1.3.0','linux',1,datetime('now'));
  `);
}

test("同日跨版本实例去重且事件保留各版次数和遗留表非重叠数据", async (t) => {
  const db = database(t);
  seed(db);
  const [today] = await queryUsage({ DB: db }, 1);
  assert.equal(today.active_devices, 2);
  assert.equal(today.protect_successes, 6);
  assert.equal(today.usage_date, new Date().toISOString().slice(0, 10));
  const versions = await queryUsage({ DB: db }, 1, true);
  assert.deepEqual(versions.map(row => [row.app_version, row.active_devices, row.protect_successes]), [
    ["1.3.0", 1, 1], ["1.4.2-rc.3", 1, 2], ["1.4.2-rc.4", 1, 3],
  ]);
  assert.equal(JSON.stringify(today).includes("anonymous_id"), false);
});

test("七日和三十日窗口跨日期版本去重并包含准确日期边界", async (t) => {
  const db = database(t);
  seed(db);
  const rows = await queryActivityWindows({ DB: db });
  assert.deepEqual(rows.map(row => [row.window_days, row.active_devices]), [[7, 3], [30, 5]]);
  for (const row of rows) {
    assert.equal((Date.parse(row.end_date) - Date.parse(row.start_date)) / 86400000, row.window_days - 1);
    assert.equal(row.end_date, new Date().toISOString().slice(0, 10));
  }
});

test("空数据库日趋势为空但窗口返回真实零值", async (t) => {
  const env = { DB: database(t) };
  assert.deepEqual(await queryUsage(env, 14), []);
  assert.deepEqual((await queryActivityWindows(env)).map(row => row.active_devices), [0, 0]);
});
