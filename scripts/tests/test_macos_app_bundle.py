"""共包脚本的轻量进程夹具；不构建真实应用，也不冒充平台签名验收。"""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
import zipfile


ROOT = Path(__file__).resolve().parents[2]
TOOLS = r'''
import json, os, pathlib, shutil, sys
root = pathlib.Path(os.environ["BUNDLE_FIXTURE_ROOT"])
name = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
with (root / "calls.jsonl").open("a") as log:
    log.write(json.dumps([name, *args]) + "\n")
if name == "uname":
    print("arm64")
elif name == "cargo" and args[0] == "build":
    target = args[args.index("--target") + 1] if "--target" in args else ""
    binary = root / "target" / target / "release/shield"
    binary.parent.mkdir(parents=True, exist_ok=True)
    binary.write_text("#!/bin/sh\nexit 0\n")
    binary.chmod(0o755)
elif name == "cargo" and args[0] == "tauri":
    target = "universal-apple-darwin" if "--target" in args else ""
    app = root / "target" / target / "release/bundle/macos/MocikaShield.app/Contents"
    (app / "MacOS").mkdir(parents=True, exist_ok=True)
    (app / "MacOS/mocika-shield").write_bytes(b"GUI")
    for source, dest in [
        ("tools/apktool_3.0.1.jar", "tools/apktool.jar"),
        ("tools/apksigner.jar", "tools/apksigner.jar"),
        ("tools/java-adapter/build/java-adapter.jar", "tools/java-adapter.jar"),
        ("shield-stub/build/outputs/resources/resources.zip", "resources/resources.zip"),
        ("shield-stub/build/outputs/resources/resources-api19.zip", "resources/resources-api19.zip"),
    ]:
        path = app / "Resources" / dest
        path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(root / source, path)
    if os.environ.get("BUNDLE_FIXTURE_STALE"):
        (app / "Resources/tools/apksigner.jar").write_bytes(b"stale")
    if os.environ.get("BUNDLE_FIXTURE_STALE_ADAPTER"):
        (app / "Resources/tools/java-adapter.jar").write_bytes(b"stale-adapter")
elif name == "lipo":
    if "-create" in args:
        dest = pathlib.Path(args[args.index("-output") + 1])
        shutil.copyfile(args[args.index("-create") + 1], dest)
        dest.chmod(0o755)
    elif os.environ.get("BUNDLE_FIXTURE_BAD_ARCH"):
        sys.exit(4)
elif name == "codesign":
    if os.environ.get("BUNDLE_FIXTURE_BAD_SIGN"):
        sys.exit(5)
    if "--deep" in args:
        assert (pathlib.Path(args[-1]) / "Contents/MacOS/shield").is_file()
        assert not (pathlib.Path(args[-1]) / "Contents/Resources/skills").exists()
        assert not (pathlib.Path(args[-1]) / "Contents/Resources/integrations/skills").exists()
else:
    raise AssertionError("未知夹具调用")
'''


