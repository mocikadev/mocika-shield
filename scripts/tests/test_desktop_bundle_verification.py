"""实际产物验收脚本的隔离测试，不冒充系统加载器／安装器验证。"""
import importlib.util
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("desktop_bundle_verification", ROOT / "scripts/verify_desktop_bundle.py")
VERIFY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFY)


class DesktopBundleVerificationTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.repo = self.root / "repo"
        self.package = self.root / "中文 桌面包"
        self.package.mkdir()
        config = self.repo / "apps/shield-gui/src-tauri/tauri.conf.json"
        config.parent.mkdir(parents=True)
        config.write_text('{"version":"1.4.3", "productName":"MocikaShield"}')

    def layout(self, platform):
        binary, gui, resources = {
            "linux": ("usr/bin/mocika-shield-cli", "usr/bin/mocika-shield", "usr/lib/MocikaShield"),
            "windows": ("mocika-shield-cli.exe", "mocika-shield.exe", "."),
            "macos": ("Contents/MacOS/shield", "Contents/MacOS/mocika-shield", "Contents/Resources"),
        }[platform]
        for name in [binary, gui]:
            path = self.package / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"synthetic-binary")
        base = self.package / resources
        for target, source in [
            ("tools/apktool.jar", "tools/apktool_3.0.1.jar"),
            ("tools/apksigner.jar", "tools/apksigner.jar"),
            ("tools/java-adapter.jar", "tools/java-adapter/build/java-adapter.jar"),
            ("resources/resources.zip", "shield-stub/build/outputs/resources/resources.zip"),
            ("resources/resources-api19.zip", "shield-stub/build/outputs/resources/resources-api19.zip"),
        ]:
            original, copied = self.repo / source, base / target
            original.parent.mkdir(parents=True, exist_ok=True)
            copied.parent.mkdir(parents=True, exist_ok=True)
            original.write_bytes(source.encode())
            shutil.copyfile(original, copied)
        return self.package / binary, base

    def process(self, args, **options):
        output = b"shield 1.4.3\n"
        if args[1:] == ["doctor", "--json"]:
            output = b'{"status":"ready", "tool_version":"1.4.3"}'
        return subprocess.CompletedProcess(args, 0, output, b"")

    def test_三平台实际布局均校验资源摘要及入口(self):
        for platform in ["linux", "windows", "macos"]:
            binary, _ = self.layout(platform)
            with mock.patch.object(VERIFY.subprocess, "run", side_effect=self.process) as run:
                self.assertEqual(VERIFY.verify(self.package, platform, self.repo), binary)
                self.assertEqual(run.call_count, 4)
                self.assertTrue(all(call.kwargs["timeout"] == 60 for call in run.call_args_list))

    def test_缺失错配和技能残留不执行命令(self):
        _, base = self.layout("linux")
        jar = base / "tools/java-adapter.jar"
        content = jar.read_bytes()
        with mock.patch.object(VERIFY.subprocess, "run") as run:
            jar.unlink()
            with self.assertRaises(ValueError):
                VERIFY.verify(self.package, "linux", self.repo)
            jar.write_bytes(b"stale")
            with self.assertRaises(ValueError):
                VERIFY.verify(self.package, "linux", self.repo)
            jar.write_bytes(content)
            (base / "skills").mkdir()
            with self.assertRaises(ValueError):
                VERIFY.verify(self.package, "linux", self.repo)
            run.assert_not_called()

    def test_版本和依赖不符以及执行修改包均拒绝(self):
        binary, _ = self.layout("windows")
        with mock.patch.object(VERIFY.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, b"shield 0.0.0", b"")):
            with self.assertRaises(ValueError):
                VERIFY.verify(self.package, "windows", self.repo)
        def blocked(args, **options):
            if args[1:] == ["doctor", "--json"]:
                return subprocess.CompletedProcess(args, 0, b'{"status":"blocked", "tool_version":"1.4.3"}', b"")
            return self.process(args, **options)
        with mock.patch.object(VERIFY.subprocess, "run", side_effect=blocked):
            with self.assertRaises(ValueError):
                VERIFY.verify(self.package, "windows", self.repo)
        def modified(args, **options):
            binary.write_bytes(b"changed")
            return self.process(args, **options)
        with mock.patch.object(VERIFY.subprocess, "run", side_effect=modified):
            with self.assertRaises(ValueError):
                VERIFY.verify(self.package, "windows", self.repo)

    def test_产物专项仅手动触发且不构建壳或上传测试资料(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        job = workflow.split("  agent-bundles:\n", 1)[1].split("  agent-windows-processes:\n", 1)[0]
        self.assertIn("github.event_name == 'workflow_dispatch' && inputs.validation_scope == 'agent_bundles'", job)
        self.assertIn("os: [ubuntu-22.04, windows-2025]", job)
        release = (ROOT / ".github/workflows/release.yml").read_text()
        self.assertIn("runs-on: ubuntu-22.04", release)
        self.assertIn("runs-on: windows-2025", release)
        self.assertIn("scripts/prepare_desktop_cli.py", job)
        self.assertIn("scripts/extract_protection_test_resources.py", job)
        self.assertIn("scripts/build_protection_test_apk.py", job)
        self.assertIn("dpkg-deb -x", job)
        self.assertIn("--appimage-extract", job)
        self.assertIn("/S /D=$destination", job)
        self.assertIn("--test desktop_bundle installed::\n          -- --ignored", job)
        for forbidden in ["cargo-ndk", "build-stub", "gradlew", "gh release create", "gh release upload", "Downloads", "ClassInRepo"]:
            self.assertNotIn(forbidden, job)
        upload = job.split("uses: actions/upload-artifact@v7", 1)[1]
        self.assertEqual(re.findall(r"^            (target/.+)$", upload, re.M), [
            "target/release/bundle/deb/*.deb", "target/release/bundle/appimage/*.AppImage", "target/release/bundle/nsis/*.exe",
        ])
        self.assertIn("inputs.validation_scope == 'agent_bundles' && '-agent-bundles'", workflow)

    def test_原生签名专项不误跑要求双壳和真实DEX的加固测试(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        signing = workflow.split("  agent-platforms:\n", 1)[1].split("  agent-bundles:\n", 1)[0]
        self.assertIn("--skip 真实共享规划加固签名与绑定验收保持同一会话", signing)
        self.assertIn("--skip ordinary::", signing)
        self.assertIn("tests/fixtures/agent-signing/AndroidManifest.xml", signing)
        self.assertNotIn("build-stub", signing)
        defaults = workflow.split("  windows-protection:\n", 1)[1].split("  windows-paths:\n", 1)[0]
        self.assertIn("--test agent_desktop ordinary::", defaults)
        bundle = workflow.split("  agent-bundles:\n", 1)[1].split("  agent-windows-processes:\n", 1)[0]
        self.assertIn("scripts/build_protection_test_apk.py", bundle)
        self.assertIn("--test desktop_bundle installed::", bundle)
