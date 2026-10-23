import struct
import unittest
import os
import pathlib
import subprocess

from scripts.native_surface import audit_native


ABIS = {"armeabi-v7a": (1, 40), "arm64-v8a": (2, 183), "x86": (1, 3), "x86_64": (2, 62)}


def make_elf(abi="arm64-v8a", exports=("JNI_OnLoad",), imports=("malloc",),
             extra_section=None, marker=b"", binding=1, visibility=0, symbol_kind=2):
    """生成仅用于表结构审计的夹具，不是可运行 Native 库。"""
    elf_class, machine = ABIS[abi]
    is_64 = elf_class == 2
    symbol_size = 24 if is_64 else 16
    names = bytearray(b"\0")
    symbols = bytearray(symbol_size)
    for name, defined in [(name, True) for name in exports] + [(name, False) for name in imports]:
        name_offset = len(names)
        names.extend(name.encode() + b"\0")
        info = ((binding if defined else 1) << 4) | symbol_kind
        index, address = (1, 0x1000) if defined else (0, 0)
        if is_64:
            symbols.extend(struct.pack("<IBBHQQ", name_offset, info, visibility, index, address, 4))
        else:
            symbols.extend(struct.pack("<IIIBBH", name_offset, address, 4, info, visibility, index))
    sections = [("", 0, 0, b"", 0, 0),
                (".text", 1, 6, b"\0" * 4, 0, 0),
                (".dynstr", 3, 2, bytes(names), 0, 0),
                (".dynsym", 11, 2, bytes(symbols), 2, symbol_size),
                (".rodata", 1, 2, marker, 0, 0)]
    if extra_section:
        name, kind = extra_section
        sections.append((name, kind, 0, b"debug", 0, 0))
    name_table = bytearray(b"\0")
    offsets = {"": 0}
    for name in [section[0] for section in sections[1:]] + [".shstrtab"]:
        offsets[name] = len(name_table)
        name_table.extend(name.encode() + b"\0")
    sections.append((".shstrtab", 3, 0, bytes(name_table), 0, 0))
    header_size, section_size = (64, 64) if is_64 else (52, 40)
    data = bytearray(header_size)
    headers = []
    for name, kind, flags, content, link, entry_size in sections:
        start = len(data)
        data.extend(content)
        headers.append((offsets[name], kind, flags, 0x1000 if name == ".text" else 0,
                        start, len(content), link, 0, 1, entry_size))
    section_offset = len(data)
    for header in headers:
        data.extend(struct.pack("<IIQQQQIIQQ" if is_64 else "<IIIIIIIIII", *header))
    ident = b"\x7fELF" + bytes([elf_class, 1, 1]) + b"\0" * 9
    struct.pack_into("<16sHHIQQQIHHHHHH" if is_64 else "<16sHHIIIIIHHHHHH", data, 0,
                     ident, 3, machine, 1, 0, 0, section_offset, 0, header_size,
                     0, 0, section_size, len(sections), len(sections) - 1)
    return bytes(data)


