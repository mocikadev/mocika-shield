import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock
import zipfile

ROOT = Path(__file__).resolve().parents[2]
CLASSES = ("AdapterMain", "Request", "Reply", "CertificateReader", "CertificateCreator", "JarLauncher")
SPEC = importlib.util.spec_from_file_location("java_adapter_builder", ROOT / "scripts/build_java_adapter.py")
BUILDER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BUILDER)


class JavaAdapterBuildTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.source = self.root / "source"
        for name in CLASSES:
            path = self.source / BUILDER.PREFIX / (name + ".java")
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("// 合成源码\n", encoding="utf-8")
        self.output = self.root / "output/adapter.jar"

    def compiler(self, command, **options):
        if command[-1] == "-version":
            return subprocess.CompletedProcess(command, 0, b"javac 17.0.1\n", b"")
        self.assertIn("--release", command)
        self.assertTrue(all(str(arg).isascii() for arg in command[1:]))
        for name in CLASSES:
            path = Path(options["cwd"]) / "classes" / BUILDER.PREFIX / (name + ".class")
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"\xca\xfe\xba\xbe\0\0\0\x34")
        return subprocess.CompletedProcess(command, 0, b"", b"")

    def build(self):
        with mock.patch.object(BUILDER.subprocess, "run", side_effect=self.compiler):
            return BUILDER.build(self.source, self.output, "1.4.3", Path("javac"))

    def test_同输入打包确定且字节码清单完整(self):
        checksum = self.build()
        original = self.output.read_bytes()
        self.assertEqual(self.build(), checksum)
        self.assertEqual(self.output.read_bytes(), original)
        with zipfile.ZipFile(self.output) as archive:
            metadata = json.loads(archive.read(BUILDER.METADATA))
            self.assertEqual(len(metadata["classes"]), 6)
            self.assertEqual(len(metadata["sources"]), 6)
            self.assertTrue(all(item.date_time == (1980, 1, 1, 0, 0, 0) for item in archive.infolist()))

    def test_旧编译器不得修改已有生成物(self):
        self.output.parent.mkdir()
        self.output.write_bytes(b"previous")
        with mock.patch.object(BUILDER.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, b"javac 1.8.0\n", b"")):
            with self.assertRaises(ValueError):
                BUILDER.build(self.source, self.output, "1.4.3", Path("javac"))
        self.assertEqual(self.output.read_bytes(), b"previous")

    def test_错版和源码变化不能复用旧包(self):
        self.build()
        with self.assertRaises(ValueError):
            BUILDER.validate(self.output, "1.4.4", BUILDER.source_files(self.source))
        (self.source / BUILDER.PREFIX / "Request.java").write_text("// 修改\n", encoding="utf-8")
        with self.assertRaises(ValueError):
            BUILDER.validate(self.output, "1.4.3", BUILDER.source_files(self.source))

    def test_额外条目或损坏字节码被拒绝(self):
        for defect in ("extra.txt", BUILDER.PREFIX + "Request.class"):
            self.build()
            rewritten = self.root / "invalid.jar"
            with zipfile.ZipFile(self.output) as original, zipfile.ZipFile(rewritten, "w") as output:
                for item in original.infolist():
                    output.writestr(item, b"invalid" if item.filename == defect else original.read(item))
                if defect == "extra.txt":
                    output.writestr(defect, b"extra")
            with self.assertRaises(ValueError):
                BUILDER.validate(rewritten, "1.4.3", BUILDER.source_files(self.source))

    def test_版本不能注入清单或超出行预算(self):
        for version in ("1.4.3\r\nInjected: yes", "x", "1.4.3-" + "a" * 50):
            with self.assertRaises(ValueError):
                BUILDER.manifest(version)

    def test_显式构建交付资源但普通Cargo不编译Java(self):
        makefile = (ROOT / "Makefile").read_text()
        self.assertIn("build-java-adapter:", makefile)
        self.assertIn("$(PYTHON) scripts/build_java_adapter.py", makefile)
        self.assertIn("build-cli: build-java-adapter", makefile)
        self.assertIn("cp tools/java-adapter/build/java-adapter.jar target/lib/java-adapter.jar", makefile)
        self.assertFalse((ROOT / "crates/shield-core/build.rs").exists())

    def test_桌面和维护包共用同一辅助资源且跳过独立CLI不漏工具(self):
        config = json.loads((ROOT / "apps/shield-gui/src-tauri/tauri.conf.json").read_text())
        self.assertEqual(config["bundle"]["resources"]["../../../tools/java-adapter/build/java-adapter.jar"], "tools/java-adapter.jar")
        mac = (ROOT / "scripts/build-macos-app.sh").read_text()
        self.assertLess(mac.index('python3 "$ROOT/scripts/build_java_adapter.py"'), mac.index('TAURI_ARGS='))
        self.assertIn('cmp "$ROOT/tools/java-adapter/build/java-adapter.jar" "$APP_DIR/Contents/Resources/tools/java-adapter.jar"', mac)
        linux = (ROOT / "scripts/release-linux.sh").read_text()
        windows = (ROOT / "scripts/release-windows.ps1").read_text()
        self.assertIn('python3 scripts/prepare_desktop_cli.py', linux.split("build_gui() {", 1)[1].split("\n}", 1)[0])
        self.assertIn('scripts\\prepare_desktop_cli.py', windows.split("function Build-Gui {", 1)[1].split("\n}", 1)[0])
        prepare = (ROOT / "scripts/prepare_desktop_cli.py").read_text()
        self.assertIn('scripts/build_java_adapter.py', prepare)
        for name in ["release-linux.sh", "release-macos.sh", "release-cli.sh"]:
            content = (ROOT / "scripts" / name).read_text()
            self.assertIn('lib/java-adapter.jar', content)
        self.assertIn('$pkgDir\\lib\\java-adapter.jar', windows)
        ci = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertIn('python3 ../../scripts/prepare_desktop_cli.py --debug', ci)

    def test_只读处理器固定且服务不复制工具参数或持有上下文(self):
        self.build()
        self.assertEqual(BUILDER.PROTOCOL, 4)
        core = (ROOT / "crates/shield-core/src/keytool.rs").read_text()
        services = (ROOT / "crates/shield-services/src/certificates/keytool.rs").read_text()
        self.assertIn("legacy::aliases(path, password, kind)", core)
        self.assertIn("legacy::fingerprint(path, alias, password, kind)", core)
        self.assertIn("shield_core::keytool::query_keystore_aliases", services)
        self.assertNotIn(".args(", services)
        self.assertNotIn("JavaToolContext::new", services)
        java = (ROOT / "tools/java-adapter/src/dev/mocika/shield/tools/CertificateReader.java").read_text()
        for forbidden in (".store(", ".getKey(", "newOutputStream(", "printStackTrace("):
            self.assertNotIn(forbidden, java)
        for handler in ("CertificateReader", "JarLauncher"):
            missing = self.root / "missing-handler.jar"
            name = BUILDER.PREFIX + handler + ".class"
            with zipfile.ZipFile(self.output) as source, zipfile.ZipFile(missing, "w") as target:
                for item in source.infolist():
                    if item.filename != name:
                        target.writestr(item, source.read(item))
            with self.assertRaises(ValueError):
                BUILDER.validate(missing, "1.4.3", BUILDER.source_files(self.source))

    def test_新建工具语义下沉且测试生成器不进入正式资源(self):
        service = (ROOT / "crates/shield-services/src/certificates/creation.rs").read_text()
        self.assertNotIn(".args(", service)
        self.assertIn("generate_keystore_draft", service)
        self.assertIn("create_keystore_draft_with_context", service)
        self.assertIn("managed::publish", service)
        self.build()
        with zipfile.ZipFile(self.output) as archive:
            self.assertIn(BUILDER.PREFIX + "CertificateCreator.class", archive.namelist())
            self.assertFalse(any("Probe" in name or "ReadOnlyStores" in name for name in archive.namelist()))
