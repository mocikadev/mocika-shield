import struct
import argparse
import json
import tempfile
import unittest
from pathlib import Path

from scripts.analyze_stub_dex import analyze_dex, check_limits, load_limits


def build_test_dex() -> bytes:
    strings = ["Lmsk/a;", "缓存校验失败: e1"]
    string_data = [bytes([len(value)]) + value.encode() + b"\0" for value in strings]
    string_ids_offset = 112
    type_ids_offset = string_ids_offset + len(strings) * 4
    class_defs_offset = type_ids_offset + 4
    data_offset = class_defs_offset + 32
    string_offsets = []
    cursor = data_offset
    for value in string_data:
        string_offsets.append(cursor)
        cursor += len(value)

    dex = bytearray(cursor)
    dex[:8] = b"dex\n035\0"
    struct.pack_into("<I", dex, 32, len(dex))
    struct.pack_into("<I", dex, 36, 112)
    struct.pack_into("<I", dex, 40, 0x12345678)
    struct.pack_into("<II", dex, 56, len(strings), string_ids_offset)
    struct.pack_into("<II", dex, 64, 1, type_ids_offset)
    struct.pack_into("<I", dex, 72, 2)
    struct.pack_into("<I", dex, 80, 3)
    struct.pack_into("<I", dex, 88, 4)
    struct.pack_into("<II", dex, 96, 1, class_defs_offset)
    for index, offset in enumerate(string_offsets):
        struct.pack_into("<I", dex, string_ids_offset + index * 4, offset)
    struct.pack_into("<I", dex, type_ids_offset, 0)
    struct.pack_into("<I", dex, class_defs_offset, 0)
    for offset, value in zip(string_offsets, string_data):
        dex[offset : offset + len(value)] = value
    return bytes(dex)


class AnalyzeStubDexTests(unittest.TestCase):
    def test_读取结构指标与可读线索(self) -> None:
        metrics = analyze_dex(build_test_dex(), "fixture.dex")

        self.assertEqual(metrics.classes, 1)
        self.assertEqual(metrics.methods, 4)
        self.assertEqual(metrics.fields, 3)
        self.assertEqual(metrics.strings, 2)
        self.assertEqual(metrics.class_descriptors, ["Lmsk/a;"])
        self.assertEqual(metrics.diagnostic_strings, ["缓存校验失败: e1"])

    def test_拒绝非DEX输入(self) -> None:
        with self.assertRaisesRegex(ValueError, "有效的 DEX"):
            analyze_dex(b"not a dex".ljust(112, b"0"), "invalid")

    def test_拒绝越界字符串表(self) -> None:
        dex = bytearray(build_test_dex())
        struct.pack_into("<I", dex, 60, len(dex) - 1)

        with self.assertRaisesRegex(ValueError, "索引表越界"):
            analyze_dex(bytes(dex), "invalid")

    def test_读取上限并拒绝指标膨胀(self) -> None:
        args = argparse.Namespace(
            max_bytes=None,
            max_classes=None,
            max_methods=None,
            max_fields=None,
            max_strings=None,
        )
        limits = {
            "schema": 1,
            "maximum": {
                "bytes": 100,
                "classes": 1,
                "methods": 3,
                "fields": 3,
                "strings": 2,
            },
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "limits.json"
            path.write_text(json.dumps(limits), encoding="utf-8")
            load_limits(path, args)

        failures = check_limits(analyze_dex(build_test_dex(), "fixture.dex"), args)

        self.assertTrue(any("DEX 字节数" in failure for failure in failures))
        self.assertTrue(any("方法数量" in failure for failure in failures))


if __name__ == "__main__":
    unittest.main()
