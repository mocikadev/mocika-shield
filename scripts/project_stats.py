#!/usr/bin/env python3
"""采集 GitHub 项目聚合数据并生成静态统计页面。"""

from __future__ import annotations

import argparse
import html
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
PLATFORM_COLORS = {
    "Windows": "#2563eb",
    "macOS": "#8b5cf6",
    "Linux": "#f59e0b",
    "其他": "#64748b",
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
            rows = json.load(response).get("data", [])
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

    latest = trend[-1] if trend else {}
    return {
        "available": True,
        "active_devices": latest.get("active_devices"),
        "app_starts": sum(int(row.get("app_starts") or 0) for row in trend),
        "protect_successes": sum(int(row.get("protect_successes") or 0) for row in trend),
        "protect_failures": sum(int(row.get("protect_failures") or 0) for row in trend),
        "trend": trend,
    }


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


def svg_document(title: str, description: str, body: str, width: int, height: int) -> str:
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" role="img" aria-labelledby="title desc">
<title id="title">{html.escape(title)}</title>
<desc id="desc">{html.escape(description)}</desc>
<rect width="100%" height="100%" rx="12" fill="#ffffff"/>
<style>
text {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; fill: #172033; }}
.muted {{ fill: #667085; }}
.grid {{ stroke: #e4e7ec; stroke-width: 1; }}
@media (prefers-color-scheme: dark) {{
  rect:first-of-type {{ fill: #0d1117; }}
  text {{ fill: #e6edf3; }}
  .muted {{ fill: #9da7b3; }}
  .grid {{ stroke: #30363d; }}
}}
</style>
{body}
</svg>
'''


def platform_chart(snapshot: dict[str, Any]) -> str:
    values = snapshot["totals"]["platform_downloads"]
    maximum = max(max(values.values()), 1)
    rows = []
    for index, platform in enumerate(PLATFORMS):
        value = values[platform]
        y = 76 + index * 54
        bar_width = round(500 * value / maximum)
        rows.append(
            f'<text x="28" y="{y + 18}" font-size="15">{platform}</text>'
            f'<rect x="112" y="{y}" width="500" height="24" rx="5" fill="#e4e7ec" opacity="0.45"/>'
            f'<rect x="112" y="{y}" width="{bar_width}" height="24" rx="5" fill="{PLATFORM_COLORS[platform]}"/>'
            f'<text x="{min(626, 124 + bar_width)}" y="{y + 18}" font-size="14">{value}</text>'
        )
    body = (
        '<text x="28" y="34" font-size="20" font-weight="500">各系统平台累计下载</text>'
        f'<text x="28" y="57" class="muted" font-size="13">截至 {snapshot["date"]}，下载次数不等于独立用户数</text>'
        + "".join(rows)
    )
    return svg_document("各系统平台累计下载", "按 Windows、macOS、Linux 和其他平台归类的累计发布包下载次数", body, 680, 310)


def version_sort_key(tag: str) -> tuple[Any, ...]:
    normalized = tag.lstrip("vV")
    version, _, prerelease = normalized.partition("-")
    numbers = tuple(int(part) if part.isdigit() else 0 for part in version.split("."))
    padded = (numbers + (0, 0, 0))[:3]
    return (*padded, 1 if not prerelease else 0, prerelease)


def summarize_versions(snapshot: dict[str, Any]) -> list[dict[str, Any]]:
    versions: dict[str, dict[str, Any]] = {}
    for asset in snapshot["release_assets"]:
        tag = asset["tag"] or "未标记版本"
        summary = versions.setdefault(
            tag,
            {
                "tag": tag,
                "prerelease": "-" in tag.lstrip("vV"),
                "total": 0,
                "platforms": {platform: 0 for platform in PLATFORMS},
                "assets": [],
            },
        )
        summary["total"] += asset["download_count"]
        summary["platforms"][asset["platform"]] += asset["download_count"]
        summary["assets"].append(asset)
    return sorted(versions.values(), key=lambda item: version_sort_key(item["tag"]), reverse=True)


def version_chart(snapshot: dict[str, Any]) -> str:
    versions = summarize_versions(snapshot)
    maximum = max((item["total"] for item in versions), default=1)
    row_height = 42
    height = max(250, 108 + len(versions) * row_height)
    parts = [
        '<text x="24" y="32" font-size="20" font-weight="500">各版本累计下载</text>',
        '<text x="24" y="54" class="muted" font-size="13">正式版与预发布版分别统计，横条按系统平台拆分</text>',
    ]
    for index, item in enumerate(versions):
        y = 76 + index * row_height
        parts.append(f'<text x="24" y="{y + 17}" font-size="13">{html.escape(item["tag"])}</text>')
        parts.append(f'<rect x="142" y="{y}" width="500" height="22" rx="4" fill="#e4e7ec" opacity="0.45"/>')
        cursor = 142.0
        for platform in PLATFORMS:
            value = item["platforms"][platform]
            segment = 500 * value / max(maximum, 1)
            if segment <= 0:
                continue
            parts.append(
                f'<rect x="{cursor:.1f}" y="{y}" width="{segment:.1f}" height="22" '
                f'fill="{PLATFORM_COLORS[platform]}"><title>{platform}：{value}</title></rect>'
            )
            cursor += segment
        parts.append(f'<text x="{min(690, cursor + 8):.1f}" y="{y + 17}" font-size="13">{item["total"]}</text>')
    legend_y = height - 18
    legend_x = 24
    for platform in PLATFORMS:
        parts.append(f'<rect x="{legend_x}" y="{legend_y - 10}" width="10" height="10" rx="2" fill="{PLATFORM_COLORS[platform]}"/>')
        parts.append(f'<text x="{legend_x + 16}" y="{legend_y}" font-size="11">{platform}</text>')
        legend_x += 108
    return svg_document(
        "各版本累计下载",
        "每个正式版本和预发布版本按系统平台拆分的累计下载次数",
        "".join(parts),
        760,
        height,
    )


def line_chart(
    title: str,
    subtitle: str,
    snapshots: list[dict[str, Any]],
    series: list[tuple[str, str, str]],
    value_getter,
) -> str:
    width, height = 760, 340
    left, top, plot_width, plot_height = 58, 72, 662, 210
    all_values = [
        value
        for item in snapshots
        for _, key, _ in series
        if (value := value_getter(item, key)) is not None
    ]
    maximum = max(max(all_values, default=0), 1)
    parts = [
        f'<text x="24" y="32" font-size="20" font-weight="500">{html.escape(title)}</text>',
        f'<text x="24" y="54" class="muted" font-size="13">{html.escape(subtitle)}</text>',
    ]
    for step in range(5):
        y = top + plot_height * step / 4
        value = round(maximum * (4 - step) / 4)
        parts.append(f'<line class="grid" x1="{left}" y1="{y:.1f}" x2="{left + plot_width}" y2="{y:.1f}"/>')
        parts.append(f'<text x="50" y="{y + 4:.1f}" text-anchor="end" class="muted" font-size="11">{value}</text>')
    count = max(len(snapshots) - 1, 1)
    for label, key, color in series:
        points = []
        for index, item in enumerate(snapshots):
            x = left + plot_width * index / count
            value = value_getter(item, key)
            if value is None:
                continue
            y = top + plot_height - plot_height * value / maximum
            points.append((x, y, value))
        if points:
            point_text = " ".join(f"{x:.1f},{y:.1f}" for x, y, _ in points)
            parts.append(f'<polyline points="{point_text}" fill="none" stroke="{color}" stroke-width="3" stroke-linejoin="round" stroke-linecap="round"/>')
            for x, y, value in points:
                parts.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="3.5" fill="{color}"><title>{label}：{value}</title></circle>')
    if snapshots:
        parts.append(f'<text x="{left}" y="306" class="muted" font-size="11">{snapshots[0]["date"]}</text>')
        parts.append(f'<text x="{left + plot_width}" y="306" text-anchor="end" class="muted" font-size="11">{snapshots[-1]["date"]}</text>')
    legend_x = 24
    for label, _, color in series:
        parts.append(f'<circle cx="{legend_x + 5}" cy="326" r="5" fill="{color}"/>')
        parts.append(f'<text x="{legend_x + 16}" y="330" font-size="12">{html.escape(label)}</text>')
        legend_x += 112
    return svg_document(title, subtitle, "".join(parts), width, height)


def render_index(repository: str, snapshots: list[dict[str, Any]]) -> str:
    latest = snapshots[-1]
    totals = latest["totals"]
    traffic = latest["traffic"]
    usage = latest.get("usage", {})
    usage_available = usage.get("available", bool(usage))
    repo = latest["repository"]
    cards: list[tuple[str, Any]] = [
        ("发布包累计下载", totals["downloads"]),
        ("近 14 天独立访客", traffic["unique_visitors"]),
        ("近 14 天独立克隆", traffic["unique_cloners"]),
        ("星标", repo["stars"]),
        ("最近有数据日活跃设备", usage.get("active_devices")),
        ("近 14 天加固成功", usage.get("protect_successes")),
    ]
    card_html = "".join(
        f'<article class="metric"><span>{html.escape(label)}</span><strong>{value if value is not None else "—"}</strong></article>'
        for label, value in cards
    )
    usage_notice = (
        ""
        if usage_available
        else '<p class="status-note">匿名使用统计接口本次采集不可用，下载与仓库统计仍正常更新。</p>'
    )
    versions = summarize_versions(latest)
    version_rows = "".join(
        "<tr>"
        f'<td><code>{html.escape(item["tag"])}</code></td>'
        f'<td>{"预发布版" if item["prerelease"] else "正式版"}</td>'
        f'<td>{item["platforms"]["Windows"]}</td>'
        f'<td>{item["platforms"]["macOS"]}</td>'
        f'<td>{item["platforms"]["Linux"]}</td>'
        f'<td><strong>{item["total"]}</strong></td>'
        "</tr>"
        for item in versions
    )
    asset_rows = "".join(
        "<tr>"
        f'<td><code>{html.escape(asset["tag"])}</code></td>'
        f'<td>{html.escape(asset["name"])}</td>'
        f'<td>{html.escape(asset["platform"])}</td>'
        f'<td>{asset["download_count"]}</td>'
        "</tr>"
        for item in versions
        for asset in sorted(item["assets"], key=lambda value: value["name"])
    )
    return f'''<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Mocika Shield 项目统计</title>
  <style>
    :root {{ color-scheme: light dark; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; background: #f6f8fb; color: #172033; }}
    * {{ box-sizing: border-box; }}
    body {{ max-width: 1160px; margin: 0 auto; padding: 24px 20px 72px; background: #f6f8fb; color: #172033; }}
    a {{ color: #2563eb; text-decoration: none; }}
    a:hover {{ text-decoration: underline; }}
    .topbar {{ display: flex; align-items: center; justify-content: space-between; gap: 20px; padding: 8px 0 30px; }}
    .brand {{ display: flex; align-items: center; gap: 11px; color: #172033; font-weight: 650; letter-spacing: -.02em; }}
    .brand-mark {{ display: grid; place-items: center; width: 34px; height: 34px; border-radius: 10px; background: linear-gradient(135deg, #2563eb, #7c3aed); color: #fff; font-weight: 700; }}
    nav {{ display: flex; flex-wrap: wrap; gap: 18px; font-size: 13px; }}
    .hero {{ border-radius: 22px; padding: 36px clamp(24px, 5vw, 56px); background: radial-gradient(circle at 90% 10%, rgba(124, 58, 237, .32), transparent 35%), linear-gradient(135deg, #111827, #1e3a8a); color: #f8fafc; box-shadow: 0 18px 45px rgba(30, 58, 138, .2); }}
    .eyebrow {{ display: inline-flex; align-items: center; gap: 8px; margin: 0 0 15px; color: #bfdbfe; font-size: 12px; letter-spacing: .08em; text-transform: uppercase; }}
    .pulse {{ width: 7px; height: 7px; border-radius: 50%; background: #4ade80; box-shadow: 0 0 0 5px rgba(74, 222, 128, .16); }}
    h1 {{ margin: 0; font-size: clamp(28px, 4vw, 42px); line-height: 1.08; letter-spacing: -.045em; }}
    .hero-copy {{ max-width: 650px; margin: 16px 0 0; color: #dbeafe; font-size: 16px; line-height: 1.7; }}
    .hero-meta {{ display: flex; flex-wrap: wrap; gap: 16px; margin: 25px 0 0; color: #bfdbfe; font-size: 12px; }}
    .metrics {{ display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 14px; margin: 24px 0; }}
    .metric {{ min-height: 126px; border: 1px solid #e2e8f0; border-radius: 16px; padding: 18px; background: rgba(255, 255, 255, .82); box-shadow: 0 5px 18px rgba(15, 23, 42, .04); }}
    .metric span {{ display: block; color: #64748b; font-size: 13px; }}
    .metric strong {{ display: block; margin-top: 11px; font-size: 31px; letter-spacing: -.04em; }}
    .metric small {{ display: block; margin-top: 8px; color: #94a3b8; font-size: 11px; line-height: 1.4; }}
    .section {{ margin-top: 34px; }}
    .section-heading {{ display: flex; align-items: end; justify-content: space-between; gap: 18px; margin-bottom: 14px; }}
    h2 {{ margin: 0; font-size: 22px; letter-spacing: -.025em; }}
    .section-heading p {{ margin: 0; color: #64748b; font-size: 13px; }}
    .charts {{ display: grid; gap: 16px; }}
    .chart-card {{ overflow: hidden; border: 1px solid #e2e8f0; border-radius: 16px; background: #fff; box-shadow: 0 5px 18px rgba(15, 23, 42, .04); }}
    .charts img {{ display: block; width: 100%; height: auto; }}
    .table-card {{ overflow: hidden; border: 1px solid #e2e8f0; border-radius: 16px; background: #fff; box-shadow: 0 5px 18px rgba(15, 23, 42, .04); }}
    .table-wrap {{ overflow-x: auto; }}
    table {{ width: 100%; border-collapse: collapse; font-size: 14px; }}
    th, td {{ padding: 13px 16px; border-bottom: 1px solid #eef2f7; text-align: left; white-space: nowrap; }}
    th {{ color: #64748b; background: #f8fafc; font-size: 12px; font-weight: 600; }}
    tr:last-child td {{ border-bottom: 0; }}
    tbody tr:hover {{ background: #f8fafc; }}
    details {{ margin-top: 13px; }}
    summary {{ padding: 15px 16px; cursor: pointer; color: #475569; font-size: 13px; }}
    .footnote {{ margin: 14px 0 0; color: #94a3b8; font-size: 12px; line-height: 1.7; }}
    .status-note {{ margin: -8px 0 24px; border: 1px solid #f59e0b; border-radius: 12px; padding: 11px 14px; background: #fffbeb; color: #92400e; font-size: 13px; }}
    footer {{ display: flex; flex-wrap: wrap; justify-content: space-between; gap: 12px; margin-top: 46px; padding-top: 20px; border-top: 1px solid #e2e8f0; color: #94a3b8; font-size: 12px; }}
    @media (max-width: 760px) {{
      body {{ padding: 15px 14px 50px; }}
      .topbar {{ align-items: flex-start; flex-direction: column; padding-bottom: 20px; }}
      nav {{ gap: 12px; }}
      .hero {{ border-radius: 18px; padding: 28px 22px; }}
      .metrics {{ grid-template-columns: repeat(2, minmax(0, 1fr)); }}
      .section-heading {{ align-items: flex-start; flex-direction: column; gap: 6px; }}
    }}
    @media (prefers-color-scheme: dark) {{
      :root, body {{ background: #0b1220; color: #e5e7eb; }}
      .brand {{ color: #e5e7eb; }}
      .metric, .chart-card, .table-card {{ border-color: #263247; background: #111a2b; box-shadow: none; }}
      .metric span, .section-heading p, .footnote, footer {{ color: #94a3b8; }}
      .status-note {{ border-color: #92400e; background: #2b1d0e; color: #fcd34d; }}
      th {{ color: #94a3b8; background: #162033; }}
      th, td {{ border-color: #263247; }}
      tbody tr:hover {{ background: #162033; }}
      footer {{ border-color: #263247; }}
    }}
  </style>
</head>
<body>
  <header class="topbar">
    <a class="brand" href="https://github.com/{html.escape(repository)}"><span class="brand-mark" aria-hidden="true">M</span><span>Mocika Shield</span></a>
    <nav aria-label="页面导航"><a href="#overview">概览</a><a href="#charts">趋势图</a><a href="#versions">版本明细</a><a href="https://github.com/{html.escape(repository)}/releases">发布页</a><a href="https://github.com/{html.escape(repository)}">GitHub</a></nav>
  </header>
  <main>
    <section class="hero" id="overview">
      <p class="eyebrow"><span class="pulse" aria-hidden="true"></span>每日自动更新的公开统计</p>
      <h1>Mocika Shield 关注与使用趋势</h1>
      <p class="hero-copy"><strong>Mocika Shield 是一个 Android APK 加固工具</strong>，提供 DEX 加密、壳保护和运行时反调试能力。这里通过 GitHub 公开指标和用户允许上报的匿名每日汇总，观察项目关注与实际使用趋势；展示的是聚合趋势，不是精确的独立用户数。</p>
      <div class="hero-meta"><span>最后采集：{html.escape(latest["collected_at"])}</span><span>数据窗口：最近 14 天</span></div>
    </section>
    <section class="metrics" aria-label="统计摘要">{card_html}</section>{usage_notice}
    <section class="section" id="charts">
      <div class="section-heading"><h2>趋势与分布</h2><p>按平台、版本和时间观察下载变化</p></div>
      <div class="charts" aria-label="趋势图">
        <div class="chart-card"><img src="charts/platform-downloads.svg" alt="各系统平台累计下载量"></div>
        <div class="chart-card"><img src="charts/version-downloads.svg" alt="各版本按系统平台拆分的累计下载量"></div>
        <div class="chart-card"><img src="charts/download-trend.svg" alt="发布包累计下载趋势"></div>
        <div class="chart-card"><img src="charts/traffic-trend.svg" alt="仓库独立访客与独立克隆趋势"></div>
        <div class="chart-card"><img src="charts/usage-trend.svg" alt="应用启动与加固成功趋势"></div>
      </div>
    </section>
    <section class="section" id="versions">
      <div class="section-heading"><h2>版本下载明细</h2><p>正式版与预发布版分开统计</p></div>
      <div class="table-card">
        <div class="table-wrap">
          <table>
            <thead><tr><th>版本</th><th>类型</th><th>Windows</th><th>macOS</th><th>Linux</th><th>合计</th></tr></thead>
            <tbody>{version_rows}</tbody>
          </table>
        </div>
        <details>
          <summary>查看具体发布产物</summary>
          <div class="table-wrap">
            <table>
              <thead><tr><th>版本</th><th>产物</th><th>平台</th><th>下载</th></tr></thead>
              <tbody>{asset_rows}</tbody>
            </table>
          </div>
        </details>
      </div>
      <p class="footnote">下载次数包含重复下载和不同格式下载，不等于独立用户数。访客和克隆在 GitHub 权限不可用时显示为“—”，不会被当作零。</p>
    </section>
  </main>
  <footer><span>数据来自 GitHub 聚合指标，以及用户允许上报的匿名客户端每日汇总；不包含 APK、路径、证书或密码。</span><span><a href="https://github.com/{html.escape(repository)}/blob/main/docs/ops/project-statistics.md">查看统计口径</a></span></footer>
</body>
</html>
'''


def write_outputs(history: dict[str, Any], output_dir: Path) -> None:
    snapshots = history["snapshots"]
    if not snapshots:
        raise RuntimeError("没有可生成的统计快照")
    charts = output_dir / "charts"
    data_dir = output_dir / "data"
    charts.mkdir(parents=True, exist_ok=True)
    data_dir.mkdir(parents=True, exist_ok=True)
    (data_dir / "history.json").write_text(
        json.dumps(history, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    (charts / "platform-downloads.svg").write_text(platform_chart(snapshots[-1]), encoding="utf-8")
    (charts / "version-downloads.svg").write_text(version_chart(snapshots[-1]), encoding="utf-8")
    (charts / "download-trend.svg").write_text(
        line_chart(
            "发布包累计下载趋势",
            "每日快照中的全部安装包与命令行包累计下载次数",
            snapshots,
            [("累计下载", "downloads", "#2563eb")],
            lambda item, key: item["totals"][key],
        ),
        encoding="utf-8",
    )
    (charts / "traffic-trend.svg").write_text(
        line_chart(
            "仓库近期关注趋势",
            "每个快照记录 GitHub 最近 14 天窗口内的独立数量",
            snapshots,
            [("独立访客", "unique_visitors", "#2563eb"), ("独立克隆", "unique_cloners", "#f59e0b")],
            lambda item, key: item["traffic"][key],
        ),
        encoding="utf-8",
    )
    usage_points = snapshots[-1].get("usage", {}).get("trend", [])
    (charts / "usage-trend.svg").write_text(
        line_chart(
            "应用实际使用趋势",
            "匿名客户端每日汇总",
            usage_points,
            [("启动次数", "app_starts", "#2563eb"), ("加固成功", "protect_successes", "#16a34a")],
            lambda item, key: item.get(key) or 0,
        ),
        encoding="utf-8",
    )
    (output_dir / "index.html").write_text(
        render_index(history["repository"], snapshots), encoding="utf-8"
    )
    (output_dir / ".nojekyll").write_text("", encoding="utf-8")


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
        "https://mocika-shield-stats-api.xuechao-suo.workers.dev/stats/trend?days=14",
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
