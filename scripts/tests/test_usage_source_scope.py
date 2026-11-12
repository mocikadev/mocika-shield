"""来源私有查询只在隔离 SQLite 验证，不连接生产或输出用户明细。"""
from pathlib import Path
import sqlite3
import unittest

WORKER = Path(__file__).resolve().parents[2] / "tools/stats-worker"


class UsageSourceScopeTests(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.addCleanup(self.db.close)
        self.db.row_factory = sqlite3.Row
        self.db.executescript((WORKER / "schema.sql").read_text())

    def legacy(self, table, identity="a", count=3):
        self.db.execute(f"INSERT INTO {table} "
                        "(anonymous_id,usage_date,app_version,platform,app_start_count,protect_success_count,created_at) "
                        "VALUES (?,date('now'),'1.5.0-beta.2','macos',?,?,datetime('now'))",
                        (identity, count, count))

    def source(self, key, identity="a", name="测试工具", app_id="dev.example.tool", version="1.5.0-beta.2", count=2):
        self.db.execute("INSERT INTO daily_usage_v3 "
                        "(anonymous_id,usage_date,app_version,source_key,product_name,application_id,build_revision,"
                        "platform,app_start_count,protect_success_count,created_at) "
                        "VALUES (?,date('now'),?,?,?,?,?,'macos',?,?,datetime('now'))",
                        (identity, version, key, name, app_id, "abcdef01", count, count))

    def scope(self, excluded=None):
        sql = (WORKER / "queries/usage-source-scope.sql").read_text()
        rows = self.db.execute(sql, ("2000-01-01", "2099-01-01", '["1.5.0-beta.2"]',
                                     "测试工具", "dev.example.tool", excluded))
        return {row["scope"]: dict(row) for row in rows}

    def test_旧未知和跨来源分段保留且跨组实例不可相加(self):
        self.legacy("daily_usage", count=99)
        self.legacy("daily_usage_v2", count=3)
        self.source("first", count=2)
        self.source("second", name="另一个工具", count=4)
        result = self.scope()
        self.assertEqual(result["all_received"]["active_instances"], 1)
        self.assertEqual(result["all_received"]["protect_successes"], 9)
        self.assertEqual(result["project_identity_match"]["protect_successes"], 2)
        self.assertEqual(result["other_declared_identity"]["protect_successes"], 4)
        self.assertEqual(result["unknown_source"]["protect_successes"], 3)
        self.assertEqual(sum(value["active_instances"] for key, value in result.items()
                             if key != "all_received"), 3)
        self.assertNotIn("anonymous_id", result["all_received"])

    def test_匹配同时要求名称标识和公开版本且未知实例进入覆盖率分母(self):
        self.source("matched")
        self.source("name", identity="b", name="改名工具")
        self.source("id", identity="c", app_id="dev.other.tool")
        self.source("version", identity="d", version="9.0.0")
        self.legacy("daily_usage_v2", identity="e")
        result = self.scope()
        self.assertEqual(result["project_identity_match"]["active_instances"], 1)
        self.assertEqual(result["other_declared_identity"]["active_instances"], 3)
        self.assertEqual(result["unknown_source"]["active_instances"], 1)
        self.assertEqual(result["all_received"]["identity_coverage_percent"], 80)
        self.assertEqual(self.scope("a")["all_received"]["active_instances"], 4)
        self.assertNotIn("project_identity_match", self.scope("a"))

    def test_版本明细仅返回来源聚合且只读不回填旧记录(self):
        self.source("one")
        self.source("two", version="1.5.0-beta.3")
        self.legacy("daily_usage_v2")
        before = self.db.total_changes
        rows = list(self.db.execute((WORKER / "queries/usage-source-versions.sql").read_text(),
                                   ("2000-01-01", "2099-01-01", None)))
        self.assertEqual(len(rows), 2)
        self.assertTrue(all(row["product_name"] == "测试工具" for row in rows))
        self.assertTrue(all("anonymous_id" not in row.keys() for row in rows))
        self.assertEqual(self.db.total_changes, before)
        self.assertEqual(self.db.execute("SELECT COUNT(*) FROM daily_usage_v2").fetchone()[0], 1)

    def test_地区活跃分母包括仅有第三代上报的实例(self):
        self.source("one")
        rows = list(self.db.execute((WORKER / "queries/country-distribution.sql").read_text(),
                                   ("2000-01-01", "2099-01-01", None)))
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["total_active_instances"], 1)
        self.assertIsNone(rows[0]["country_code"])
        self.assertEqual(rows[0]["coverage_percent"], 0)