@unittest.skipUnless(os.name == "posix" and shutil.which("bash"), "需要类 Unix 环境和 Bash 执行脚本夹具")
class MacosAppBundleTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="共包 空格")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        for directory in ["scripts", "tools", "apps/shield-cli", "apps/shield-gui/src-tauri", "bin"]:
            (self.root / directory).mkdir(parents=True)
        for name in ["build-macos-app.sh", "check_resource_version.py"]:
            shutil.copyfile(ROOT / "scripts" / name, self.root / "scripts" / name)
        # 只验证构建编排；真实编译器／字节码由独立构建器专项验证。
        (self.root / "scripts/build_java_adapter.py").write_text(
            'import json, os, pathlib, sys\n'
            'root = pathlib.Path(os.environ["BUNDLE_FIXTURE_ROOT"])\n'
            'with (root / "calls.jsonl").open("a") as log: log.write(json.dumps(["java-adapter"]) + "\\n")\n'
            'if os.environ.get("BUNDLE_FIXTURE_ADAPTER_FAILURE"): sys.exit(6)\n'
            'output = root / "tools/java-adapter/build/java-adapter.jar"\n'
            'output.parent.mkdir(parents=True, exist_ok=True)\n'
            'output.write_bytes(b"adapter-fixture")\n', encoding="utf-8")
        (self.root / "apps/shield-cli/Cargo.toml").write_text('version = "1.4.3"\n')
        (self.root / "apps/shield-gui/src-tauri/tauri.conf.json").write_text('{"version":"1.4.3"}')
        for name in ["apktool_3.0.1.jar", "apksigner.jar"]:
            (self.root / "tools" / name).write_bytes(name.encode())
        self.resources = self.root / "shield-stub/build/outputs/resources"
        self.resources.mkdir(parents=True)
        for name in ["resources.zip", "resources-api19.zip"]:
            with zipfile.ZipFile(self.resources / name, "w") as archive:
                archive.writestr("metadata.json", '{"version":"1.4.3"}')
        for name in ["cargo", "lipo", "codesign", "uname"]:
            path = self.root / "bin" / name
            path.write_text(f"#!{sys.executable}\n" + TOOLS)
            path.chmod(0o755)
        self.env = dict(os.environ, PATH=str(self.root / "bin") + os.pathsep + os.environ["PATH"],
                        BUNDLE_FIXTURE_ROOT=str(self.root))
        self.env.pop("CARGO_TARGET_DIR", None)

    def run_bundle(self, *args):
        # 仅模拟平台变量，所有构建/签名操作都由临时夹具接管。
        return subprocess.run(["bash", "-c", 'OSTYPE=darwin; source "$0" "$@"',
                               str(self.root / "scripts/build-macos-app.sh"), *args],
                              env=self.env, capture_output=True, timeout=15)

    def calls(self):
        path = self.root / "calls.jsonl"
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def test_单架构先共包再签名并启用开发命令(self):
        result = self.run_bundle()
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        calls = self.calls()
        builds = [args for name, *args in calls if name == "cargo" and args[0] == "build"]
        self.assertEqual(len(builds), 1)
        self.assertIn("mcp-server", builds[0])
        self.assertIn("--locked", builds[0])
        signatures = [args for name, *args in calls if name == "codesign"]
        self.assertEqual(len(signatures), 3)
        self.assertTrue(signatures[0][-1].endswith("Contents/MacOS/shield"))
        self.assertIn("--verify", signatures[-1])
        self.assertTrue((self.root / "target/release/bundle/macos/MocikaShield.app/Contents/MacOS/shield").is_file())
        adapter = self.root / "target/release/bundle/macos/MocikaShield.app/Contents/Resources/tools/java-adapter.jar"
        self.assertEqual(adapter.read_bytes(), b"adapter-fixture")
        self.assertEqual(calls[0], ["java-adapter"])

    def test_通用架构分别编译后合并且核对两个可执行文件(self):
        result = self.run_bundle("universal")
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        calls = self.calls()
        builds = [args for name, *args in calls if name == "cargo" and args[0] == "build"]
        self.assertEqual(len(builds), 2)
        self.assertTrue(all("mcp-server" in args for args in builds))
        self.assertIn("aarch64-apple-darwin", builds[0])
        self.assertIn("x86_64-apple-darwin", builds[1])
        checks = [args for name, *args in calls if name == "lipo" and "-verify_arch" in args]
        self.assertEqual(len(checks), 2)
        self.assertTrue(all(args[-2:] == ["arm64", "x86_64"] for args in checks))

    def test_缺资源或版本不匹配时不启动编译(self):
        for defect in ["缺工控包", "版本错配"]:
            with self.subTest(defect=defect):
                if defect == "缺工控包":
                    (self.resources / "resources-api19.zip").unlink()
                else:
                    with zipfile.ZipFile(self.resources / "resources-api19.zip", "w") as archive:
                        archive.writestr("metadata.json", '{"version":"0.0.0"}')
                self.assertNotEqual(self.run_bundle().returncode, 0)
                self.assertEqual(self.calls(), [])

    def test_GUI版本错配不启动编译(self):
        (self.root / "apps/shield-gui/src-tauri/tauri.conf.json").write_text('{"version":"0.0.0"}')
        self.assertNotEqual(self.run_bundle().returncode, 0)
        self.assertEqual(self.calls(), [])

    def test_应用构建不依赖技能源码也不装入技能(self):
        self.assertFalse((self.root / "skills").exists())
        self.assertFalse((self.root / "integrations").exists())
        result = self.run_bundle()
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        app = self.root / "target/release/bundle/macos/MocikaShield.app"
        self.assertEqual(list(app.rglob("SKILL.md")), [])

    def test_旧技能残留不会进入最终签名(self):
        old = self.root / "target/release/bundle/macos/MocikaShield.app/Contents/Resources/integrations/skills"
        old.mkdir(parents=True)
        result = self.run_bundle()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(call[0] == "codesign" for call in self.calls()))

    def test_架构或实际资源错配不进入签名(self):
        for defect in ["BUNDLE_FIXTURE_BAD_ARCH", "BUNDLE_FIXTURE_STALE", "BUNDLE_FIXTURE_STALE_ADAPTER"]:
            with self.subTest(defect=defect):
                self.env[defect] = "1"
                self.assertNotEqual(self.run_bundle().returncode, 0)
                self.assertFalse(any(call[0] == "codesign" for call in self.calls()))
                self.env.pop(defect)
                (self.root / "calls.jsonl").unlink()

    def test_辅助资源生成失败不进入构建或签名(self):
        self.env["BUNDLE_FIXTURE_ADAPTER_FAILURE"] = "1"
        result = self.run_bundle()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.calls(), [["java-adapter"]])

    def test_签名失败不报告成功(self):
        self.env["BUNDLE_FIXTURE_BAD_SIGN"] = "1"
        result = self.run_bundle()
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("已完成 GUI".encode(), result.stdout)

    def test_非法参数和自定义构建目录失败关闭(self):
        self.assertEqual(self.run_bundle("unexpected").returncode, 2)
        self.env["CARGO_TARGET_DIR"] = str(self.root / "other-target")
        self.assertNotEqual(self.run_bundle().returncode, 0)
        self.assertEqual(self.calls(), [])

    def test_发布跳过独立压缩包不跳过应用共包(self):
        text = (ROOT / "scripts/release-macos.sh").read_text()
        main = re.search(r"(?ms)^main\(\) \{.*?^\}", text).group()
        names = ["check_env", "build_stub", "prepare_dirs", "build_gui", "package_cli",
                 "collect_gui", "generate_checksums", "show_results", "info"]
        functions = "\n".join(f'{name}() {{ echo "调用:{name}"; }}' for name in names)
        for skip in ("0", "1"):
            env = dict(self.env, ROOT=str(self.root), VERSION="1.4.3", SKIP_CLI_RELEASE=skip)
            result = subprocess.run(["bash", "-c", "set -eu\n" + functions + "\n" + main + "\nmain"],
                                    env=env, capture_output=True, timeout=5)
            self.assertEqual(result.returncode, 0, result.stderr.decode())
            self.assertEqual(result.stdout.count("调用:build_gui".encode()), 1)
            self.assertEqual("调用:package_cli".encode() in result.stdout, skip == "0")

    def test_非macOS不能进入构建(self):
        result = subprocess.run(["bash", "-c", 'OSTYPE=linux-gnu; source "$0"',
                                 str(self.root / "scripts/build-macos-app.sh")],
                                env=self.env, capture_output=True, timeout=5)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.calls(), [])


if __name__ == "__main__":
    unittest.main()
