#!/usr/bin/env python3
"""验证 AAB 中 DEX 尾部标记经过 bundletool 生成 APK 后是否保留。"""

from __future__ import annotations

import argparse
import io
import json
import struct
import subprocess
import zipfile
from pathlib import Path


MARKER = b"MOCIKA_AAB_TAIL_PROBE_V1"
DEX_FILE_SIZE_OFFSET = 32


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--aab", required=True, type=Path, help="Gradle 生成的原始 AAB")
    parser.add_argument("--bundletool", required=True, type=Path, help="bundletool-all jar")
    parser.add_argument("--output", required=True, type=Path, help="实验产物目录")
    return parser.parse_args()


def dex_file_size(data: bytes) -> int:
    if len(data) < DEX_FILE_SIZE_OFFSET + 4 or not data.startswith(b"dex\n"):
        raise ValueError("输入内容不是有效 DEX")
    return struct.unpack_from("<I", data, DEX_FILE_SIZE_OFFSET)[0]


def inject_marker(source: Path, target: Path) -> dict[str, int | str]:
    dex_entry = "base/dex/classes.dex"
    with zipfile.ZipFile(source) as archive:
        original = archive.read(dex_entry)
        if MARKER in original:
            raise ValueError("原始 DEX 已包含实验标记")
        target.parent.mkdir(parents=True, exist_ok=True)
        with zipfile.ZipFile(target, "w") as output:
            for entry in archive.infolist():
                data = archive.read(entry.filename)
                if entry.filename == dex_entry:
                    data += MARKER
                output.writestr(entry, data)
    return {
        "aab_dex_entry": dex_entry,
        "dex_header_file_size": dex_file_size(original),
        "dex_original_bytes": len(original),
        "dex_injected_bytes": len(original) + len(MARKER),
        "marker_bytes": len(MARKER),
    }


def build_apks(bundletool: Path, aab: Path, target: Path, mode: str | None) -> None:
    command = [
        "java",
        "-jar",
        str(bundletool),
        "build-apks",
        f"--bundle={aab}",
        f"--output={target}",
        "--overwrite",
    ]
    if mode:
        command.append(f"--mode={mode}")
    subprocess.run(command, check=True)


def inspect_apks(path: Path) -> list[dict[str, int | str | bool]]:
    results: list[dict[str, int | str | bool]] = []
    with zipfile.ZipFile(path) as apk_set:
        for apk_entry in apk_set.namelist():
            if not apk_entry.endswith(".apk"):
                continue
            with zipfile.ZipFile(io.BytesIO(apk_set.read(apk_entry))) as apk:
                for dex_entry in apk.namelist():
                    if not dex_entry.endswith(".dex"):
                        continue
                    data = apk.read(dex_entry)
                    marker_offset = data.find(MARKER)
                    results.append(
                        {
                            "apk": apk_entry,
                            "dex": dex_entry,
                            "dex_bytes": len(data),
                            "dex_header_file_size": dex_file_size(data),
                            "marker_preserved": marker_offset >= 0,
                            "marker_offset": marker_offset,
                            "marker_at_end": marker_offset + len(MARKER) == len(data),
                        }
                    )
    return results


def main() -> int:
    args = parse_args()
    output = args.output.resolve()
    injected_aab = output / "probe-injected.aab"
    metadata = inject_marker(args.aab.resolve(), injected_aab)

    reports: dict[str, object] = {"input": metadata, "outputs": {}}
    for name, mode in (("split", None), ("universal", "universal")):
        apks = output / f"probe-{name}.apks"
        build_apks(args.bundletool.resolve(), injected_aab, apks, mode)
        reports["outputs"][name] = inspect_apks(apks)  # type: ignore[index]

    preserved = [
        row
        for rows in reports["outputs"].values()  # type: ignore[union-attr]
        for row in rows
        if row["marker_preserved"] and row["marker_at_end"]
    ]
    reports["conclusion"] = {
        "marker_preserved": bool(preserved),
        "matching_dex_count": len(preserved),
    }
    report_path = output / "report.json"
    report_path.write_text(json.dumps(reports, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(report_path.read_text(encoding="utf-8"), end="")
    return 0 if preserved else 1


if __name__ == "__main__":
    raise SystemExit(main())