class NativeSurfaceTests(unittest.TestCase):
    def test_构建路径参数保留空格中文且不污染主进程环境(self):
        helper = pathlib.Path(__file__).resolve().parents[1] / "native-build-flags.sh"
        environment = dict(os.environ, CARGO_HOME="/task/依赖 空格", RUSTUP_HOME="/task/工具 链", OS="test-unix")
        result = subprocess.run(["bash", "-c",
            'source "$1"; configure_native_source_paths "$2"; printf "%s" "$CARGO_ENCODED_RUSTFLAGS"',
            "--", str(helper), "/task/中文 项目"], env=environment, text=True, capture_output=True, check=True)
        self.assertEqual(result.stdout.split("\x1f"), ["--remap-path-prefix", "/task/中文 项目=.",
            "--remap-path-prefix", "/task/依赖 空格=.cargo", "--remap-path-prefix", "/task/工具 链=.rustup"])

    def test_四种架构只允许最小导出且不误报系统导入(self):
        for abi in ABIS:
            with self.subTest(abi=abi):
                result = audit_native(make_elf(abi), abi)
                self.assertEqual(result["exports"], ["JNI_OnLoad"])

    def test_拒绝额外全局弱和唯一可见导出(self):
        for binding in (1, 2, 10):
            with self.subTest(binding=binding), self.assertRaisesRegex(ValueError, "导出"):
                audit_native(make_elf(exports=("JNI_OnLoad", "decrypt"), binding=binding), "arm64-v8a")

    def test_入口不能只有导入或被隐藏或不是函数(self):
        for data in (make_elf(exports=(), imports=("JNI_OnLoad",)),
                     make_elf(visibility=2), make_elf(symbol_kind=1)):
            with self.subTest(data=data[:8]), self.assertRaisesRegex(ValueError, "JNI_OnLoad"):
                audit_native(data, "arm64-v8a")

    def test_拒绝调试内容及改名的普通符号表(self):
        for section in ((".debug_info", 1), (".zdebug_line", 1),
                        (".gnu_debugdata", 1), (".renamed_symbols", 2)):
            with self.subTest(section=section), self.assertRaisesRegex(ValueError, "调试|符号表"):
                audit_native(make_elf(extra_section=section), "arm64-v8a")

    def test_允许加载定位元数据和规范化源码信息(self):
        data = make_elf(extra_section=(".eh_frame", 1), marker=b".cargo/registry/src/crate/lib.rs\0/rustc/hash/library/core.rs\0")
        result = audit_native(data, "arm64-v8a")
        self.assertTrue(result["strings"]["normalized_dependency_paths"])
        self.assertTrue(result["strings"]["toolchain_paths"])

    def test_拦截本机路径且不回显用户名(self):
        for marker in (b"/Users/private-person/.cargo/src/lib.rs", b"/home/private-person/project/src/lib.rs",
                       b"C:\\Users\\private-person\\.cargo\\lib.rs", b"C:\\users\\private-person\\lib.rs"):
            with self.subTest(marker=marker), self.assertRaisesRegex(ValueError, "构建机绝对路径") as error:
                audit_native(make_elf(marker=marker), "arm64-v8a")
            self.assertNotIn("private-person", str(error.exception))

    def test_拒绝原始包路径且品牌线索分类不误删(self):
        for marker in (b"dev/mocika/shield/loader/Ld", b"dev.mocika.shield.loader.Ld"):
            with self.assertRaisesRegex(ValueError, "桥接包路径"):
                audit_native(make_elf(marker=marker), "arm64-v8a")
        self.assertIn("mocika", audit_native(make_elf(marker=b"mocika-example"), "arm64-v8a")["strings"]["brand_markers"])

    def test_拒绝错误架构截断及缺失动态符号表(self):
        data = make_elf()
        for invalid, abi in ((data, "x86"), (data[:30], "arm64-v8a"), (data[:-1], "arm64-v8a")):
            with self.subTest(abi=abi), self.assertRaises(ValueError):
                audit_native(invalid, abi)
        broken = bytearray(data)
        section_start = struct.unpack_from("<Q", data, 40)[0]
        struct.pack_into("<I", broken, section_start + 3 * 64 + 4, 1)
        with self.assertRaisesRegex(ValueError, "动态符号表"):
            audit_native(bytes(broken), "arm64-v8a")

    def test_拒绝损坏符号表与非法字符串引用(self):
        for abi, is_64 in (("arm64-v8a", True), ("x86", False)):
            data = make_elf(abi)
            section_start = struct.unpack_from("<Q" if is_64 else "<I", data, 40 if is_64 else 32)[0]
            width = 64 if is_64 else 40
            symbols_header = section_start + 3 * width
            for field in (6, 9):
                broken = bytearray(data)
                field_offset = (40 if field == 6 else 56) if is_64 else field * 4
                struct.pack_into("<I" if field == 6 or not is_64 else "<Q", broken,
                                 symbols_header + field_offset, 999)
                with self.subTest(abi=abi, field=field), self.assertRaises(ValueError):
                    audit_native(bytes(broken), abi)
            symbol_start = struct.unpack_from("<Q" if is_64 else "<I", data, symbols_header + (24 if is_64 else 16))[0]
            broken = bytearray(data)
            struct.pack_into("<I", broken, symbol_start + (24 if is_64 else 16), 0xffffffff)
            with self.assertRaisesRegex(ValueError, "字符串索引越界"):
                audit_native(bytes(broken), abi)


if __name__ == "__main__":
    unittest.main()
