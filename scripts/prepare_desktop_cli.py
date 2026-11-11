#!/usr/bin/env python3
"""准备 Linux／Windows GUI 的原生 CLI 共包材料；不安装、不发布、不改全局环境。"""
import argparse
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {"linux": "x86_64-unknown-linux-gnu", "win32": "x86_64-pc-windows-msvc"}


def verify_binary(path, target):
    """只核对本阶段承诺的 x64 原生格式，不替代系统加载器或运行验证。"""
    with path.open("rb") as source:
        header = source.read(64)
        if target == TARGETS["linux"]:
            if len(header) < 64 or header[:6] != b"\x7fELF\x02\x01" or struct.unpack_from("<H", header, 18)[0] != 62:
                raise ValueError("CLI 不是预期的 Linux x64 可执行文件")
        else:
            if len(header) < 64 or header[:2] != b"MZ":
                raise ValueError("CLI 不是预期的 Windows 可执行文件")
            offset = struct.unpack_from("<I", header, 60)[0]
            if not 64 <= offset <= 1024 * 1024:
                raise ValueError("CLI 的 Windows 文件头位置无效")
            source.seek(offset)
            if source.read(6) != b"PE\x00\x00\x64\x86":
                raise ValueError("CLI 不是预期的 Windows x64 可执行文件")


def prepare(root, target, debug=False):
    if target not in TARGETS.values():
        raise ValueError("当前共包仅支持 Linux GNU x64／Windows MSVC x64 原生构建")
    if os.environ.get("CARGO_TARGET_DIR"):
        raise ValueError("共包固定使用仓库 target 目录，请移除本次命令的 CARGO_TARGET_DIR")
    version = json.loads((root / "apps/shield-gui/src-tauri/tauri.conf.json").read_text(encoding="utf-8"))["version"]
    resources = root / "shield-stub/build/outputs/resources"
    subprocess.run([sys.executable, str(root / "scripts/check_resource_version.py"), version,
                    str(resources / "resources.zip"), str(resources / "resources-api19.zip")], cwd=root, check=True)
    subprocess.run([sys.executable, str(root / "scripts/build_java_adapter.py")], cwd=root, check=True)
    args = ["cargo", "build", "--locked", "-p", "shield-cli", "--features", "mcp-server", "--target", target]
    if not debug:
        args.append("--release")
    subprocess.run(args, cwd=root, check=True)
    extension = ".exe" if target == TARGETS["win32"] else ""
    source = root / "target" / target / ("debug" if debug else "release") / ("shield" + extension)
    verify_binary(source, target)
    for args in [["--version"], ["agent", "--help"], ["mcp", "serve", "--help"]]:
        result = subprocess.run([str(source), *args], cwd=root, check=True, capture_output=True, timeout=30)
        if args == ["--version"] and result.stdout.decode("utf-8").strip() != "shield " + version:
            raise ValueError("GUI 与 CLI 版本不一致")
    output = root / "target/desktop-cli"
    output.mkdir(parents=True, exist_ok=True)
    # 只在全部构建和入口检查成功后更新本阶段自己的生成物；不覆盖用户材料。
    with tempfile.TemporaryDirectory(prefix="prepare-", dir=output) as directory:
        stage = Path(directory)
        binary = stage / ("mocika-shield-cli-" + target + extension)
        shutil.copy2(source, binary)
        config = stage / "tauri-sidecar.json"
        config.write_text(json.dumps({"bundle": {"externalBin": [str(output / "mocika-shield-cli")]}}), encoding="utf-8")
        os.replace(binary, output / binary.name)
        os.replace(config, output / config.name)
    return output / "tauri-sidecar.json"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--debug", action="store_true", help="准备与 Tauri --debug 配套的 CLI")
    args = parser.parse_args()
    expected = TARGETS.get(sys.platform)
    if expected is None:
        parser.error("此入口仅支持 Linux／Windows；macOS 使用 build-macos-app.sh")
    host = subprocess.run(["rustc", "--print", "host-tuple"], check=True, capture_output=True, text=True).stdout.strip()
    if host != expected:
        parser.error("当前阶段仅支持原生 x64 构建，不从宿主类型猜测交叉编译目标")
    print("已准备 GUI 内 CLI：" + str(prepare(ROOT, host, args.debug)))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        print("准备桌面 CLI 失败：" + str(error), file=sys.stderr)
        sys.exit(1)
