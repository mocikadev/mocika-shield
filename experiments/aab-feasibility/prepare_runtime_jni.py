#!/usr/bin/env python3
"""从壳资源包提取 AAB 实验所需的 JNI 运行库。"""

from __future__ import annotations

import argparse
import json
import shutil
import zipfile
from pathlib import Path


LIBRARY_NAME = "libmocikashield.so"
MARKER_NAME = ".mocika-aab-runtime-jni"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--resources", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    return parser.parse_args()


def prepare(resources: Path, output: Path) -> list[str]:
    with zipfile.ZipFile(resources) as archive:
        entries = sorted(
            name
            for name in archive.namelist()
            if name.startswith("lib/") and name.endswith(f"/{LIBRARY_NAME}")
        )
        if not entries:
            raise ValueError("壳资源包不包含 JNI 运行库")

        if output.exists():
            unexpected = [
                path
                for path in output.rglob("*")
                if path.is_file()
                and path.name not in {LIBRARY_NAME, MARKER_NAME}
            ]
            if unexpected:
                raise ValueError(f"输出目录包含非实验文件，拒绝清理：{unexpected}")
            shutil.rmtree(output)
        output.mkdir(parents=True)
        (output / MARKER_NAME).write_text("AAB 实验 JNI 临时目录\n", encoding="utf-8")

        abis: list[str] = []
        for entry in entries:
            parts = Path(entry).parts
            if len(parts) != 3:
                raise ValueError(f"JNI 运行库路径格式异常：{entry}")
            abi = parts[1]
            target = output / abi / LIBRARY_NAME
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(archive.read(entry))
            abis.append(abi)
    return abis


def main() -> int:
    args = parse_args()
    abis = prepare(args.resources.resolve(), args.output.resolve())
    print(json.dumps({"native_abis": abis, "output": str(args.output.resolve())}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
