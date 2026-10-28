import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
import zipfile


ROOT = pathlib.Path(__file__).resolve().parents[2]
CHECKER = ROOT / "scripts/check_resource_version.py"


class ResourceVersionTests(unittest.TestCase):
    def test_非_utf8输出环境仍能报告成功和失败(self):
        for defect in (None, "版本不一致", "文件缺失"):
            with self.subTest(defect=defect), tempfile.TemporaryDirectory() as directory:
                path = pathlib.Path(directory) / "中文资源.zip"
                if defect != "文件缺失":
                    with zipfile.ZipFile(path, "w") as archive:
                        archive.writestr("metadata.json", json.dumps({
                            "version": "1.0.0" if defect else "1.4.3",
                        }))
                env = dict(os.environ, PYTHONIOENCODING="cp1252:strict", PYTHONUTF8="0")
                result = subprocess.run(
                    [sys.executable, str(CHECKER), "1.4.3", str(path)],
                    env=env, capture_output=True,
                )
                self.assertEqual(result.returncode, 1 if defect else 0)
                output = (result.stderr if defect else result.stdout).decode("utf-8")
                self.assertIn("资源版本检查失败" if defect else "资源版本检查通过", output)
                self.assertNotIn("Traceback", output)

    def test_三平台发布均在资源构建后校验版本(self):
        for platform in ("macos", "linux"):
            text = (ROOT / f"scripts/release-{platform}.sh").read_text()
            self.assertIn('SHIELD_VERSION="$VERSION" bash scripts/build-stub.sh', text)
            self.assertIn('scripts/check_resource_version.py', text)
            self.assertLess(text.index('SKIP_STANDARD_STUB_BUILD=1'), text.index('scripts/check_resource_version.py'))
        text = (ROOT / "scripts/release-windows.ps1").read_text()
        self.assertIn('-ShieldVersion $Version', text)
        self.assertIn('"scripts\\check_resource_version.py"', text)
        self.assertIn('$Version, $zip, $api19Zip', text)

    def test_发布函数传递真实版本且覆盖外部陈旧值(self):
        for platform in ("macos", "linux"):
            with self.subTest(platform=platform), tempfile.TemporaryDirectory() as directory:
                root = pathlib.Path(directory)
                (root / "scripts").mkdir()
                resources = root / "shield-stub/build/outputs/resources"
                resources.mkdir(parents=True)
                (root / "scripts/build-stub.sh").write_text(
                    'test "$SHIELD_VERSION" = "1.4.3" || exit 7\n'
                )
                (root / "scripts/build-android-api19-resources.sh").write_text('exit 0\n')
                for name in ("resources.zip", "resources-api19.zip"):
                    with zipfile.ZipFile(resources / name, "w") as archive:
                        archive.writestr("metadata.json", '{"version":"1.4.3"}')
                shutil.copyfile(CHECKER, root / "scripts/check_resource_version.py")
                text = (ROOT / f"scripts/release-{platform}.sh").read_text()
                function = re.search(r"(?ms)^build_stub\(\) \{.*?^\}", text).group()
                script = 'set -eu\nROOT="$1"\nVERSION=1.4.3\nexport SHIELD_VERSION=0.0.1\n'
                script += 'info() { :; }; success() { :; }; error() { exit 1; };\n'
                result = subprocess.run(["bash", "-c", script + function + '\nbuild_stub\n', "test", str(root)], capture_output=True)
                self.assertEqual(result.returncode, 0, result.stderr.decode())

    def test_校验完整版本并拒绝默认错误版本(self):
        for version in ("1.4.3", "1.4.3-rc.1", "1.0.0", None):
            with self.subTest(version=version), tempfile.TemporaryDirectory() as directory:
                paths = [pathlib.Path(directory) / name for name in ("标准.zip", "兼容.zip")]
                for path in paths:
                    with zipfile.ZipFile(path, "w") as archive:
                        archive.writestr("metadata.json", json.dumps({"version": version}))
                result = subprocess.run([sys.executable, str(CHECKER), "1.4.3", *map(str, paths)], capture_output=True)
                self.assertEqual(result.returncode == 0, version == "1.4.3", result.stderr.decode())

    def test_任一资源异常必须拦截(self):
        for defect in ("版本不一致", "无元数据", "非法JSON", "错误类型", "损坏ZIP", "文件缺失"):
            with self.subTest(defect=defect), tempfile.TemporaryDirectory() as directory:
                paths = [pathlib.Path(directory) / name for name in ("standard.zip", "compat.zip")]
                for path in paths:
                    with zipfile.ZipFile(path, "w") as archive:
                        if path == paths[1] and defect == "无元数据":
                            continue
                        payload = '{"version":"1.4.3"}'
                        if path == paths[1]:
                            payload = {"版本不一致": '{"version":"1.0.0"}', "非法JSON": '{', "错误类型": '[]'}.get(defect, payload)
                        archive.writestr("metadata.json", payload)
                if defect == "损坏ZIP":
                    paths[1].write_bytes(b"broken")
                if defect == "文件缺失":
                    paths[1].unlink()
                result = subprocess.run([sys.executable, str(CHECKER), "1.4.3", *map(str, paths)], capture_output=True)
                self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
