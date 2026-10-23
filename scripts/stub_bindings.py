#!/usr/bin/env python3
"""生成受控混淆规则并统一解析、审计壳的 JNI 构建契约。"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import zipfile
from pathlib import Path

if __package__:
    from .stub_dex_contract import dex_contract
    from .native_surface import audit_native
else:
    from stub_dex_contract import dex_contract
    from native_surface import audit_native


PREFIX = "dev.mocika.shield.loader."
BRIDGE = PREFIX + "Ld"
NATIVE_RULE = re.compile(
    r"(?m)^-keepclasseswithmembernames,includedescriptorclasses class \* \{\r?\n"
    r"    native <methods>;\r?\n\}"
)
CLASS_LINE = re.compile(r"^(\S+) -> ([\w.$]+):$")
METHOD_LINE = re.compile(
    r"^(?:\d+:\d+:)?(\S+) ([\w.$<>]+)\(([^)]*)\)(?::\d+(?::\d+)?)?$"
)
# 原始返回值、方法、参数、编译期变量和最终 JVM 签名。
METHODS = (
    ("boolean", "p", "java.lang.ClassLoader,java.lang.String[],java.lang.String", "STUB_METHOD_INJECT_DEX", "(Ljava/lang/ClassLoader;[Ljava/lang/String;Ljava/lang/String;)Z"),
    ("byte[][]", "q", "android.content.Context,byte[]", "STUB_METHOD_EXTRACT_DECRYPT", "(Landroid/content/Context;[B)[[B"),
    ("int", "r", "", "STUB_METHOD_CHECK_ENV", "()I"),
    ("java.nio.ByteBuffer[]", "s", "android.content.Context,byte[]", "STUB_METHOD_DECRYPT_BUFFERS", "(Landroid/content/Context;[B)[Ljava/nio/ByteBuffer;"),
    ("java.lang.String", "getSignatureSha256", "android.content.Context", "STUB_METHOD_GET_SIG", "(Landroid/content/Context;)Ljava/lang/String;"),
)


def prepare_rules(text: str) -> str:
    if len(NATIVE_RULE.findall(text)) != 1:
        raise ValueError("默认 Native 名称规则缺失、重复或已变化，须审核 AGP 配置")
    return NATIVE_RULE.sub("# Native 绑定由项目完整保留规则及最终映射负责。", text)


def parse_mapping(text: str) -> dict:
    classes = {}
    current = None
    for line in text.splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if not line[0].isspace():
            match = CLASS_LINE.fullmatch(line)
            if not match:
                raise ValueError(f"无法解析 R8 类映射：{line}")
            original, renamed = match.groups()
            if original in classes:
                raise ValueError(f"R8 类映射重复：{original}")
            current = {"name": renamed, "methods": {}}
            classes[original] = current
            continue
        if current is None or " -> " not in line:
            raise ValueError("R8 成员映射不属于有效类块")
        original, renamed = line.strip().rsplit(" -> ", 1)
        match = METHOD_LINE.fullmatch(original)
        if match:
            key = match.groups()
            previous = current["methods"].get(key)
            # 同一方法的多个行号范围可重复出现，但不能指向不同最终名称。
            if previous is not None and previous != renamed:
                raise ValueError(f"R8 方法映射不唯一：{original}")
            if not re.fullmatch(r"[\w$<>]+", renamed):
                raise ValueError(f"R8 方法名无效：{renamed}")
            current["methods"][key] = renamed
    return classes


def binding_environment(classes: dict) -> dict[str, str]:
    environment = {}
    for original, key in ((BRIDGE, "STUB_BINLOADER_CLASS"), (PREFIX + "StubApp", "STUB_APPLICATION"), (PREFIX + "StubComponentFactory", "STUB_COMPONENT_FACTORY")):
        if original not in classes:
            raise ValueError(f"缺少必需类映射：{original}")
        name = classes[original]["name"]
        if name.startswith(PREFIX) or not re.fullmatch(r"[A-Za-z_$][\w$]*(?:\.[A-Za-z_$][\w$]*)+", name):
            raise ValueError(f"最终类名仍为原始包名或格式非法：{name}")
        environment[key] = name.replace(".", "/") if key == "STUB_BINLOADER_CLASS" else name
    for result, method, parameters, key, descriptor in METHODS:
        signature = (result, method, parameters)
        renamed = classes[BRIDGE]["methods"].get(signature)
        if renamed is None:
            raise ValueError(f"缺少必需方法映射：{method}({parameters})")
        environment[key] = renamed
        environment[key.replace("METHOD", "SIGNATURE", 1)] = descriptor
    identity = json.dumps(environment, sort_keys=True, separators=(",", ":"))
    environment["STUB_BINDING_ID"] = hashlib.sha256(identity.encode("ascii")).hexdigest()
    environment["STUB_BINDINGS_REQUIRED"] = "1"
    return environment


def audit(mapping: Path, resource: Path, standard: Path | None = None) -> dict:
    environment = binding_environment(parse_mapping(mapping.read_text(encoding="utf-8-sig")))
    with zipfile.ZipFile(resource) as archive:
        if len(set(archive.namelist())) != len(archive.namelist()):
            raise ValueError("资源 ZIP 含重复条目")
        dex = archive.read("stub-classes.dex")
        classes, methods = dex_contract(dex)
        if PREFIX.replace(".", "/").encode() in dex:
            raise ValueError("Stub DEX 仍含原始包名")
        bridge = "L" + environment["STUB_BINLOADER_CLASS"] + ";"
        for descriptor in (bridge, "L" + environment["STUB_APPLICATION"].replace(".", "/") + ";", "L" + environment["STUB_COMPONENT_FACTORY"].replace(".", "/") + ";"):
            if descriptor not in classes:
                raise ValueError(f"Stub DEX 缺少最终类：{descriptor}")
        for _, original, _, key, signature in METHODS:
            flags = methods.get((bridge, environment[key], signature))
            required = 0x8 | (0x100 if original != "getSignatureSha256" else 0)
            if flags is None or flags & required != required:
                raise ValueError(f"Stub DEX 缺少正确的 JNI 方法定义：{original}")
        metadata = json.loads(archive.read("metadata.json").decode("utf-8-sig"))
        if metadata["stub_application"] != environment["STUB_APPLICATION"]:
            raise ValueError("资源 Application 与映射不一致")
        if metadata.get("stub_component_factory", environment["STUB_COMPONENT_FACTORY"]) != environment["STUB_COMPONENT_FACTORY"]:
            raise ValueError("资源组件工厂与映射不一致")
        native_surface = {}
        for abi in ("armeabi-v7a", "arm64-v8a", "x86", "x86_64"):
            native = archive.read(f"lib/{abi}/libmocikashield.so")
            try:
                native_surface[abi] = audit_native(native, abi)
            except ValueError as error:
                raise ValueError(f"{abi}：{error}") from error
            if not native.startswith(b"\x7fELF") or BRIDGE.replace(".", "/").encode() in native:
                raise ValueError(f"{abi} Native 非 ELF 或仍含原始桥接类名")
            if environment["STUB_BINDING_ID"].encode() not in native:
                raise ValueError(f"{abi} Native 不属于当前绑定映射")
        if standard:
            with zipfile.ZipFile(standard) as reference:
                if dex != reference.read("stub-classes.dex"):
                    raise ValueError("标准与兼容资源的 Stub DEX 不一致")
                for abi in ("arm64-v8a", "x86", "x86_64"):
                    entry = f"lib/{abi}/libmocikashield.so"
                    if archive.read(entry) != reference.read(entry):
                        raise ValueError(f"兼容资源的 {abi} 与标准资源不一致")
    return {"binding_id": environment["STUB_BINDING_ID"], "bridge": bridge,
            "dex_sha256": hashlib.sha256(dex).hexdigest(), "native_surface": native_surface}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    rules = commands.add_parser("rules")
    rules.add_argument("source", type=Path)
    rules.add_argument("output", type=Path)
    environment = commands.add_parser("env")
    environment.add_argument("mapping", type=Path)
    check = commands.add_parser("audit")
    check.add_argument("mapping", type=Path)
    check.add_argument("resource", type=Path)
    check.add_argument("--standard", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "rules":
            transformed = prepare_rules(args.source.read_text(encoding="utf-8"))
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(transformed, encoding="utf-8")
        elif args.command == "env":
            bindings = binding_environment(parse_mapping(args.mapping.read_text(encoding="utf-8-sig")))
            for key, value in bindings.items():
                print(f"{key}={value}")
        else:
            print(json.dumps(audit(args.mapping, args.resource, args.standard), ensure_ascii=False))
        return 0
    except (OSError, ValueError, KeyError, zipfile.BadZipFile) as error:
        print(f"壳绑定检查失败：{error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
