#!/usr/bin/env python3
"""采集 GitHub 项目聚合数据并保存维护历史。"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request
from datetime import date, datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Optional


SCHEMA_VERSION = 1
PLATFORMS = ("Windows", "macOS", "Linux", "其他")
USAGE_FIELDS = {
    "usage_date", "active_devices", "app_starts", "protect_starts", "protect_successes",
    "protect_failures", "sign_successes", "sign_failures",
}
def github_get(repository: str, endpoint: str, token: str) -> Any:
    suffix = f"/{endpoint}" if endpoint else ""
    request = urllib.request.Request(
        f"https://api.github.com/repos/{repository}{suffix}",
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}",
            "User-Agent": "mocika-shield-project-stats",
            "X-GitHub-Api-Version": "2022-11-28",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        detail = error.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"GitHub API 请求失败：{endpoint}，状态码 {error.code}，{detail}") from error


def collect_usage_stats(stats_url: str) -> dict[str, Any]:
    request = urllib.request.Request(
        stats_url,
        headers={
            "Accept": "application/json",
            "User-Agent": "mocika-shield-project-stats",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=15) as response:
            payload = json.load(response)
        rows = payload.get("data", [])
        if not isinstance(rows, list):
            raise ValueError("data 字段不是数组")
        trend: list[dict[str, Any]] = []
        for row in rows:
            if not isinstance(row, dict):
                raise ValueError("data 数组元素不是对象")
            date = row.get("date") or row.get("usage_date") or row.get("day")
            if not date:
                raise ValueError("匿名使用统计缺少日期字段")
            trend.append({**{key: row[key] for key in USAGE_FIELDS if key in row}, "date": str(date)})
        trend.sort(key=lambda row: row["date"])
    except (OSError, ValueError, TypeError) as error:
        print(f"警告：匿名使用统计接口不可用：{error}", file=sys.stderr)
        return {"available": False, "trend": []}

    # 当日数据仍在持续累计，静态快照和历史图只使用已经结束的 UTC 日期。
    today = datetime.now(timezone.utc).date().isoformat()
    complete_trend = [row for row in trend if row["date"] < today][-14:]
    latest = complete_trend[-1] if complete_trend else {}
    version_trend = normalize_usage_breakdown(payload.get("versions"), "版本统计", USAGE_FIELDS | {"app_version"})
    failure_breakdown = normalize_usage_breakdown(payload.get("failure_breakdown"), "失败阶段统计", {
        "usage_date", "app_version", "operation", "stage", "count",
    })
    complete_versions = [row for row in (version_trend or []) if row["date"] < today][-90:]
    complete_failures = [row for row in (failure_breakdown or []) if row["date"] < today][-90:]
    # 只读取公开聚合白名单；绝不抓取报告接口或保留混入响应的报告正文。
    reason_rows = normalize_public_failure_rows(payload.get("failure_reason_breakdown"), {
        "usage_date", "app_version", "flow", "operation", "stage", "code", "classifier_version", "count",
    }, today)
    coverage_rows = normalize_public_failure_rows(payload.get("failure_classifier_coverage"), {
        "usage_date", "app_version", "failure_classifier_version", "reporting_devices", "failure_count",
    }, today)
    try:
        activity_windows = normalize_activity_windows(payload.get("activity_windows"))
    except (ValueError, TypeError, KeyError):
        print("警告：活跃窗口数据无效，保留其他使用统计", file=sys.stderr)
        activity_windows = None
    return {
        "available": True,
        "active_devices_definition": "distinct_installation_per_utc_period"
        if payload.get("active_devices_definition") == "distinct_installation_per_utc_period" else None,
        "activity_windows_available": activity_windows is not None,
        "activity_windows": activity_windows,
        "active_devices": latest.get("active_devices"),
        "latest_complete_date": latest.get("date"),
        "app_starts": sum(int(row.get("app_starts") or 0) for row in complete_trend),
        "protect_successes": sum(int(row.get("protect_successes") or 0) for row in complete_trend),
        "protect_failures": sum(int(row.get("protect_failures") or 0) for row in complete_trend),
        "trend": complete_trend,
        "version_breakdown_available": version_trend is not None,
        "version_trend": complete_versions if version_trend is not None else [],
        "failure_breakdown_available": failure_breakdown is not None,
        "failure_breakdown": complete_failures if failure_breakdown is not None else [],
        "failure_reason_breakdown_available": reason_rows is not None,
        "failure_reason_breakdown": reason_rows or [],
        "failure_classifier_coverage": coverage_rows or [],
    }


def normalize_activity_windows(value: Any) -> Optional[list[dict[str, Any]]]:
    """窗口已在数据库跨日期去重；这里只保留公开聚合字段，不用日活相加替代。"""
    if value is None:
        return None
    if not isinstance(value, list):
        raise ValueError("活跃窗口字段不是数组")
    rows = []
    for row in value:
        if not isinstance(row, dict):
            raise ValueError("活跃窗口元素不是对象")
        days, devices = row.get("window_days"), row.get("active_devices")
        if days not in (7, 30) or type(devices) is not int or devices < 0:
            raise ValueError("活跃窗口计数无效")
        start, end = date.fromisoformat(row["start_date"]), date.fromisoformat(row["end_date"])
        if end - start != timedelta(days=days - 1):
            raise ValueError("活跃窗口日期无效")
        rows.append({"window_days": days, "start_date": start.isoformat(),
                     "end_date": end.isoformat(), "active_devices": devices})
    return rows


def normalize_public_failure_rows(value: Any, fields: set[str], today: str) -> Optional[list[dict[str, Any]]]:
    rows = normalize_usage_breakdown(value, "失败类别统计", fields)
    if rows is None:
        return None
    return [
        {key: row[key] for key in fields | {"date"} if key in row}
        for row in rows if row["date"] < today
    ]


def normalize_usage_breakdown(value: Any, label: str, fields: set[str]) -> Optional[list[dict[str, Any]]]:
    """新版维度缺失时明确不可用，不能用零值替代历史空白。"""
    if value is None:
        return None
    if not isinstance(value, list):
        raise ValueError(f"{label}字段不是数组")
    rows: list[dict[str, Any]] = []
    for row in value:
        if not isinstance(row, dict):
            raise ValueError(f"{label}数组元素不是对象")
        date = row.get("date") or row.get("usage_date") or row.get("day")
        if not date:
            raise ValueError(f"{label}缺少日期字段")
        rows.append({**{key: row[key] for key in fields if key in row}, "date": str(date)})
    rows.sort(key=lambda row: row["date"])
    return rows


def classify_platform(name: str) -> str:
    lowered = name.lower()
    if "windows" in lowered or lowered.endswith((".exe", ".msi")):
        return "Windows"
    if "macos" in lowered or lowered.endswith((".dmg", ".pkg")):
        return "macOS"
    if "linux" in lowered or lowered.endswith((".appimage", ".deb", ".rpm")):
        return "Linux"
    return "其他"


def is_download_asset(name: str) -> bool:
    lowered = name.lower()
    return not any(word in lowered for word in ("checksum", "checksums", "sha256"))


def normalize_releases(releases: list[dict[str, Any]]) -> list[dict[str, Any]]:
    assets: list[dict[str, Any]] = []
    for release in releases:
        if release.get("draft"):
            continue
        for asset in release.get("assets", []):
            name = str(asset.get("name", ""))
            if not name or not is_download_asset(name):
                continue
            assets.append(
                {
                    "tag": str(release.get("tag_name", "")),
                    "published_at": release.get("published_at"),
                    "name": name,
                    "download_count": int(asset.get("download_count", 0)),
                    "platform": classify_platform(name),
                }
            )
    assets.sort(key=lambda item: (item["tag"], item["name"]))
    return assets


def build_snapshot(payload: dict[str, Any], collected_at: datetime) -> dict[str, Any]:
    repository = payload["repository"]
    views = payload["views"]
    clones = payload["clones"]
    usage = payload.get("usage", {})
    assets = normalize_releases(payload["releases"])
    releases_available = payload.get("releases_available", True)
    published_versions = {
        str(release.get("tag_name", "")).removeprefix("v")
        for release in payload["releases"]
        if not release.get("draft") and release.get("published_at")
    }
    if isinstance(usage, dict) and "version_trend" in usage:
        usage = {**usage, "version_trend": [
            {**row, "release_version_match": row.get("app_version") in published_versions
             if releases_available else None}
            for row in usage["version_trend"]
        ]}
    platform_downloads = {platform: 0 for platform in PLATFORMS}
    for asset in assets:
        platform_downloads[asset["platform"]] += asset["download_count"]

    return {
        "date": collected_at.date().isoformat(),
        "collected_at": collected_at.isoformat().replace("+00:00", "Z"),
        "repository": {
            "available": repository.get("available", True),
            "stars": optional_int(repository.get("stargazers_count")),
            "forks": optional_int(repository.get("forks_count")),
            "open_issues": optional_int(repository.get("open_issues_count")),
        },
        "traffic": {
            "window_days": 14,
            "available": bool(views.get("available", True) and clones.get("available", True)),
            "views": optional_int(views.get("count")),
            "unique_visitors": optional_int(views.get("uniques")),
            "clones": optional_int(clones.get("count")),
            "unique_cloners": optional_int(clones.get("uniques")),
        },
        "usage": usage if isinstance(usage, dict) else {},
        "release_assets": assets,
        "release_assets_available": releases_available,
        "totals": {
            "downloads": sum(item["download_count"] for item in assets) if releases_available else None,
            "platform_downloads": platform_downloads if releases_available else None,
        },
    }


def optional_int(value: Any) -> Optional[int]:
    return int(value) if value is not None else None


def load_history(path: Path, repository: str) -> dict[str, Any]:
    if not path.exists():
        return {"schema_version": SCHEMA_VERSION, "repository": repository, "snapshots": []}
    data = json.loads(path.read_text(encoding="utf-8"))
    if data.get("schema_version") != SCHEMA_VERSION:
        raise RuntimeError("统计历史数据版本不受支持")
    if data.get("repository") != repository:
        raise RuntimeError("统计历史数据所属仓库不匹配")
    return data


def merge_snapshot(history: dict[str, Any], snapshot: dict[str, Any]) -> dict[str, Any]:
    snapshots = [item for item in history.get("snapshots", []) if item.get("date") != snapshot["date"]]
    snapshots.append(snapshot)
    snapshots.sort(key=lambda item: item["date"])
    history["snapshots"] = snapshots
    history["updated_at"] = snapshot["collected_at"]
    return history


def write_outputs(history: dict[str, Any], output_dir: Path) -> None:
    if not history["snapshots"]:
        raise RuntimeError("没有可保存的统计快照")
    data_dir = output_dir / "data"
    data_dir.mkdir(parents=True, exist_ok=True)
    (data_dir / "history.json").write_text(
        json.dumps(history, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )


def collect_payload(repository: str, token: str) -> dict[str, Any]:
    def optional_github(endpoint: str) -> Any:
        try:
            return github_get(repository, endpoint, token)
        except (RuntimeError, OSError, ValueError):
            print(f"警告：GitHub 指标不可用：{endpoint or 'repository'}", file=sys.stderr)
            return None

    releases = optional_github("releases?per_page=100")
    payload = {
        "repository": optional_github("") or {"available": False},
        "releases": releases if releases is not None else [],
        "releases_available": releases is not None,
        "views": optional_github("traffic/views?per=day") or {"available": False},
        "clones": optional_github("traffic/clones?per=day") or {"available": False},
    }
    stats_url = os.environ.get(
        "STATS_API_URL",
        "https://mocika-shield-stats-api.xuechao-suo.workers.dev/stats/trend?days=15",
    )
    payload["usage"] = collect_usage_stats(stats_url)
    return payload


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True, help="组织/仓库")
    parser.add_argument("--history-file", type=Path, required=True, help="已有历史数据文件")
    parser.add_argument("--output-dir", type=Path, required=True, help="生成结果目录")
    parser.add_argument("--fixture", type=Path, help="测试用固定 API 数据")
    parser.add_argument("--collected-at", help="固定采集时间，ISO 8601 格式")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if args.fixture:
        payload = json.loads(args.fixture.read_text(encoding="utf-8"))
    else:
        token = os.environ.get("GITHUB_TOKEN", "")
        if not token:
            raise RuntimeError("缺少 GITHUB_TOKEN")
        payload = collect_payload(args.repository, token)
    collected_at = (
        datetime.fromisoformat(args.collected_at.replace("Z", "+00:00"))
        if args.collected_at
        else datetime.now(timezone.utc)
    )
    if collected_at.tzinfo is None:
        collected_at = collected_at.replace(tzinfo=timezone.utc)
    history = load_history(args.history_file, args.repository)
    snapshot = build_snapshot(payload, collected_at.astimezone(timezone.utc))
    write_outputs(merge_snapshot(history, snapshot), args.output_dir)


if __name__ == "__main__":
    main()
