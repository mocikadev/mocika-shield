import unittest
import json
import struct
import tempfile
import zipfile
from pathlib import Path
from unittest.mock import patch

from scripts.stub_bindings import METHODS, audit, binding_environment, parse_mapping, prepare_rules
from scripts.stub_dex_contract import dex_contract


LD = "dev.mocika.shield.loader.Ld"
RULE = "-keepclasseswithmembernames,includedescriptorclasses class * {\n    native <methods>;\n}"
MAPPING = f"""{LD} -> msk.a:
    boolean p(java.lang.ClassLoader,java.lang.String[],java.lang.String) -> a
    byte[][] q(android.content.Context,byte[]) -> b
    int r() -> c
    java.nio.ByteBuffer[] s(android.content.Context,byte[]) -> d
    1:2:java.lang.String getSignatureSha256(android.content.Context):3:4 -> e
dev.mocika.shield.loader.StubApp -> msk.b:
dev.mocika.shield.loader.StubComponentFactory -> msk.c:
"""


class StubBindingTests(unittest.TestCase):
    def test_只移除预期的默认名称约束(self):
        source = "-keepattributes Signature\n" + RULE + "\n-keep class example.Other { *; }\n"
        result = prepare_rules(source)
        self.assertNotIn(RULE, result)
        self.assertIn("-keepattributes Signature", result)
        self.assertIn("-keep class example.Other { *; }", result)

    def test_拒绝缺失重复或未知规则(self):
        for source in ("", RULE + "\n" + RULE, RULE.replace("includedescriptorclasses", "allowshrinking")):
            with self.subTest(source=source), self.assertRaises(ValueError):
                prepare_rules(source)

    def test_完整绑定包含第四个方法和签名回调(self):
        environment = binding_environment(parse_mapping(MAPPING))
        self.assertEqual(environment["STUB_BINLOADER_CLASS"], "msk/a")
        self.assertEqual(environment["STUB_METHOD_DECRYPT_BUFFERS"], "d")
        self.assertEqual(environment["STUB_METHOD_GET_SIG"], "e")
        self.assertEqual(environment["STUB_SIGNATURE_GET_SIG"], "(Landroid/content/Context;)Ljava/lang/String;")
        self.assertEqual(environment["STUB_APPLICATION"], "msk.b")

    def test_显式同名映射有效但不允许原始类名(self):
        mapping = MAPPING.replace(" -> a\n", " -> p\n")
        self.assertEqual(binding_environment(parse_mapping(mapping))["STUB_METHOD_INJECT_DEX"], "p")
        with self.assertRaisesRegex(ValueError, "原始包名"):
            binding_environment(parse_mapping(MAPPING.replace(" -> msk.a:", f" -> {LD}:")))

    def test_缺失方法不能从后续类块匹配(self):
        mapping = MAPPING.replace("    int r() -> c\n", "") + "    int r() -> wrong\n"
        with self.assertRaisesRegex(ValueError, "r"):
            binding_environment(parse_mapping(mapping))

    def test_重载不能代替所需签名(self):
        mapping = MAPPING.replace("int r()", "int r(int)")
        with self.assertRaisesRegex(ValueError, "r"):
            binding_environment(parse_mapping(mapping))

    def test_允许同一方法多个行号范围但拒绝歧义(self):
        line = "    1:2:java.lang.String getSignatureSha256(android.content.Context):3:4 -> e\n"
        repeated = MAPPING.replace(line, line + line.replace("1:2:", "3:4:"))
        binding_environment(parse_mapping(repeated))
        with self.assertRaisesRegex(ValueError, "不唯一"):
            binding_environment(parse_mapping(repeated.replace(":3:4 -> e\n", ":3:4 -> other\n", 1)))

    def test_拒绝重复映射和非法最终名称(self):
        for mapping in (MAPPING + MAPPING, MAPPING.replace(" -> c\n", " -> bad/name\n")):
            with self.subTest(mapping=mapping), self.assertRaises(ValueError):
                binding_environment(parse_mapping(mapping))

    def test_资源审计拒绝错配映射原名和缺失库(self):
        environment = binding_environment(parse_mapping(MAPPING))
        bridge = "Lmsk/a;"
        definitions = {
            (bridge, environment[key], signature): 0x8 | (0x100 if method != "getSignatureSha256" else 0)
            for _, method, _, key, signature in METHODS
        }
        classes = {bridge, "Lmsk/b;", "Lmsk/c;"}
        with tempfile.TemporaryDirectory() as directory:
            mapping, resource = Path(directory) / "mapping.txt", Path(directory) / "resources.zip"
            mapping.write_text(MAPPING, encoding="utf-8")
            for defect in (None, "摘要", "原名", "缺库", "方法"):
                with self.subTest(defect=defect):
                    marker = b"wrong" if defect == "摘要" else environment["STUB_BINDING_ID"].encode()
                    native = b"\x7fELF" + marker
                    if defect == "原名":
                        native += LD.replace(".", "/").encode()
                    with zipfile.ZipFile(resource, "w") as archive:
                        archive.writestr("stub-classes.dex", b"fixture")
                        archive.writestr("metadata.json", json.dumps({"stub_application": "msk.b"}))
                        for abi in ("armeabi-v7a", "arm64-v8a", "x86", "x86_64"):
                            if defect != "缺库" or abi != "x86":
                                archive.writestr(f"lib/{abi}/libmocikashield.so", native)
                    methods = {} if defect == "方法" else definitions
                    with patch("scripts.stub_bindings.dex_contract", return_value=(classes, methods)):
                        if defect is None:
                            self.assertEqual(audit(mapping, resource)["bridge"], bridge)
                        else:
                            with self.assertRaises((ValueError, KeyError)):
                                audit(mapping, resource)

    def test_严格读取类定义而非字符串匹配(self):
        # 最小结构夹具，仅验证表和类读取，不作为可运行 Android DEX。
        dex = bytearray(161)
        dex[:8] = b"dex\n035\0"
        for offset, value in ((32, len(dex)), (36, 112), (40, 0x12345678),
                              (56, 1), (60, 112), (64, 1), (68, 116),
                              (96, 1), (100, 120), (112, 152)):
            struct.pack_into("<I", dex, offset, value)
        dex[152:] = b"\x07Lmsk/a;\0"
        self.assertEqual(dex_contract(bytes(dex)), ({"Lmsk/a;"}, {}))
        struct.pack_into("<I", dex, 120, 5)
        with self.assertRaisesRegex(ValueError, "越界"):
            dex_contract(bytes(dex))


if __name__ == "__main__":
    unittest.main()
