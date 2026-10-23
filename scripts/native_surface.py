"""只读审计 Android Native 产物的导出、调试信息和构建路径，不改写 ELF。"""

from __future__ import annotations

import re
import struct
from dataclasses import dataclass


ABI_FORMATS = {"armeabi-v7a": (1, 40), "arm64-v8a": (2, 183), "x86": (1, 3), "x86_64": (2, 62)}


@dataclass(frozen=True)
class _Section:
    name: str
    kind: int
    flags: int
    offset: int
    size: int
    link: int
    entry_size: int


class _Elf:
    def __init__(self, data: bytes, abi: str):
        if len(data) < 16 or data[:4] != b"\x7fELF" or data[5:7] != b"\x01\x01":
            raise ValueError("Native ELF 格式、字节序或版本无效")
        expected = ABI_FORMATS.get(abi)
        if expected is None or data[4] != expected[0]:
            raise ValueError("Native ELF 位宽与预期 ABI 不一致")
        self.data = data
        self.is_64 = data[4] == 2
        header_format = "<16sHHIQQQIHHHHHH" if self.is_64 else "<16sHHIIIIIHHHHHH"
        header_size = struct.calcsize(header_format)
        header = struct.unpack(header_format, self.span(0, header_size))
        if header[1:4] != (3, expected[1], 1) or header[8] != header_size:
            raise ValueError("Native ELF 不是预期 ABI 的共享库")
        table_offset, entry_size, count, names_index = header[6], header[11], header[12], header[13]
        section_format = "<IIQQQQIIQQ" if self.is_64 else "<IIIIIIIIII"
        if not count or names_index >= count or entry_size != struct.calcsize(section_format):
            raise ValueError("Native ELF 节表缺失或表项大小无效")
        self.span(table_offset, entry_size * count)
        raw = [struct.unpack(section_format, self.span(table_offset + index * entry_size, entry_size)) for index in range(count)]
        names = raw[names_index]
        if names[1] != 3:
            raise ValueError("Native ELF 节名字符串表无效")
        name_data = self.span(names[4], names[5])
        self.sections = []
        for section in raw:
            # NOBITS 是正常的 .bss 等内存节，不占用对应文件字节。
            if section[1] != 8:
                self.span(section[4], section[5])
            self.sections.append(_Section(self.string(name_data, section[0]), section[1], section[2],
                                          section[4], section[5], section[6], section[9]))

    def span(self, offset: int, size: int) -> bytes:
        if offset < 0 or size < 0 or offset > len(self.data) or size > len(self.data) - offset:
            raise ValueError("Native ELF 数据截断或索引越界")
        return self.data[offset:offset + size]

    @staticmethod
    def string(data: bytes, offset: int) -> str:
        if offset >= len(data):
            raise ValueError("Native ELF 字符串索引越界")
        end = data.find(b"\0", offset)
        if end < 0:
            raise ValueError("Native ELF 字符串缺少结束标记")
        return data[offset:end].decode("ascii")

    def exports(self) -> list[dict]:
        tables = [section for section in self.sections if section.kind == 11]
        if len(tables) != 1:
            raise ValueError("Native ELF 必须有且只有一份动态符号表")
        table = tables[0]
        symbol_format = "<IBBHQQ" if self.is_64 else "<IIIBBH"
        width = struct.calcsize(symbol_format)
        if table.entry_size != width or table.size % width or table.link >= len(self.sections):
            raise ValueError("Native ELF 动态符号表项或链接无效")
        strings = self.sections[table.link]
        if strings.kind != 3:
            raise ValueError("Native ELF 动态字符串表无效")
        names = self.span(strings.offset, strings.size)
        exports = []
        for offset in range(table.offset, table.offset + table.size, width):
            symbol = struct.unpack(symbol_format, self.span(offset, width))
            if self.is_64:
                name, info, visibility, section_index = symbol[:4]
            else:
                name, _, _, info, visibility, section_index = symbol
            name = self.string(names, name)
            binding, kind, visibility = info >> 4, info & 15, visibility & 3
            # UND 是系统导入；HIDDEN／INTERNAL 不是对外接口。
            if section_index and binding in (1, 2, 10) and visibility in (0, 3):
                exports.append({"name": name, "binding": binding, "kind": kind,
                                "section": section_index})
        return exports


def _string_inventory(data: bytes) -> dict:
    normalized = data.replace(b"\\", b"/")
    normalized_lower = normalized.lower()
    if b"/users/" in normalized_lower or b"/home/" in normalized_lower:
        raise ValueError("Native 残留构建机绝对路径，须修正构建期路径规范化")
    if b"dev/mocika/shield/loader/" in data or b"dev.mocika.shield.loader." in data:
        raise ValueError("Native 残留原始 JNI 桥接包路径")
    lower = data.lower()
    return {
        "normalized_dependency_paths": b".cargo/registry/" in normalized,
        "toolchain_paths": b"/rustc/" in normalized or b".rustup/" in normalized,
        "brand_markers": [word for word in ("mocika", "shield-stub") if word.encode() in lower],
        "owned_diagnostics": [name for name, marker in (
            ("native_fallback", "注入失败，降级到 Java 反射"),
            ("payload_read", "读取 dex_data 失败"),
            ("payload_extract", "提取 MSHD payload 失败"),
            ("payload_decrypt", "解密解压失败"),
        ) if marker.encode("utf-8") in data],
    }


def audit_native(data: bytes, abi: str) -> dict:
    elf = _Elf(data, abi)
    for section in elf.sections:
        if section.kind == 2:
            raise ValueError("Native 残留普通符号表")
        if re.match(r"^\.(?:debug(?:_|$)|zdebug(?:_|$)|gnu_debugdata$)", section.name):
            raise ValueError("Native 残留调试信息")
    exports = elf.exports()
    if len(exports) != 1 or exports[0]["name"] != "JNI_OnLoad":
        names = ", ".join(symbol["name"][:80] for symbol in exports[:6]) or "无"
        raise ValueError(f"Native 对外导出必须且只能是 JNI_OnLoad，实际：{names}")
    entry = exports[0]
    if (entry["binding"] != 1 or entry["kind"] != 2
            or entry["section"] >= len(elf.sections)
            or not elf.sections[entry["section"]].flags & 4):
        raise ValueError("Native JNI_OnLoad 导出必须是可执行节内的全局函数")
    return {"exports": ["JNI_OnLoad"], "strings": _string_inventory(data)}
