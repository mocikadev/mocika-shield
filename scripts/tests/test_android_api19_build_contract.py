import pathlib
import re
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]


class AndroidApi19BuildContractTests(unittest.TestCase):
    def test_android_setup_explicitly_selects_available_packages(self) -> None:
        for workflow in (ROOT / ".github/workflows").glob("*.yml"):
            content = workflow.read_text(encoding="utf-8")
            steps = re.split(r"(?m)^      - ", content)
            for step in steps:
                if "uses: android-actions/setup-android@" not in step:
                    continue
                with self.subTest(workflow=workflow.name, step=step.splitlines()[0]):
                    self.assertRegex(
                        step,
                        r"(?m)^        with:\n          packages: platform-tools$",
                        "必须显式指定可用包，避免默认安装已不可用的 tools",
                    )

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
        self.assertIn('zip -qr "$CANDIDATE_RESOURCES"', content)
        self.assertIn("-x '*.DS_Store' '__MACOSX/*'", standard_content)
        self.assertIn("find \"$WORK_DIR\" -type f -name '.DS_Store' -delete", content)
        self.assertIn('rm -rf "$WORK_DIR/__MACOSX"', content)
        self.assertNotIn("jar --create", content)
        self.assertIn('unzip -tq "$CANDIDATE_RESOURCES"', content)
        self.assertIn('"min_android_api": 19', content)
        self.assertIn("未安装 Android 4.4 兼容构建所需的 NDK", content)

    def test_所有资源入口复用严格绑定且兼容_windows_换行(self) -> None:
        for script in ("scripts/build-stub.sh", "scripts/build-android-api19-resources.sh"):
            content = (ROOT / script).read_text(encoding="utf-8")
            self.assertIn("scripts/stub_bindings.py", content)
            self.assertIn("audit", content)
            self.assertIn("binding_value=\"${binding_value%$'\\r'}\"", content)
            self.assertNotIn("parse_mapping_method", content)
            self.assertNotIn("getSignatureSha256}", content)
        windows = (ROOT / "scripts/build-stub.ps1").read_text(encoding="utf-8")
        self.assertIn('scripts\\stub_bindings.py', windows)
        self.assertIn('$bindingTool, "audit"', windows)
        self.assertNotIn("Parse-MappingMethod", windows)
        probe = (ROOT / "tests/scripts/run-api19-protect-e2e.sh").read_text(encoding="utf-8")
        self.assertIn("build/outputs/resources/resources-api19.zip", probe)
        self.assertNotIn("build/experiments/api19/resources-api19.zip", probe)

    def test_两套_native_均规范化依赖源码位置且尊重自定义目录(self) -> None:
        for script in ("scripts/build-stub.sh", "scripts/verify-android-api19-native.sh"):
            content = (ROOT / script).read_text(encoding="utf-8")
            self.assertIn('source "$SCRIPT_DIR/native-build-flags.sh"', content)
            self.assertIn('configure_native_source_paths "$PROJECT_ROOT"', content)
        windows = (ROOT / "scripts/build-stub.ps1").read_text(encoding="utf-8")
        self.assertIn("$env:CARGO_HOME", windows)
        self.assertIn("$env:RUSTUP_HOME", windows)
        self.assertIn("${cargoSourceHome}", windows)
        self.assertIn("${rustupSourceHome}", windows)
        self.assertIn("$nativeSourceFlags -join [char]31", windows)

    def test_资源审计必须先于自动发现入口更新(self) -> None:
        standard = (ROOT / "scripts/build-stub.sh").read_text(encoding="utf-8")
        self.assertLess(standard.rindex('scripts/stub_bindings.py audit'), standard.index('ln -sf '))
        compat = (ROOT / "scripts/build-android-api19-resources.sh").read_text(encoding="utf-8")
        self.assertLess(compat.rindex('scripts/stub_bindings.py" audit'), compat.index('mv -f "$CANDIDATE_RESOURCES"'))
        windows = (ROOT / "scripts/build-stub.ps1").read_text(encoding="utf-8")
        self.assertLess(windows.rindex('$bindingTool, "audit"'), windows.index('Copy-Item $resourcesZip $resourcesZipLink'))


if __name__ == "__main__":
    unittest.main()
