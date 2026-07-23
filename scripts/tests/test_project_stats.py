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
    load_history,
    merge_snapshot,
    summarize_versions,
    version_sort_key,
    write_outputs,
)


class ProjectStatsTests(unittest.TestCase):
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
            self.assertTrue((root / "index.html").exists())
            self.assertTrue((root / "charts/platform-downloads.svg").exists())
            self.assertTrue((root / "charts/version-downloads.svg").exists())
            self.assertTrue((root / "data/history.json").exists())
            page = (root / "index.html").read_text(encoding="utf-8")
            self.assertIn("匿名使用统计接口本次采集不可用", page)
            self.assertIn('data-live=\'downloads\'', page)
            self.assertIn("当前指标近实时", page)
            self.assertNotIn("不包含客户端遥测", page)

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

    def test_version_summary_separates_stable_and_prerelease(self):
        snapshot = {
            "release_assets": [
                {"tag": "v1.2.0", "name": "stable.exe", "platform": "Windows", "download_count": 8},
                {"tag": "v1.2.0-rc.1", "name": "preview.dmg", "platform": "macOS", "download_count": 3},
            ]
        }
        versions = summarize_versions(snapshot)
        self.assertEqual([item["tag"] for item in versions], ["v1.2.0", "v1.2.0-rc.1"])
        self.assertFalse(versions[0]["prerelease"])
        self.assertTrue(versions[1]["prerelease"])
        self.assertGreater(version_sort_key("v1.2.0"), version_sort_key("v1.2.0-rc.1"))

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
            chart = (root / "charts/usage-trend.svg").read_text(encoding="utf-8")
            self.assertIn("2026-07-10", chart)

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


if __name__ == "__main__":
    unittest.main()
