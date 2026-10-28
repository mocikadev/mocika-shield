#!/usr/bin/env python3
"""发布前检查所有内置资源的版本，不修改资源内容。"""

import argparse
import json
from pathlib import Path
import sys
import zipfile


def check_version(path: Path, expected: str) -> None:
    with zipfile.ZipFile(path) as archive:
        entries = [item for item in archive.infolist() if item.filename == "metadata.json"]
        if len(entries) != 1 or entries[0].file_size > 65536:
            raise ValueError(f"{path.name} 的元数据缺失、重复或过大")
        metadata = json.loads(archive.read(entries[0]).decode("utf-8-sig"))
    actual = metadata.get("version") if isinstance(metadata, dict) else None
    if actual != expected:
        raise ValueError(f"{path.name} 版本不一致：期望 {expected}，实际 {actual!r}")


def main() -> int:
    # Windows CI 的重定向管道可能使用 cp1252；只固定本脚本输出，不修改系统设置。
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", help="本次发布版本（含预发布后缀）")
    parser.add_argument("resources", nargs="+", type=Path, help="待打包的资源 ZIP")
    args = parser.parse_args()
    try:
        for path in args.resources:
            check_version(path, args.version)
    except (OSError, ValueError, zipfile.BadZipFile, RuntimeError) as error:
        print(f"资源版本检查失败：{error}", file=sys.stderr)
        return 1
    print(f"资源版本检查通过：{args.version}，共 {len(args.resources)} 套")
    return 0


if __name__ == "__main__":
    sys.exit(main())
