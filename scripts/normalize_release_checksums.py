#!/usr/bin/env python3
"""将本地发布目录中的校验和转换为 GitHub Release 扁平文件名。"""

from __future__ import annotations

import argparse
import re
from pathlib import Path, PurePosixPath


CHECKSUM_LINE = re.compile(r"^([0-9a-fA-F]{64})  [*]?(.+)$")


def normalize_checksum_text(content: str) -> str:
    """去掉构建目录前缀，并拒绝无效行或重复的最终文件名。"""
    normalized: list[tuple[str, str]] = []
    names: set[str] = set()

    for line_number, raw_line in enumerate(content.splitlines(), start=1):
        line = raw_line.strip()
        if not line:
            continue

        match = CHECKSUM_LINE.fullmatch(line)
        if match is None:
            raise ValueError(f"第 {line_number} 行不是有效的 SHA-256 校验记录")

        digest, source_name = match.groups()
        file_name = PurePosixPath(source_name.replace("\\", "/")).name
        if not file_name or file_name in {".", ".."}:
            raise ValueError(f"第 {line_number} 行缺少有效文件名")
        if file_name in names:
            raise ValueError(f"Release 中存在重复文件名：{file_name}")

        names.add(file_name)
        normalized.append((file_name, digest.lower()))

    if not normalized:
        raise ValueError("校验和文件为空")

    normalized.sort(key=lambda item: item[0])
    return "".join(f"{digest}  {file_name}\n" for file_name, digest in normalized)


def normalize_checksum_file(path: Path) -> None:
    content = path.read_text(encoding="utf-8-sig")
    with path.open("w", encoding="utf-8", newline="\n") as output:
        output.write(normalize_checksum_text(content))


def main() -> None:
    parser = argparse.ArgumentParser(description="规范化 GitHub Release 校验和文件")
    parser.add_argument("files", nargs="+", type=Path, help="待规范化的校验和文件")
    args = parser.parse_args()

    for path in args.files:
        normalize_checksum_file(path)


if __name__ == "__main__":
    main()
