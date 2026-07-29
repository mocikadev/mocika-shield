import pathlib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]


class AndroidApi19BuildContractTests(unittest.TestCase):
    def test_ci_and_release_install_both_ndk_versions(self) -> None:
        for relative_path in (".github/workflows/ci.yml", ".github/workflows/release.yml"):
            content = (ROOT / relative_path).read_text(encoding="utf-8")
            self.assertIn("ANDROID_NDK_VERSION: 29.0.14206865", content)
            self.assertIn("ANDROID_NDK_API19_VERSION: 25.2.9519653", content)
            self.assertIn('"ndk;${ANDROID_NDK_API19_VERSION}"', content)

        release = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        self.assertIn('"ndk;$env:ANDROID_NDK_API19_VERSION"', release)

        ci = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        self.assertIn("windows-api19-resources:", ci)
        self.assertIn('& bash "scripts/build-android-api19-resources.sh"', ci)
        self.assertIn("name: 变更范围检查", ci)
        self.assertIn("docs/*|*.md|LICENSE|LICENSE.*", ci)
        self.assertIn("run_checks: ${{ steps.scope.outputs.run_checks }}", ci)
        self.assertIn("if: needs.changes.outputs.run_checks == 'true'", ci)
        self.assertIn("name: 基础快速检查", ci)
        self.assertIn("name: 完整代码质量检查", ci)
        self.assertGreaterEqual(
            ci.count("if: github.event_name == 'workflow_dispatch'"), 5
        )
        self.assertNotIn("run_android_stub", ci)
        self.assertNotIn("run_linux_gui", ci)
        self.assertNotIn("run_windows_api19", ci)

    def test_build_script_only_validates_dependencies(self) -> None:
        content = (ROOT / "scripts/build-android-api19-resources.sh").read_text(
            encoding="utf-8"
        )
        standard_content = (ROOT / "scripts/build-stub.sh").read_text(encoding="utf-8")
        commands = [line.strip() for line in content.splitlines()]
        self.assertNotIn('sdkmanager "ndk;25.2.9519653"', commands)
        self.assertFalse(any(line.startswith("rustup toolchain install") for line in commands))
        self.assertIn('if [[ "${OS:-}" == "Windows_NT" ]]', content)
        self.assertIn("Compress-Archive -Path $files", content)
        self.assertIn('$ErrorActionPreference = "Stop"', content)
        self.assertIn('zip -qr "$LEGACY_RESOURCES"', content)
        self.assertIn("-x '*.DS_Store' '__MACOSX/*'", standard_content)
        self.assertIn("find \"$WORK_DIR\" -type f -name '.DS_Store' -delete", content)
        self.assertIn('rm -rf "$WORK_DIR/__MACOSX"', content)
        self.assertNotIn("jar --create", content)
        self.assertIn('unzip -tq "$LEGACY_RESOURCES"', content)
        self.assertIn('"min_android_api": 19', content)
        self.assertIn("未安装 Android 4.4 兼容构建所需的 NDK", content)


if __name__ == "__main__":
    unittest.main()
