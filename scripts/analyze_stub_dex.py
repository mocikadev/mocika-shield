#!/usr/bin/env python3
"""分析 Stub DEX 的稳定结构指标，并可执行体积膨胀守门。"""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
import sys
import zipfile
from dataclasses import asdict, dataclass
from pathlib import Path


DEX_HEADER_SIZE = 112
DEX_MAGIC_PREFIX = b"dex\n"


@dataclass(frozen=True)
class DexMetrics:
    source: str
    sha256: str
    bytes: int
    declared_bytes: int
    classes: int
    methods: int
    fields: int
    strings: int
    types: int
    prototypes: int
    class_descriptors: list[str]
    diagnostic_strings: list[str]


def _read_u32(data: bytes, offset: int) -> int:
    if offset < 0 or offset + 4 > len(data):
        raise ValueError(f"DEX 偏移越界：{offset}")
    return struct.unpack_from("<I", data, offset)[0]


def _read_uleb128(data: bytes, offset: int) -> tuple[int, int]:
    value = 0
    for index in range(5):
        if offset >= len(data):
            raise ValueError("DEX ULEB128 数据不完整")
        current = data[offset]
        offset += 1
        value |= (current & 0x7F) << (index * 7)
        if current & 0x80 == 0:
            return value, offset
    raise ValueError("DEX ULEB128 长度超过 5 字节")


def _read_string(data: bytes, offset: int) -> str:
    _, start = _read_uleb128(data, offset)
    end = data.find(b"\0", start)
    if end < 0:
        raise ValueError("DEX 字符串缺少结束标记")
    return data[start:end].decode("utf-8", errors="replace")


def _read_table(data: bytes, size: int, offset: int, item_size: int) -> list[int]:
    if size < 0 or offset < 0 or offset + size * item_size > len(data):
        raise ValueError("DEX 索引表越界")
    return [_read_u32(data, offset + index * item_size) for index in range(size)]


def _is_diagnostic_string(value: str) -> bool:
    descriptor = value.lstrip("[")
    if len(value) < 4 or descriptor.startswith("L") and descriptor.endswith(";"):
        return False
    has_cjk = any("\u4e00" <= char <= "\u9fff" for char in value)
    has_separator = any(char in value for char in (" ", ":", "=", "/"))
    return has_cjk or has_separator


def analyze_dex(data: bytes, source: str) -> DexMetrics:
    if len(data) < DEX_HEADER_SIZE or not data.startswith(DEX_MAGIC_PREFIX):
        raise ValueError("输入不是有效的 DEX 文件")
    if _read_u32(data, 36) != DEX_HEADER_SIZE:
        raise ValueError("DEX 头长度不受支持")

    declared_bytes = _read_u32(data, 32)
    if declared_bytes < DEX_HEADER_SIZE or declared_bytes > len(data):
        raise ValueError("DEX 声明长度非法")

    string_count = _read_u32(data, 56)
    string_offsets = _read_table(data, string_count, _read_u32(data, 60), 4)
    strings = [_read_string(data, offset) for offset in string_offsets]

    type_count = _read_u32(data, 64)
    type_string_indexes = _read_table(data, type_count, _read_u32(data, 68), 4)
    if any(index >= string_count for index in type_string_indexes):
        raise ValueError("DEX 类型字符串索引越界")

    class_count = _read_u32(data, 96)
    class_offset = _read_u32(data, 100)
    if class_offset + class_count * 32 > declared_bytes:
        raise ValueError("DEX 类定义表越界")
    class_type_indexes = [
        _read_u32(data, class_offset + index * 32) for index in range(class_count)
    ]
    if any(index >= type_count for index in class_type_indexes):
        raise ValueError("DEX 类类型索引越界")

    descriptors = sorted(
        strings[type_string_indexes[index]] for index in class_type_indexes
    )
    diagnostics = sorted({value for value in strings if _is_diagnostic_string(value)})
    return DexMetrics(
        source=source,
        sha256=hashlib.sha256(data[:declared_bytes]).hexdigest(),
        bytes=len(data),
        declared_bytes=declared_bytes,
        classes=class_count,
        methods=_read_u32(data, 88),
        fields=_read_u32(data, 80),
        strings=string_count,
        types=type_count,
        prototypes=_read_u32(data, 72),
        class_descriptors=descriptors,
        diagnostic_strings=diagnostics,
    )


def load_dex(path: Path) -> tuple[bytes, str]:
    if path.suffix.lower() != ".zip":
        return path.read_bytes(), str(path)
    with zipfile.ZipFile(path) as archive:
        try:
            return archive.read("stub-classes.dex"), f"{path}!stub-classes.dex"
        except KeyError as error:
            raise ValueError("资源 ZIP 缺少 stub-classes.dex") from error


def _check_limit(name: str, actual: int, maximum: int | None) -> str | None:
    if maximum is not None and actual > maximum:
        return f"{name} 超过上限：{actual} > {maximum}"
    return None


def check_limits(metrics: DexMetrics, args: argparse.Namespace) -> list[str]:
    checks = (
        ("DEX 字节数", metrics.bytes, args.max_bytes),
        ("类数量", metrics.classes, args.max_classes),
        ("方法数量", metrics.methods, args.max_methods),
        ("字段数量", metrics.fields, args.max_fields),
        ("字符串数量", metrics.strings, args.max_strings),
    )
    return [failure for name, actual, maximum in checks if (failure := _check_limit(name, actual, maximum))]


def load_limits(path: Path, args: argparse.Namespace) -> None:
    try:
        limits = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise ValueError(f"指标上限文件不是有效 JSON：{error}") from error
    if limits.get("schema") != 1:
        raise ValueError("指标上限协议不受支持")
    for name in ("bytes", "classes", "methods", "fields", "strings"):
        value = limits.get("maximum", {}).get(name)
        if not isinstance(value, int) or value <= 0:
            raise ValueError(f"指标上限缺少有效字段：maximum.{name}")
        argument = f"max_{name}"
        if getattr(args, argument) is None:
            setattr(args, argument, value)


def main() -> int:
    parser = argparse.ArgumentParser(description="分析 Stub DEX 结构指标")
    parser.add_argument("input", type=Path, help="stub-classes.dex 或资源 ZIP")
    parser.add_argument("--output", type=Path, help="JSON 报告输出路径")
    parser.add_argument("--limits", type=Path, help="JSON 指标上限文件")
    parser.add_argument("--max-bytes", type=int)
    parser.add_argument("--max-classes", type=int)
    parser.add_argument("--max-methods", type=int)
    parser.add_argument("--max-fields", type=int)
    parser.add_argument("--max-strings", type=int)
    args = parser.parse_args()

    try:
        if args.limits:
            load_limits(args.limits, args)
        data, source = load_dex(args.input)
        metrics = analyze_dex(data, source)
        report = json.dumps(asdict(metrics), ensure_ascii=False, indent=2) + "\n"
        if args.output:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            with args.output.open("w", encoding="utf-8", newline="\n") as output:
                output.write(report)
        else:
            print(report, end="")
        failures = check_limits(metrics, args)
        for failure in failures:
            print(f"错误：{failure}", file=sys.stderr)
        return 1 if failures else 0
    except (OSError, ValueError, zipfile.BadZipFile) as error:
        print(f"错误：{error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
