#!/usr/bin/env python3
"""将正式 DEXB v5 载荷与运行时壳资源组装进实验 AAB。"""

from __future__ import annotations

import argparse
import json
import struct
import zipfile
from pathlib import Path


DEX_ENTRY = "base/dex/classes.dex"
MSHD_MAGIC = b"MSHD"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--original-aab", required=True, type=Path)
    parser.add_argument("--shell-aab", required=True, type=Path)
    parser.add_argument("--resources", required=True, type=Path)
    parser.add_argument("--dexb", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--dex-dir", required=True, type=Path)
    return parser.parse_args()


def extract_original_dex(original_aab: Path, dex_dir: Path) -> list[Path]:
    dex_dir.mkdir(parents=True, exist_ok=True)
    extracted: list[Path] = []
    with zipfile.ZipFile(original_aab) as archive:
        entries = sorted(
            name
            for name in archive.namelist()
            if name.startswith("base/dex/") and name.endswith(".dex")
        )
        if not entries:
            raise ValueError("原始 AAB 不包含 base DEX")
        for entry in entries:
            target = dex_dir / Path(entry).name
            target.write_bytes(archive.read(entry))
            extracted.append(target)
    return extracted


def validate_dexb(payload: bytes) -> None:
    if len(payload) < 13 or payload[:4] != b"DEXB":
        raise ValueError("输入不是 DEXB 数据")
    version = struct.unpack_from("<I", payload, 4)[0]
    if version != 5:
        raise ValueError(f"DEXB 版本不是 v5：{version}")


def assemble(shell_aab: Path, resources: Path, dexb: Path, output: Path) -> dict[str, object]:
    payload = dexb.read_bytes()
    validate_dexb(payload)
    with zipfile.ZipFile(resources) as runtime:
        stub = runtime.read("stub-classes.dex")
        expected_libraries = sorted(
            f"base/{name}"
            for name in runtime.namelist()
            if name.startswith("lib/") and name.endswith("/libmocikashield.so")
        )
        if not expected_libraries:
            raise ValueError("壳资源包不包含 JNI 运行库")
        metadata = json.loads(runtime.read("metadata.json"))

    dex_header_size = struct.unpack_from("<I", stub, 32)[0]
    if dex_header_size != len(stub):
        raise ValueError("壳 DEX 头部 file_size 与物理长度不一致")
    protected_dex = stub + MSHD_MAGIC + struct.pack("<I", len(payload)) + payload

    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(shell_aab) as source, zipfile.ZipFile(output, "w") as target:
        source_entries = set(source.namelist())
        missing_libraries = sorted(set(expected_libraries) - source_entries)
        if missing_libraries:
            raise ValueError(f"壳 AAB 缺少 Gradle 打包的 JNI 运行库：{missing_libraries}")
        for entry in source.infolist():
            if entry.filename == DEX_ENTRY or entry.filename.startswith("META-INF/"):
                continue
            target.writestr(entry, source.read(entry.filename))
        target.writestr(DEX_ENTRY, protected_dex)

    return {
        "stub_application": metadata["stub_application"],
        "original_stub_dex_bytes": len(stub),
        "protected_dex_bytes": len(protected_dex),
        "dex_header_file_size": dex_header_size,
        "dexb_bytes": len(payload),
        "native_abis": sorted(Path(name).parts[2] for name in expected_libraries),
    }


def main() -> int:
    args = parse_args()
    extracted = extract_original_dex(args.original_aab.resolve(), args.dex_dir.resolve())
    if not args.dexb.exists():
        print(json.dumps({"extracted_dex": [str(path) for path in extracted]}, ensure_ascii=False))
        return 2
    report = assemble(
        args.shell_aab.resolve(),
        args.resources.resolve(),
        args.dexb.resolve(),
        args.output.resolve(),
    )
    report["extracted_dex"] = [str(path) for path in extracted]
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
