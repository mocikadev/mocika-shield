"""读取构建期 Stub DEX 的类及实际方法定义，不依赖外部 Android 工具。"""

from __future__ import annotations

import struct


def dex_contract(data: bytes) -> tuple[set[str], dict]:
    if len(data) < 112 or data[:8] not in (b"dex\n035\0", b"dex\n037\0", b"dex\n038\0", b"dex\n039\0", b"dex\n040\0"):
        raise ValueError("不支持的 Stub DEX 格式")

    def u32(offset):
        if offset < 0 or offset + 4 > len(data):
            raise ValueError("Stub DEX 索引越界")
        return struct.unpack_from("<I", data, offset)[0]

    if u32(32) != len(data) or u32(36) != 112 or u32(40) != 0x12345678:
        raise ValueError("Stub DEX 头不完整")

    def table(header, width):
        count, start = u32(header), u32(header + 4)
        if count and (start < 112 or count > (len(data) - start) // width):
            raise ValueError("Stub DEX 表越界")
        return [start + index * width for index in range(count)]

    def uleb(offset):
        value = 0
        for index in range(5):
            if offset >= len(data):
                raise ValueError("Stub DEX ULEB128 截断")
            byte = data[offset]
            offset += 1
            value |= (byte & 127) << (index * 7)
            if not byte & 128:
                return value, offset
        raise ValueError("Stub DEX ULEB128 越界")

    def string(offset):
        _, start = uleb(offset)
        end = data.find(b"\0", start)
        if end < 0:
            raise ValueError("Stub DEX 字符串截断")
        return data[start:end].replace(b"\xc0\x80", b"\0").decode("utf-8", errors="surrogatepass")

    strings = [string(u32(offset)) for offset in table(56, 4)]

    def item(items, index):
        if index >= len(items):
            raise ValueError("Stub DEX 引用越界")
        return items[index]

    types = [item(strings, u32(offset)) for offset in table(64, 4)]
    prototypes = []
    for offset in table(72, 12):
        parameters_offset = u32(offset + 8)
        parameters = []
        if parameters_offset:
            count = u32(parameters_offset)
            if count > (len(data) - parameters_offset - 4) // 2:
                raise ValueError("Stub DEX 参数列表越界")
            for index in range(count):
                type_index = struct.unpack_from("<H", data, parameters_offset + 4 + index * 2)[0]
                parameters.append(item(types, type_index))
        prototypes.append("(" + "".join(parameters) + ")" + item(types, u32(offset + 4)))
    identifiers = []
    for offset in table(88, 8):
        owner, prototype, name = struct.unpack_from("<HHI", data, offset)
        identifiers.append((item(types, owner), item(strings, name), item(prototypes, prototype)))

    classes, methods = set(), {}
    for offset in table(96, 32):
        owner = item(types, u32(offset))
        if owner in classes:
            raise ValueError("Stub DEX 类重复")
        classes.add(owner)
        cursor = u32(offset + 24)
        if not cursor:
            continue
        counts = []
        for _ in range(4):
            count, cursor = uleb(cursor)
            if count > len(data):
                raise ValueError("Stub DEX 成员数量越界")
            counts.append(count)
        for _ in range(counts[0] + counts[1]):
            _, cursor = uleb(cursor)
            _, cursor = uleb(cursor)
        for count in counts[2:]:
            index = 0
            for _ in range(count):
                difference, cursor = uleb(cursor)
                index += difference
                flags, cursor = uleb(cursor)
                _, cursor = uleb(cursor)
                identifier = item(identifiers, index)
                if identifier[0] != owner or identifier in methods:
                    raise ValueError("Stub DEX 方法定义重复或所属类错误")
                methods[identifier] = flags
    return classes, methods
