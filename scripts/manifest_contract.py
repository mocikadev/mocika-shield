#!/usr/bin/env python3
"""校验单一 Manifest 字段定义、生成投影并审计资源消费契约。"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "shield-stub/runtime-manifest-contract.json"
RUST = ROOT / "crates/shield-core/src/protect/manifest_keys.rs"
JAVA = ROOT / "shield-stub/src/main/java/dev/mocika/shield/loader/ManifestKeys.java"
FIELDS = ("application", "policy", "factory", "cache_schema", "cache_count", "cache_root", "payload_bytes")
PROTOCOLS = ("legacy_standard", "legacy_memory", "standard", "memory")


def unique_object(pairs: list) -> dict:
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("Manifest 契约定义含重复字段")
        result[key] = value
    return result


def load_contract(source: Path = SOURCE) -> dict:
    contract = json.loads(source.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
    if not isinstance(contract, dict) or set(contract) != {"protocols", "fields"}:
        raise ValueError("Manifest 契约顶层字段不匹配")
    protocols, fields = contract["protocols"], contract["fields"]
    if not isinstance(protocols, dict) or not isinstance(fields, dict) or set(protocols) != set(PROTOCOLS) or set(fields) != set(FIELDS):
        raise ValueError("Manifest 契约协议或字段集合不完整")
    if any(type(value) is not int or value <= 0 for value in protocols.values()):
        raise ValueError("Manifest 契约协议必须为正整数")
    if len(set(protocols.values())) != len(PROTOCOLS):
        raise ValueError("Manifest 契约协议号重复")
    # 旧协议已公开，不允许编辑源定义时改变其含义。
    if protocols["legacy_standard"] != 2 or protocols["legacy_memory"] != 3:
        raise ValueError("已公开的旧资源协议不可重新分配")
    names = []
    for field in FIELDS:
        values = fields[field]
        if not isinstance(values, dict) or set(values) != {"legacy", "current"}:
            raise ValueError("Manifest 字段缺少新旧映射")
        for generation, name in values.items():
            if not isinstance(name, str) or not re.fullmatch(r"[A-Za-z][A-Za-z0-9_.]*", name):
                raise ValueError("Manifest 字段名称不合法")
            if generation == "current" and ("mocika" in name.lower() or name == "ORIGINAL_APPLICATION"):
                raise ValueError("当前 Manifest 字段仍含旧标识")
            names.append(name)
    if len(set(names)) != len(names):
        raise ValueError("Manifest 字段名称重复")
    return contract


def projections(contract: dict) -> dict[Path, str]:
    rust = ["// 由 scripts/manifest_contract.py 生成，请修改单一契约定义。", "", "#[derive(Debug)]", "pub(crate) struct ManifestKeys {"]
    rust += [f"    pub(crate) {field}: &'static str," for field in FIELDS]
    rust += ["}", ""]
    for generation, symbol in (("legacy", "LEGACY"), ("current", "CURRENT")):
        rust.append(f"pub(crate) const {symbol}: ManifestKeys = ManifestKeys {{")
        rust += [f'    {field}: "{contract["fields"][field][generation]}",' for field in FIELDS]
        rust += ["};", ""]
    for name in PROTOCOLS:
        rust.append(f'pub(crate) const {name.upper()}_PROTOCOL: u32 = {contract["protocols"][name]};')
    java = ["package dev.mocika.shield.loader;", "", "/** 由 scripts/manifest_contract.py 生成，只包含当前资源字段。 */", "final class ManifestKeys {"]
    java += [f'    static final String {field.upper()} = "{contract["fields"][field]["current"]}";' for field in FIELDS]
    java += ["", "    private ManifestKeys() {}", "}"]
    return {RUST: "\n".join(rust) + "\n", JAVA: "\n".join(java) + "\n"}


def check_projections(contract: dict) -> None:
    for path, expected in projections(contract).items():
        if not path.is_file() or path.read_text(encoding="utf-8") != expected:
            raise ValueError(f"Manifest 契约生成文件过期：{path.relative_to(ROOT)}")


def audit_resource(metadata: dict, strings: set[str], contract: dict | None = None) -> None:
    contract = contract or load_contract()
    if not isinstance(metadata, dict):
        raise ValueError("资源元数据必须为对象")
    protocol = metadata.get("runtime_protocol")
    memory = metadata.get("memory_dex")
    expected = contract["protocols"]["memory" if memory is True else "standard"]
    if type(protocol) is not int or type(memory) is not bool or protocol != expected:
        raise ValueError("资源协议与当前 Manifest 契约或内存能力不一致")
    for field in contract["fields"].values():
        if field["current"] not in strings or field["legacy"] in strings:
            raise ValueError("Stub DEX 的 Manifest 字段缺失或含旧字段")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("check", "generate", "standard", "memory"))
    args = parser.parse_args()
    try:
        contract = load_contract()
        if args.command == "generate":
            for path, content in projections(contract).items():
                path.write_text(content, encoding="utf-8", newline="\n")
        elif args.command == "check":
            check_projections(contract)
        else:
            check_projections(contract)
            print(contract["protocols"][args.command])
    except (ValueError, KeyError, TypeError, OSError) as error:
        parser.exit(1, f"Manifest 契约检查失败：{error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
