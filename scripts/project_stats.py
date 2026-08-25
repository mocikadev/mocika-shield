#!/usr/bin/env python3
"""采集 GitHub 项目聚合数据并保存维护历史。"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Optional


SCHEMA_VERSION = 1
PLATFORMS = ("Windows", "macOS", "Linux", "其他")
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
            trend.append({**row, "date": str(date)})
        trend.sort(key=lambda row: row["date"])
    except (OSError, ValueError, TypeError) as error:
        print(f"警告：匿名使用统计接口不可用：{error}", file=sys.stderr)
        return {"available": False, "trend": []}

    # 当日数据仍在持续累计，静态快照和历史图只使用已经结束的 UTC 日期。
    today = datetime.now(timezone.utc).date().isoformat()
    complete_trend = [row for row in trend if row["date"] < today][-14:]
    latest = complete_trend[-1] if complete_trend else {}
    version_trend = normalize_usage_breakdown(payload.get("versions"), "版本统计")
    failure_breakdown = normalize_usage_breakdown(payload.get("failure_breakdown"), "失败阶段统计")
    complete_versions = [row for row in (version_trend or []) if row["date"] < today][-90:]
    complete_failures = [row for row in (failure_breakdown or []) if row["date"] < today][-90:]
    return {
        "available": True,
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
    }


def normalize_usage_breakdown(value: Any, label: str) -> Optional[list[dict[str, Any]]]:
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
        rows.append({**row, "date": str(date)})
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
    platform_downloads = {platform: 0 for platform in PLATFORMS}
    for asset in assets:
        platform_downloads[asset["platform"]] += asset["download_count"]

    return {
        "date": collected_at.date().isoformat(),
        "collected_at": collected_at.isoformat().replace("+00:00", "Z"),
        "repository": {
            "stars": int(repository.get("stargazers_count", 0)),
            "forks": int(repository.get("forks_count", 0)),
            "open_issues": int(repository.get("open_issues_count", 0)),
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
        "totals": {
            "downloads": sum(item["download_count"] for item in assets),
            "platform_downloads": platform_downloads,
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
    def optional_traffic(endpoint: str) -> dict[str, Any]:
        try:
            result = github_get(repository, endpoint, token)
            result["available"] = True
            return result
        except RuntimeError as error:
            if "状态码 403" not in str(error):
                raise
            return {"available": False, "count": None, "uniques": None}

    payload = {
        "repository": github_get(repository, "", token),
        "releases": github_get(repository, "releases?per_page=100", token),
        "views": optional_traffic("traffic/views?per=day"),
        "clones": optional_traffic("traffic/clones?per=day"),
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
