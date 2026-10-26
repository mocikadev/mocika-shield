import json
import io
import tempfile
import unittest
import urllib.error
from contextlib import redirect_stderr
from datetime import datetime, timedelta, timezone
from pathlib import Path
from unittest.mock import patch

from scripts.project_stats import (
    build_snapshot,
    classify_platform,
    collect_usage_stats,
    collect_payload,
    load_history,
    merge_snapshot,
    write_outputs,
)


class ProjectStatsTests(unittest.TestCase):
    @patch("scripts.project_stats.urllib.request.urlopen")
    def test_滚动活跃只保留聚合白名单且旧接口不伪造零(self, urlopen):
        body = {"data": [], "activity_windows": [{
            "window_days": 7, "start_date": "2026-10-20", "end_date": "2026-10-26",
            "active_devices": 12, "anonymous_id": "不允许保留的标识", "payload_json": "报告正文",
        }]}
        urlopen.return_value.__enter__.return_value = io.BytesIO(json.dumps(body).encode())
        usage = collect_usage_stats("https://stats.invalid/stats/trend")
        self.assertTrue(usage["activity_windows_available"])
        self.assertEqual(usage["activity_windows"], [{
            "window_days": 7, "start_date": "2026-10-20", "end_date": "2026-10-26", "active_devices": 12,
        }])
        self.assertNotIn("报告正文", json.dumps(usage, ensure_ascii=False))
        urlopen.return_value.__enter__.return_value = io.BytesIO(b'{"data": []}')
        old = collect_usage_stats("https://stats.invalid/stats/trend")
        self.assertFalse(old["activity_windows_available"])
        self.assertIsNone(old["activity_windows"])

    def test_版本匹配仅依据公开发布且不修改原统计(self):
        usage = {"version_trend": [{"app_version": "1.4.1"}, {"app_version": "1.9.0"}, {"app_version": "1.4.2-rc.4"}]}
        payload = {"repository": {}, "views": {}, "clones": {}, "usage": usage, "releases": [
            {"tag_name": "v1.4.1", "published_at": "2026-10-08T00:00:00Z", "draft": False, "assets": []},
            {"tag_name": "v1.4.2-rc.4", "published_at": "2026-10-23T00:00:00Z", "draft": True,
             "assets": [{"name": "private.exe", "download_count": 99}]},
        ]}
        snapshot = build_snapshot(payload, datetime(2026, 10, 26, tzinfo=timezone.utc))
        self.assertEqual([r["release_version_match"] for r in snapshot["usage"]["version_trend"]], [True, False, False])
        self.assertEqual(snapshot["totals"]["downloads"], 0)
        self.assertNotIn("release_version_match", usage["version_trend"][0])
        payload["releases_available"] = False
        unknown = build_snapshot(payload, datetime(2026, 10, 26, tzinfo=timezone.utc))
        self.assertEqual([r["release_version_match"] for r in unknown["usage"]["version_trend"]], [None, None, None])

    @patch("scripts.project_stats.urllib.request.urlopen")
    def test_非法窗口不影响已有日趋势且不写成零(self, urlopen):
        yesterday = (datetime.now(timezone.utc).date() - timedelta(days=1)).isoformat()
        for windows in ("bad", [{"window_days": 7, "active_devices": -1}], [{"window_days": 7}]):
            urlopen.return_value.__enter__.return_value = io.BytesIO(json.dumps({
                "data": [{"usage_date": yesterday, "active_devices": 3}], "activity_windows": windows,
            }).encode())
            with self.subTest(windows=windows), redirect_stderr(io.StringIO()):
                usage = collect_usage_stats("https://stats.invalid/stats/trend")
                self.assertEqual(usage["active_devices"], 3)
                self.assertFalse(usage["activity_windows_available"])
                self.assertIsNone(usage["activity_windows"])

    @patch("scripts.project_stats.collect_usage_stats", return_value={"available": True, "active_devices": 7})
    @patch("scripts.project_stats.github_get", side_effect=RuntimeError("GitHub API 请求失败：状态码 403"))
    def test_GitHub不可用不影响使用采集也不把关注下载写成零(self, github_get, usage):
        with redirect_stderr(io.StringIO()):
            payload = collect_payload("mocikadev/mocika-shield", "test-token")
        snapshot = build_snapshot(payload, datetime(2026, 10, 26, tzinfo=timezone.utc))
        self.assertEqual(snapshot["usage"]["active_devices"], 7)
        self.assertIsNone(snapshot["repository"]["stars"])
        self.assertIsNone(snapshot["totals"]["downloads"])
        self.assertFalse(snapshot["release_assets_available"])

    @patch("scripts.project_stats.collect_usage_stats", return_value={"available": True, "active_devices": 7})
    @patch("scripts.project_stats.github_get", side_effect=json.JSONDecodeError("无效响应", "bad", 0))
    def test_GitHub无效JSON不影响使用采集(self, github_get, usage):
        with redirect_stderr(io.StringIO()):
            payload = collect_payload("mocikadev/mocika-shield", "test-token")
        snapshot = build_snapshot(payload, datetime(2026, 10, 26, tzinfo=timezone.utc))
        self.assertEqual(snapshot["usage"]["active_devices"], 7)
        self.assertIsNone(snapshot["totals"]["downloads"])

    @patch("scripts.project_stats.urllib.request.urlopen")
    def test_所有公开聚合只保留白名单且丢弃混入的私有字段(self, urlopen):
        yesterday = (datetime.now(timezone.utc).date() - timedelta(days=1)).isoformat()
        private = {"anonymous_id": "不允许保存的标识", "payload_json": "不允许保存的报告"}
        payload = {
            "data": [{"usage_date": yesterday, "active_devices": 3, **private}],
            "versions": [{"usage_date": yesterday, "app_version": "1.4.1", "active_devices": 3, **private}],
            "failure_breakdown": [{"usage_date": yesterday, "app_version": "1.4.1", "operation": "protect", "stage": "manifest", "count": 1, **private}],
        }
        urlopen.return_value.__enter__.return_value = io.BytesIO(json.dumps(payload).encode())
        result = collect_usage_stats("https://stats.invalid/stats/trend")
        self.assertEqual(result["active_devices"], 3)
        self.assertEqual(result["version_trend"][0]["app_version"], "1.4.1")
        self.assertEqual(result["failure_breakdown"][0]["count"], 1)
        serialized = json.dumps(result, ensure_ascii=False)
        self.assertNotIn("不允许保存", serialized)
        self.assertNotIn("anonymous_id", serialized)
        self.assertNotIn("payload_json", serialized)

    @patch("scripts.project_stats.urllib.request.urlopen")
    def test_新原因聚合保留版本但丢弃报告正文(self, urlopen):
        yesterday = (datetime.now(timezone.utc).date() - timedelta(days=1)).isoformat()
        payload = {"data": [], "failure_reason_breakdown": [{
            "usage_date": yesterday, "app_version": "1.4.0-beta.5", "flow": "sign",
            "operation": "sign", "stage": "execute", "code": "SIGNING_FAILED",
            "classifier_version": 1, "count": 2, "payload_json": "不允许保存的报告正文",
        }], "failure_classifier_coverage": [{"usage_date": yesterday,
            "app_version": "1.4.0-beta.5", "failure_classifier_version": 1,
            "reporting_devices": 2, "failure_count": 2}]}
        urlopen.return_value.__enter__.return_value = io.BytesIO(json.dumps(payload).encode())
        usage = collect_usage_stats("https://stats.example.test/trend")
        self.assertTrue(usage["failure_reason_breakdown_available"])
        self.assertEqual(usage["failure_reason_breakdown"][0]["count"], 2)
        self.assertEqual(usage["failure_classifier_coverage"][0]["app_version"], "1.4.0-beta.5")
        self.assertNotIn("报告正文", json.dumps(usage, ensure_ascii=False))
        urlopen.return_value.__enter__.return_value = io.BytesIO(b'{"data": []}')
        old = collect_usage_stats("https://stats.example.test/trend")
        self.assertFalse(old["failure_reason_breakdown_available"])

    def test_classify_platform(self):
        self.assertEqual(classify_platform("MocikaShield_windows_x64_setup.exe"), "Windows")
        self.assertEqual(classify_platform("MocikaShield_macos_universal.dmg"), "macOS")
        self.assertEqual(classify_platform("MocikaShield_linux_amd64.AppImage"), "Linux")

    def test_snapshot_excludes_checksums_and_replaces_same_day(self):
        payload = {
            "repository": {"stargazers_count": 7, "forks_count": 2, "open_issues_count": 1},
            "views": {"count": 20, "uniques": 8},
            "clones": {"count": 12, "uniques": 5},
            "releases": [
                {
                    "tag_name": "v1.0.0",
                    "published_at": "2026-01-01T00:00:00Z",
                    "assets": [
                        {"name": "MocikaShield_1.0.0_windows_x64_setup.exe", "download_count": 9},
                        {"name": "checksums-sha256.txt", "download_count": 3},
                    ],
                }
            ],
        }
        snapshot = build_snapshot(payload, datetime(2026, 7, 10, tzinfo=timezone.utc))
        self.assertEqual(snapshot["totals"]["downloads"], 9)
        self.assertEqual(snapshot["totals"]["platform_downloads"]["Windows"], 9)

        history = {"schema_version": 1, "repository": "mocikadev/mocika-shield", "snapshots": []}
        merge_snapshot(history, snapshot)
        changed = json.loads(json.dumps(snapshot))
        changed["totals"]["downloads"] = 10
        merge_snapshot(history, changed)
        self.assertEqual(len(history["snapshots"]), 1)
        self.assertEqual(history["snapshots"][0]["totals"]["downloads"], 10)

    def test_write_outputs(self):
        payload = {
            "repository": {"stargazers_count": 1, "forks_count": 0, "open_issues_count": 0},
            "views": {"count": 3, "uniques": 2},
            "clones": {"count": 1, "uniques": 1},
            "releases": [],
        }
        snapshot = build_snapshot(payload, datetime(2026, 7, 10, tzinfo=timezone.utc))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            history = load_history(root / "missing.json", "mocikadev/mocika-shield")
            merge_snapshot(history, snapshot)
            write_outputs(history, root)
            self.assertTrue((root / "data/history.json").exists())
            self.assertFalse((root / "index.html").exists())
            saved = json.loads((root / "data/history.json").read_text(encoding="utf-8"))
            self.assertEqual(saved["snapshots"][-1]["totals"]["downloads"], 0)

    def test_unavailable_traffic_is_not_recorded_as_zero(self):
        payload = {
            "repository": {"stargazers_count": 1, "forks_count": 0, "open_issues_count": 0},
            "views": {"available": False, "count": None, "uniques": None},
            "clones": {"available": False, "count": None, "uniques": None},
            "releases": [],
        }
        snapshot = build_snapshot(payload, datetime(2026, 7, 10, tzinfo=timezone.utc))
        self.assertFalse(snapshot["traffic"]["available"])
        self.assertIsNone(snapshot["traffic"]["unique_visitors"])

    @patch("scripts.project_stats.urllib.request.urlopen")
    def test_usage_stats_使用明确请求标识(self, urlopen):
        response = io.BytesIO(
            json.dumps(
                {
                    "data": [
                        {
                            "usage_date": "2026-07-10",
                            "active_devices": 2,
                            "app_starts": 3,
                            "protect_successes": 1,
                            "protect_failures": 0,
                        }
                    ]
                }
            ).encode()
        )
        urlopen.return_value.__enter__.return_value = response

        usage = collect_usage_stats("https://stats.example.test/trend")

        request = urlopen.call_args.args[0]
        self.assertEqual(request.get_header("User-agent"), "mocika-shield-project-stats")
        self.assertTrue(usage["available"])
        self.assertEqual(usage["app_starts"], 3)
        self.assertEqual(usage["trend"][0]["date"], "2026-07-10")

        payload = {
            "repository": {"stargazers_count": 1, "forks_count": 0, "open_issues_count": 0},
            "views": {"count": 3, "uniques": 2},
            "clones": {"count": 1, "uniques": 1},
            "releases": [],
            "usage": usage,
        }
        snapshot = build_snapshot(payload, datetime(2026, 7, 10, tzinfo=timezone.utc))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            history = load_history(root / "missing.json", "mocikadev/mocika-shield")
            merge_snapshot(history, snapshot)
            write_outputs(history, root)
            saved = json.loads((root / "data/history.json").read_text(encoding="utf-8"))
            self.assertEqual(saved["snapshots"][-1]["usage"]["trend"][0]["date"], "2026-07-10")

    @patch("scripts.project_stats.urllib.request.urlopen")
    def test_usage_stats_接口失败时明确标记不可用(self, urlopen):
        urlopen.side_effect = urllib.error.HTTPError(
            "https://stats.example.test/trend", 403, "Forbidden", {}, None
        )
        stderr = io.StringIO()

        with redirect_stderr(stderr):
            usage = collect_usage_stats("https://stats.example.test/trend")

        self.assertFalse(usage["available"])
        self.assertIn("接口不可用", stderr.getvalue())

    @patch("scripts.project_stats.urllib.request.urlopen")
    def test_usage_stats_历史趋势排除当天未完成数据(self, urlopen):
        today = datetime.now(timezone.utc).date()
        yesterday = today - timedelta(days=1)
        response = io.BytesIO(
            json.dumps(
                {
                    "data": [
                        {"usage_date": yesterday.isoformat(), "active_devices": 3, "app_starts": 4},
                        {"usage_date": today.isoformat(), "active_devices": 1, "app_starts": 1},
                    ]
                }
            ).encode()
        )
        urlopen.return_value.__enter__.return_value = response

        usage = collect_usage_stats("https://stats.example.test/trend")

        self.assertEqual(usage["active_devices"], 3)
        self.assertEqual(usage["latest_complete_date"], yesterday.isoformat())
        self.assertEqual([row["date"] for row in usage["trend"]], [yesterday.isoformat()])

    @patch("scripts.project_stats.urllib.request.urlopen")
    def test_usage_stats_保留新版版本与失败细分(self, urlopen):
        yesterday = (datetime.now(timezone.utc).date() - timedelta(days=1)).isoformat()
        response = io.BytesIO(
            json.dumps(
                {
                    "schema_version": 2,
                    "data": [{"usage_date": yesterday, "active_devices": 2}],
                    "versions": [
                        {
                            "usage_date": yesterday,
                            "app_version": "1.4.0-alpha.2",
                            "protect_successes": 1,
                        }
                    ],
                    "failure_breakdown": [
                        {
                            "usage_date": yesterday,
                            "app_version": "1.4.0-alpha.2",
                            "operation": "protect",
                            "stage": "manifest",
                            "count": 1,
                        }
                    ],
                }
            ).encode()
        )
        urlopen.return_value.__enter__.return_value = response

        usage = collect_usage_stats("https://stats.example.test/trend")

        self.assertTrue(usage["version_breakdown_available"])
        self.assertTrue(usage["failure_breakdown_available"])
        self.assertEqual(usage["version_trend"][0]["app_version"], "1.4.0-alpha.2")
        self.assertEqual(usage["failure_breakdown"][0]["stage"], "manifest")


if __name__ == "__main__":
    unittest.main()
