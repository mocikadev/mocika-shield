#!/usr/bin/env python3
"""构建或校验 Java 8 适配资源；不联网、不修改宿主 Java、不运行于 Cargo 构建。"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "tools/java-adapter/src"
OUTPUT = ROOT / "tools/java-adapter/build/java-adapter.jar"
MAIN = "dev.mocika.shield.tools.AdapterMain"
PREFIX = "dev/mocika/shield/tools/"
METADATA = "META-INF/adapter.json"
MAX_BYTES = 256 * 1024
PROTOCOL = 4


def application_version():
    text = (ROOT / "crates/shield-core/Cargo.toml").read_text(encoding="utf-8")
    return re.search(r'^version = "([^"]+)"$', text, re.M).group(1)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def source_files(source):
    files = {}
    for path in sorted(source.rglob("*.java")):
        name = path.relative_to(source).as_posix()
        if path.is_symlink() or not name.startswith(PREFIX) or not name.isascii():
            raise ValueError("适配源码路径无效")
        if path.stat().st_size > MAX_BYTES:
            raise ValueError("适配源码超出预算")
        files[name] = path.read_bytes()
    if not files or len(files) > 16 or sum(map(len, files.values())) > MAX_BYTES:
        raise ValueError("适配源码缺失或超出预算")
    return files


def manifest(version):
    if len(version) > 48 or not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+[A-Za-z0-9.+-]*", version):
        raise ValueError("适配资源版本无效")
    return (f"Manifest-Version: 1.0\r\nMain-Class: {MAIN}\r\n"
            f"Implementation-Version: {version}\r\nMocika-Adapter-Protocol: {PROTOCOL}\r\n\r\n").encode("ascii")


def validate(path, version, sources):
    if path.stat().st_size > MAX_BYTES:
        raise ValueError("适配资源超出预算")
    with zipfile.ZipFile(path) as archive:
        infos = archive.infolist()
        names = [item.filename for item in infos]
        if len(names) != len(set(names)) or len(names) > 20:
            raise ValueError("适配资源包含重复或过多条目")
        if any(item.file_size > MAX_BYTES for item in infos) or sum(item.file_size for item in infos) > MAX_BYTES:
            raise ValueError("适配资源解压内容超出预算")
        if archive.read("META-INF/MANIFEST.MF") != manifest(version):
            raise ValueError("适配资源 Manifest 不兼容")
        metadata = json.loads(archive.read(METADATA).decode("utf-8"))
        expected_sources = [{"name": name, "sha256": digest(data)} for name, data in sources.items()]
        if not isinstance(metadata, dict) or metadata.keys() != {"protocol_version", "application_version", "main_class", "sources", "classes"}:
            raise ValueError("适配元数据字段不兼容")
        if (type(metadata["protocol_version"]) is not int or metadata["protocol_version"] != PROTOCOL or metadata["application_version"] != version
                or metadata["main_class"] != MAIN or metadata["sources"] != expected_sources):
            raise ValueError("适配资源版本或源码不一致")
        classes = metadata["classes"]
        if not isinstance(classes, list) or not 1 <= len(classes) <= 18:
            raise ValueError("适配字节码清单无效")
        expected = {"META-INF/MANIFEST.MF", METADATA}
        for item in classes:
            if not isinstance(item, dict) or item.keys() != {"name", "bytes", "sha256"}:
                raise ValueError("适配字节码字段不兼容")
            name = item["name"]
            if (not isinstance(name, str) or not name.startswith(PREFIX) or not name.endswith(".class") or not name.isascii()
                    or ".." in name or "\\" in name or name in expected or name not in names):
                raise ValueError("适配字节码条目无效")
            data = archive.read(name)
            if (len(data) < 8 or data[:8] != b"\xca\xfe\xba\xbe\0\0\0\x34"
                    or len(data) != item["bytes"] or digest(data) != item["sha256"]):
                raise ValueError("适配字节码版本或摘要无效")
            expected.add(name)
        if any(PREFIX + name + ".class" not in expected for name in ("AdapterMain", "Request", "Reply", "CertificateReader", "CertificateCreator", "JarLauncher")) or set(names) != expected:
            raise ValueError("适配资源缺失或夹带额外文件")
    return digest(path.read_bytes())


def build(source, output, version, javac):
    sources = source_files(source)
    header = manifest(version)
    result = subprocess.run([str(javac), "-version"], capture_output=True, timeout=30, check=True)
    if not re.search(rb"(?m)^javac 17(?:\.|\s)", result.stdout + result.stderr):
        raise ValueError("构建需要 JDK 17；应用运行仍支持 Java 8")
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="java-adapter-", dir=output.parent) as directory:
        stage = Path(directory)
        for name, data in sources.items():
            target = stage / "src" / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
        (stage / "classes").mkdir()
        # 所有参数相对且为 ASCII，不把中文源码／输出绝对路径传给 javac 启动器。
        subprocess.run([str(javac), "--release", "8", "-encoding", "UTF-8", "-g:none", "-d", "classes",
                        *["src/" + name for name in sources]], cwd=stage, capture_output=True, timeout=60, check=True)
        entries = {"META-INF/MANIFEST.MF": header}
        classes = []
        for path in sorted((stage / "classes").rglob("*.class")):
            name = path.relative_to(stage / "classes").as_posix()
            data = path.read_bytes()
            entries[name] = data
            classes.append({"name": name, "bytes": len(data), "sha256": digest(data)})
        metadata = {"protocol_version": PROTOCOL, "application_version": version, "main_class": MAIN,
                    "sources": [{"name": name, "sha256": digest(data)} for name, data in sources.items()],
                    "classes": classes}
        entries[METADATA] = json.dumps(metadata, sort_keys=True, separators=(",", ":")).encode("utf-8")
        candidate = stage / "candidate.jar"
        with zipfile.ZipFile(candidate, "w", compression=zipfile.ZIP_STORED) as archive:
            for name, data in sorted(entries.items()):
                info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                info.create_system = 3
                info.external_attr = 0o100644 << 16
                archive.writestr(info, data)
        checksum = validate(candidate, version, sources)
        os.replace(candidate, output)
    return checksum


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="仅检查已生成资源")
    parser.add_argument("--javac", type=Path, help="显式构建编译器，不修改全局环境")
    args = parser.parse_args()
    try:
        if args.check:
            checksum = validate(OUTPUT, application_version(), source_files(SOURCE))
        else:
            javac = args.javac or shutil.which("javac")
            if not javac:
                raise ValueError("缺少构建编译器 javac（需要 JDK 17）")
            checksum = build(SOURCE, OUTPUT, application_version(), Path(javac).resolve())
    except (OSError, ValueError, KeyError, TypeError, zipfile.BadZipFile, subprocess.SubprocessError):
        # 编译器可能把外来 JVM 环境变量输出到 stderr，不能原样回显。
        print("Java 适配资源构建／校验失败，请检查 JDK 17、源码和资源版本；未改宿主环境。", file=sys.stderr)
        return 1
    print(f"Java 适配资源校验通过，SHA-256：{checksum}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
