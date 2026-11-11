"""桌面共包准备器的隔离编排测试；不冒充 Linux／Windows 原生安装包验收。"""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("desktop_cli_bundle", ROOT / "scripts/prepare_desktop_cli.py")
BUILDER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BUILDER)


def binary_bytes(target):
    header = bytearray(64)
    if target == BUILDER.TARGETS["linux"]:
        header[:6] = b"\x7fELF\x02\x01"
        struct.pack_into("<H", header, 18, 62)
    else:
        header[:2] = b"MZ"
        struct.pack_into("<I", header, 60, 64)
        header.extend(b"PE\x00\x00\x64\x86")
    return header


class DesktopCliBundleTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name) / "中文 项目"
        config = self.root / "apps/shield-gui/src-tauri/tauri.conf.json"
        config.parent.mkdir(parents=True)
        config.write_text('{"version":"1.4.3"}')
        self.calls = []
        self.version = b"shield 1.4.3\n"

    def run_process(self, args, **options):
        self.calls.append(args)
        if args[:2] == ["cargo", "build"]:
            target = args[args.index("--target") + 1]
            profile = "release" if "--release" in args else "debug"
            name = "shield.exe" if target.endswith("msvc") else "shield"
            path = self.root / "target" / target / profile / name
            path.parent.mkdir(parents=True)
            path.write_bytes(binary_bytes(target))
            path.chmod(0o755)
        return subprocess.CompletedProcess(args, 0, self.version, b"")

    def prepare(self, target, debug=False):
        with mock.patch.dict(os.environ, {"CARGO_TARGET_DIR": ""}), mock.patch.object(BUILDER.subprocess, "run", side_effect=self.run_process):
            return BUILDER.prepare(self.root, target, debug)

    def test_两平台各配置锁定目标与入口且仅准备一份二进制(self):
        for target in BUILDER.TARGETS.values():
            for debug in [False, True]:
                with self.subTest(target=target, debug=debug):
                    self.calls.clear()
                    config = self.prepare(target, debug)
                    self.assertTrue(self.calls[0][1].endswith("check_resource_version.py"))
                    self.assertTrue(self.calls[1][1].endswith("build_java_adapter.py"))
                    build = self.calls[2]
                    self.assertIn("--locked", build)
                    self.assertIn("mcp-server", build)
                    self.assertEqual("--release" in build, not debug)
                    self.assertEqual([call[1:] for call in self.calls[3:]], [["--version"], ["agent", "--help"], ["mcp", "serve", "--help"]])
                    value = json.loads(config.read_text())
                    self.assertEqual(value, {"bundle": {"externalBin": [str(self.root / "target/desktop-cli/mocika-shield-cli")]}})
                    suffix = ".exe" if target.endswith("msvc") else ""
                    copied = config.parent / ("mocika-shield-cli-" + target + suffix)
                    self.assertEqual(copied.read_bytes(), binary_bytes(target))

    def test_版本错配和工具失败不发布共包配置(self):
        self.version = b"shield 0.0.0\n"
        with self.assertRaises(ValueError):
            self.prepare(BUILDER.TARGETS["linux"])
        self.assertFalse((self.root / "target/desktop-cli/tauri-sidecar.json").exists())
        with mock.patch.dict(os.environ, {"CARGO_TARGET_DIR": ""}), mock.patch.object(BUILDER.subprocess, "run", side_effect=subprocess.CalledProcessError(1, "构建器")):
            with self.assertRaises(subprocess.CalledProcessError):
                BUILDER.prepare(self.root, BUILDER.TARGETS["win32"])
        self.assertFalse((self.root / "target/desktop-cli/tauri-sidecar.json").exists())

    def test_拒绝非目标架构及错误文件头(self):
        with self.assertRaises(ValueError):
            BUILDER.prepare(self.root, "aarch64-unknown-linux-gnu")
        path = self.root / "wrong"
        for content in [b"", b"#!/bin/sh", binary_bytes(BUILDER.TARGETS["win32"])]:
            path.write_bytes(content)
            with self.assertRaises(ValueError):
                BUILDER.verify_binary(path, BUILDER.TARGETS["linux"])
        path.write_bytes(binary_bytes(BUILDER.TARGETS["linux"]))
        with self.assertRaises(ValueError):
            BUILDER.verify_binary(path, BUILDER.TARGETS["win32"])

    def test_自定义输出目录拒绝且不启动构建(self):
        with mock.patch.dict(os.environ, {"CARGO_TARGET_DIR": "外部目录"}), mock.patch.object(BUILDER.subprocess, "run") as run:
            with self.assertRaises(ValueError):
                BUILDER.prepare(self.root, BUILDER.TARGETS["linux"])
            run.assert_not_called()

    def test_非UTF8管道下真实入口保持中文提示及退出码(self):
        script = self.root / "scripts/prepare_desktop_cli.py"
        script.parent.mkdir()
        shutil.copyfile(ROOT / "scripts/prepare_desktop_cli.py", script)
        target = BUILDER.TARGETS["win32"]
        binary = self.root / "target" / target / "release/shield.exe"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(binary_bytes(target))
        # 运行真实命令入口与文件检查，只替代外部构建工具，不需要 Windows 或 JDK。
        harness = '''
import argparse, json, pathlib, runpy, shutil, struct, subprocess, sys, tempfile
from unittest import mock
script, scenario = sys.argv[1:]
sys.argv = [script] + (["--help"] if scenario == "help" else [])
def run(args, **options):
    if args[0] == "rustc":
        return subprocess.CompletedProcess(args, 0, "x86_64-pc-windows-msvc\\n", "")
    if args[0] == "cargo" and scenario == "failure":
        raise subprocess.CalledProcessError(7, "中文构建工具")
    output = b"shield 1.4.3\\n" if args[1:] == ["--version"] else b""
    return subprocess.CompletedProcess(args, 0, output, b"")
with mock.patch.object(sys, "platform", "win32"), mock.patch.object(subprocess, "run", side_effect=run):
    runpy.run_path(script, run_name="__main__")
'''
        env = dict(os.environ, PYTHONIOENCODING="cp1252:strict", PYTHONUTF8="0", CARGO_TARGET_DIR="")
        for scenario in ("success", "failure", "help"):
            with self.subTest(scenario=scenario):
                result = subprocess.run([sys.executable, "-c", harness, str(script), scenario],
                                        env=env, capture_output=True)
                self.assertEqual(result.returncode, 1 if scenario == "failure" else 0, result.stderr)
                output = (result.stderr if scenario == "failure" else result.stdout).decode("utf-8")
                expected = {"success": "已准备 GUI 内 CLI：", "failure": "准备桌面 CLI 失败：", "help": "准备与 Tauri"}
                self.assertIn(expected[scenario], output)
                self.assertNotIn("Traceback", output)
                if scenario == "success":
                    self.assertIn("中文 项目", output)

    def test_现有构建入口均使用生成配置且不修改默认Tauri配置(self):
        config = json.loads((ROOT / "apps/shield-gui/src-tauri/tauri.conf.json").read_text())
        self.assertNotIn("externalBin", config["bundle"])
        for name in ["release-linux.sh", "release-windows.ps1"]:
            text = (ROOT / "scripts" / name).read_text()
            self.assertIn("prepare_desktop_cli.py", text)
            self.assertIn("tauri-sidecar.json", text)
        text = (ROOT / "Makefile").read_text()
        self.assertIn("--no-bundle --config ../../target/desktop-cli/tauri-sidecar.json", text)
        ci = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertIn("--debug --config ../../target/desktop-cli/tauri-sidecar.json", ci)

    def test_Linux资源目录与Tauri产品名一致而非Cargo包名(self):
        config = json.loads((ROOT / "apps/shield-gui/src-tauri/tauri.conf.json").read_text())
        product = config["productName"]
        core = (ROOT / "crates/shield-core/src/bundle_resources.rs").read_text()
        gui = (ROOT / "apps/shield-gui/src-tauri/src/app_paths.rs").read_text()
        fixture = (ROOT / "apps/shield-cli/tests/desktop_bundle.rs").read_text()
        extraction = (ROOT / "scripts/extract_protection_test_resources.py").read_text()
        self.assertIn(f'join("../lib/{product}")', core)
        self.assertIn(f'join("usr/lib/{product}")', gui)
        self.assertIn(f'join("usr/lib/{product}")', fixture)
        self.assertIn(f'usr/lib/{product}/resources/resources.zip', extraction)
