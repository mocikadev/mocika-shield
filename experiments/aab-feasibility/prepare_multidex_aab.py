#!/usr/bin/env python3
"""编译独立探针类并将其作为 classes2.dex 写入实验 AAB。"""

from __future__ import annotations

import argparse
import json
import subprocess
import tempfile
import zipfile
from pathlib import Path


SECOND_DEX_ENTRY = "base/dex/classes2.dex"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--aab", required=True, type=Path)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--d8", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    return parser.parse_args()


def add_second_dex(aab: Path, second_dex: bytes, output: Path) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(aab) as source, zipfile.ZipFile(output, "w") as target:
        for entry in source.infolist():
            if entry.filename == SECOND_DEX_ENTRY or entry.filename.startswith("META-INF/"):
                continue
            target.writestr(entry, source.read(entry.filename))
        target.writestr(SECOND_DEX_ENTRY, second_dex)


def prepare(aab: Path, source: Path, d8: Path, output: Path) -> int:
    with tempfile.TemporaryDirectory(prefix="mocika-aab-multidex-") as directory:
        root = Path(directory)
        classes = root / "classes"
        dex = root / "dex"
        classes.mkdir()
        dex.mkdir()
        subprocess.run(
            ["javac", "-source", "8", "-target", "8", "-d", str(classes), str(source)],
            check=True,
        )
        class_file = classes / "dev/mocika/shield/aabprobe/SecondDexMessage.class"
        subprocess.run(
            [str(d8), "--min-api", "23", "--output", str(dex), str(class_file)],
            check=True,
        )
        second_dex = (dex / "classes.dex").read_bytes()
        add_second_dex(aab, second_dex, output)
        return len(second_dex)


def main() -> int:
    args = parse_args()
    size = prepare(args.aab.resolve(), args.source.resolve(), args.d8.resolve(), args.output.resolve())
    print(json.dumps({"second_dex_bytes": size, "output": str(args.output.resolve())}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
