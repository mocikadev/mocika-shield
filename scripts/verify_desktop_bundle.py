#!/usr/bin/env python3
"""核对实际解包／安装后的桌面资源与命令；不启动 GUI 或读取用户资料。"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def verify(package, platform, repository=ROOT):
    config = json.loads((repository / "apps/shield-gui/src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
    if platform == "linux":
        binary = package / "usr/bin/mocika-shield-cli"
        gui = package / "usr/bin/mocika-shield"
        resources = package / "usr/lib" / config["productName"]
    elif platform == "windows":
        binary, gui, resources = package / "mocika-shield-cli.exe", package / "mocika-shield.exe", package
    elif platform == "macos":
        binary, gui = package / "Contents/MacOS/shield", package / "Contents/MacOS/mocika-shield"
        resources = package / "Contents/Resources"
    else:
        raise ValueError("未知的桌面包平台")
    files = {
        resources / "tools/apktool.jar": repository / "tools/apktool_3.0.1.jar",
        resources / "tools/apksigner.jar": repository / "tools/apksigner.jar",
        resources / "tools/java-adapter.jar": repository / "tools/java-adapter/build/java-adapter.jar",
        resources / "resources/resources.zip": repository / "shield-stub/build/outputs/resources/resources.zip",
        resources / "resources/resources-api19.zip": repository / "shield-stub/build/outputs/resources/resources-api19.zip",
    }
    for path in [binary, gui, *files]:
        if path.is_symlink() or not path.is_file() or path.stat().st_size == 0:
            raise ValueError("桌面包缺少有效命令或资源")
    for path, source in files.items():
        if digest(path) != digest(source):
            raise ValueError("桌面包实际资源与本次构建不一致")
    for path in [resources / "skills", resources / "integrations/skills"]:
        if path.exists() or path.is_symlink():
            raise ValueError("桌面包含不应共包的 Skill")
    before = {path: digest(path) for path in [binary, gui, *files]}
    for args in [["--version"], ["agent", "--help"], ["mcp", "serve", "--help"], ["doctor", "--json"]]:
        result = subprocess.run([str(binary), *args], cwd=package, check=True, capture_output=True, timeout=60)
        if args == ["--version"] and result.stdout.decode("utf-8").strip() != "shield " + config["version"]:
            raise ValueError("实际共包 CLI 版本不一致")
        if args == ["doctor", "--json"]:
            value = json.loads(result.stdout)
            if value.get("status") != "ready" or value.get("tool_version") != config["version"]:
                raise ValueError("实际共包 CLI 依赖未就绪")
    if before != {path: digest(path) for path in before}:
        raise ValueError("只读验收改变了桌面包")
    return binary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", type=Path)
    parser.add_argument("--platform", required=True, choices=["linux", "windows", "macos"])
    args = parser.parse_args()
    binary = verify(args.package.resolve(), args.platform)
    print("实际桌面包资源、版本、入口及依赖核对通过：" + str(binary))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.SubprocessError):
        print("实际桌面包验收失败，请核对安装布局、构建资源和本机 Java。", file=sys.stderr)
        sys.exit(1)
