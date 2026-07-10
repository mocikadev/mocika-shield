import json
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

from scripts.project_stats import (
    build_snapshot,
    classify_platform,
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


if __name__ == "__main__":
    unittest.main()
