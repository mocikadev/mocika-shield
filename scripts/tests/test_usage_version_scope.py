"""用 SQLite 校验私有聚合查询；不连接线上、不保存匿名明细。"""
import json
from pathlib import Path
import sqlite3
import unittest

SQL = (Path(__file__).resolve().parents[2] / "tools/stats-worker/queries/usage-version-scope.sql").read_text()
REPORT_SQL = (Path(__file__).resolve().parents[2] / "tools/stats-worker/queries/report-version-scope.sql").read_text()


class UsageVersionScopeTests(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.addCleanup(self.db.close)
        self.db.row_factory = sqlite3.Row
        for table in ("daily_usage", "daily_usage_v2", "daily_usage_v3"):
            self.db.execute(f"CREATE TABLE {table} (anonymous_id TEXT, usage_date TEXT, app_version TEXT, "
                            "app_start_count INTEGER, protect_success_count INTEGER, protect_failed_count INTEGER, "
                            "sign_success_count INTEGER, sign_failed_count INTEGER)")

    def add(self, table, identity, version, count, day="2026-01-02"):
        self.db.execute(f"INSERT INTO {table} VALUES (?,?,?,?,?,?,?,?)",
                        (identity, day, version, count, count, 0, count, 0))

    def query(self, excluded=None):
        rows = self.db.execute(SQL, ("2026-01-01", "2026-01-03", json.dumps(["1.4.3"]), excluded))
        return {r["scope"]: dict(r) for r in rows}

    def test_跨版本跨日去重且分组实例不能相加(self):
        self.add("daily_usage_v2", "a", "1.4.3", 2)
        self.add("daily_usage_v2", "a", "1.4.3", 3, "2026-01-03")
        self.add("daily_usage_v2", "a", "9.0.0", 4)
        self.add("daily_usage_v2", "b", "9.0.0", 1)
        result = self.query()
        self.assertEqual(result["all_received"]["active_instances"], 2)
        self.assertEqual(result["release_version_match"]["active_instances"], 1)
        self.assertEqual(result["unmatched_version"]["active_instances"], 2)
        self.assertEqual(result["all_received"]["protect_successes"], 10)

    def test_新表优先且可排除自测不返回身份(self):
        self.add("daily_usage", "a", "1.4.3", 99)
        self.add("daily_usage_v2", "a", "1.4.3", 2)
        self.add("daily_usage", "b", "1.4.3", 3)
        result = self.query("b")
        self.assertEqual(result["all_received"]["protect_successes"], 2)
        self.assertNotIn("anonymous_id", result["all_received"])
        self.assertEqual(self.db.total_changes, 3, "查询不得改写原始记录")

    def test_空窗口不是失败也不伪造活跃(self):
        self.assertEqual(self.query(), {})

    def test_分享跨版本按包名去重且错误报告只返回数量(self):
        self.db.execute("CREATE TABLE application_usage_submissions "
                        "(tool_version TEXT,package_name TEXT,operation TEXT,success_date TEXT)")
        self.db.execute("CREATE TABLE error_reports (app_version TEXT,received_at TEXT)")
        self.db.executemany("INSERT INTO application_usage_submissions VALUES (?,?,?,?)", [
            ("1.4.3", "example.app", "protect", "2026-01-02"),
            ("1.4.3", "example.app", "sign", "2026-01-02"),
            ("9.0.0", "example.app", "protect", "2026-01-02"),
        ])
        self.db.executemany("INSERT INTO error_reports VALUES (?,?)", [
            ("1.4.3", "2026-01-02T13:00:00Z"), ("9.0.0", "2026-01-02T13:00:00Z"),
        ])
        result = {(r["channel"], r["scope"]): dict(r) for r in self.db.execute(
            REPORT_SQL, ("2026-01-01", "2026-01-03", '["1.4.3"]'))}
        shared = result[("application_share", "all_received")]
        self.assertEqual((shared["records"], shared["packages"], shared["protected_packages"]), (3, 1, 1))
        self.assertEqual(result[("error_report", "all_received")]["records"], 2)
        self.assertIsNone(result[("error_report", "release_version_match")]["packages"])
        self.assertNotIn("example.app", json.dumps(result[("application_share", "all_received")]))
