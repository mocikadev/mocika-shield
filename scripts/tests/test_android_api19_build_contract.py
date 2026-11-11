import pathlib
import re
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]


class AndroidApi19BuildContractTests(unittest.TestCase):
    def test_定向手动验证不触发其他重型任务(self) -> None:
        content = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        self.assertIn("default: all", content)
        self.assertIn("          - windows_resources", content)
        self.assertIn("all|windows_resources|agent_platforms|agent_bundles|agent_windows_processes|agent_processes|agent_java|windows_paths|windows_protection|windows_defaults) ;;", content)
        self.assertIn("错误：未知的手动验证范围", content)
        jobs = dict(re.findall(r"(?ms)^  ([\w-]+):\n(.*?)(?=^  [\w-]+:|\Z)", content.split("jobs:\n", 1)[1]))
        for job in ("full-quality", "linux-gui-bundle", "stub", "release-readiness"):
            condition = re.search(r"(?m)^    if: (.*)$", jobs[job]).group(1)
            self.assertIn("github.event_name == 'workflow_dispatch'", condition)
            self.assertIn("inputs.validation_scope == 'all'", condition)
            self.assertNotIn("windows_resources", condition)
            self.assertNotIn("agent_platforms", condition)
            self.assertNotIn("agent_windows_processes", condition)
            self.assertNotIn("agent_processes", condition)
            self.assertNotIn("agent_java", condition)
            self.assertNotIn("windows_paths", condition)
            self.assertNotIn("windows_protection", condition)
            self.assertNotIn("windows_defaults", condition)
        self.assertIn("inputs.validation_scope == 'windows_resources'", jobs["windows-api19-resources"])

    def test_完整加固专项不重复构建壳且不进入日常检查(self) -> None:
        content = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        job = content.split("  windows-protection:\n", 1)[1].split("  windows-paths:\n", 1)[0]
        self.assertIn("github.event_name == 'workflow_dispatch' && (inputs.validation_scope == 'windows_protection' || inputs.validation_scope == 'windows_defaults')", job)
        self.assertIn("scripts/build_protection_test_apk.py", job)
        self.assertIn("scripts/extract_protection_test_resources.py", job)
        self.assertEqual(job.count("--lib protect_api::context_tests -- --include-ignored"), 2)
        self.assertEqual(job.count("--test agent_desktop -- --ignored"), 2)
        self.assertEqual(job.count("--lib keytool::ordinary_tests -- --ignored"), 2)
        self.assertEqual(job.count("--lib 默认新建入库失败 -- --ignored"), 2)
        self.assertIn("SHIELD_TEST_APKSIGNER=", job)
        self.assertIn("--features mcp-server --all-targets --locked", job)
        self.assertIn("gh release download v1.4.3", job)
        for forbidden in ("build-stub", "cargo-ndk", "cargo tauri", "gradlew", "gh release create", "gh release upload"):
            self.assertNotIn(forbidden, job)

    def test_普通工具专项复用准备但不重复长会话(self) -> None:
        content = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        job = content.split("  windows-protection:\n", 1)[1].split("  windows-paths:\n", 1)[0]
        self.assertEqual(job.count("scripts/build_protection_test_apk.py"), 1)
        self.assertEqual(job.count("gh release download v1.4.3"), 1)
        self.assertEqual(job.count("--lib ordinary_apk_tests -- --include-ignored"), 2)
        self.assertEqual(job.count("--test agent_desktop ordinary::\n          -- --ignored"), 2)
        self.assertNotIn("run: cargo test -p shield-cli --features mcp-server --locked --test agent_desktop ordinary:: ", job)
        self.assertEqual(job.count("--lib 异卷原地签名在对齐和工具启动前拒绝且原包不变 -- --ignored"), 2)
        self.assertIn("inputs.validation_scope == 'windows_defaults' && '-windows-defaults'", content)
        steps = job.split("      - name:")[1:]
        for step in steps:
            if "--lib protect_api::context_tests" in step or "--test agent_desktop -- --ignored" in step:
                self.assertIn("inputs.validation_scope == 'windows_protection'", step)
            if "--lib ordinary_apk_tests" in step or "--test agent_desktop ordinary::" in step or "--lib 异卷原地签名" in step:
                self.assertIn("inputs.validation_scope == 'windows_defaults'", step)

    def test_Windows路径及签名专项显式Java且不运行整套构建(self) -> None:
        content = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        jobs = dict(re.findall(r"(?ms)^  ([\w-]+):\n(.*?)(?=^  [\w-]+:|\Z)", content.split("jobs:\n", 1)[1]))
        job = jobs["windows-paths"]
        self.assertIn("github.event_name == 'workflow_dispatch' && inputs.validation_scope == 'windows_paths'", job)
        self.assertIn("runs-on: windows-2025", job)
        self.assertIn('java-version: "17"', job)
        self.assertIn('java-version: "8"', job)
        self.assertIn("distribution: corretto", job)
        self.assertIn("Java 8 路径与证书内容对照", job)
        self.assertIn("python scripts/build_java_adapter.py", job)
        self.assertIn("--lib java_adapter::tests", job)
        self.assertIn("--lib java_directory::tests", job)
        self.assertIn("--lib windows_private::tests", job)
        self.assertIn("--features agent-executor --locked --bin shield -- --test-threads=1", job)
        self.assertIn("--features agent-executor --all-targets --locked", job)
        self.assertEqual(job.count("automation::native_group::tests::certificate_tests"), 2)
        self.assertEqual(job.count("automation::native_group::tests::signing_tests"), 2)
        self.assertEqual(job.count("automation::native_group::tests::apktool_tests"), 2)
        self.assertIn("Java 17 真实签名验签窗口及故障停止", job)
        self.assertIn("Java 8 真实签名验签窗口及故障停止", job)
        self.assertIn("Java 17 新建三阶段故障及整组停止", job)
        self.assertIn("Java 8 新建三阶段故障及整组停止", job)
        self.assertIn("if: ${{ !cancelled() }}", job)
        self.assertIn("--locked --test windows_tool_paths", job)
        self.assertIn("-- --include-ignored --test-threads=1 --show-output", job)
        self.assertIn("inputs.validation_scope == 'windows_paths' && '-windows-paths'", content)
        # 真实签名阶段需要 SDK 生成自有二进制清单 APK 并独立校验对齐，不是壳或应用构建。
        self.assertEqual(job.count("uses: android-actions/setup-android@"), 1)
        self.assertIn("packages: platform-tools platforms;android-35 build-tools;35.0.0", job)
        self.assertIn('aapt2.exe" link', job)
        self.assertIn('aapt2.exe" compile', job)
        self.assertIn("--dir tests/fixtures/agent-signing/res", job)
        self.assertIn("            target/java-adapter-signing/resources.zip", job)
        self.assertIn("tests/fixtures/agent-signing/AndroidManifest.xml", job)
        self.assertIn("SHIELD_TEST_APK=", job)
        self.assertIn("SHIELD_TEST_ZIPALIGN=", job)
        for forbidden in ("cargo tauri", "build-stub", "cargo-ndk", "gh release", "--features mcp-server", "gradlew", "ndk;"):
            self.assertNotIn(forbidden, job)

    def test_Agent专项仅手动运行且不重复构建壳或发布(self) -> None:
        content = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        jobs = dict(re.findall(r"(?ms)^  ([\w-]+):\n(.*?)(?=^  [\w-]+:|\Z)", content.split("jobs:\n", 1)[1]))
        job = jobs["agent-platforms"]
        self.assertIn("if: github.event_name == 'workflow_dispatch' && inputs.validation_scope == 'agent_platforms'", job)
        self.assertIn("ubuntu-24.04", job)
        self.assertIn("macos-15", job)
        self.assertIn("windows-2025", job)
        self.assertIn("--features mcp-server --locked --bin shield", job)
        self.assertIn("--test agent_commands", job)
        self.assertIn("--test mcp_signing", job)
        self.assertIn("--test agent_desktop", job)
        self.assertIn("-- --ignored --skip 真实共享规划加固签名与绑定验收保持同一会话\n          --skip ordinary:: --test-threads=1 --show-output", job)
        self.assertIn("python scripts/build_java_adapter.py", job)
        self.assertIn("if: runner.os != 'Windows'", job)
        self.assertIn("cargo clippy", job)
        self.assertIn("Windows 默认与独立执行器特性严格检查", job)
        self.assertIn("cargo clippy -p shield-cli --all-targets --locked -- -D warnings", job)
        self.assertIn("tests/fixtures/agent-signing/AndroidManifest.xml", job)
        self.assertIn("packages: platform-tools platforms;android-35 build-tools;35.0.0", job)
        self.assertNotIn('          sdkmanager "platforms;android-35"', job)
        self.assertIn("-- --ignored --exact --test-threads=1", job)
        for forbidden in ("make build-stub", "cargo tauri", "gh release", "cargo-ndk"):
            self.assertNotIn(forbidden, job)

    def test_Windows进程基础专项仅手动运行且不冒充完整接入(self) -> None:
        content = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        self.assertIn("          - agent_windows_processes", content)
        jobs = dict(re.findall(r"(?ms)^  ([\w-]+):\n(.*?)(?=^  [\w-]+:|\Z)", content.split("jobs:\n", 1)[1]))
        job = jobs["agent-windows-processes"]
        self.assertIn("if: github.event_name == 'workflow_dispatch' && inputs.validation_scope == 'agent_windows_processes'", job)
        self.assertIn("runs-on: windows-2025", job)
        self.assertIn("--features agent-executor --locked --bin shield", job)
        self.assertIn("automation::windows_job::tests -- --test-threads=1", job)
        self.assertIn("cargo clippy", job)
        for forbidden in ("setup-java", "setup-android", "build-stub", "tauri", "gh release", "cargo-ndk"):
            self.assertNotIn(forbidden, job)

    def test_三平台原生标准流专项不触发工具和发布构建(self) -> None:
        content = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        self.assertIn("          - agent_processes", content)
        jobs = dict(re.findall(r"(?ms)^  ([\w-]+):\n(.*?)(?=^  [\w-]+:|\Z)", content.split("jobs:\n", 1)[1]))
        job = jobs["agent-processes"]
        self.assertIn("if: github.event_name == 'workflow_dispatch' && inputs.validation_scope == 'agent_processes'", job)
        for platform in ("ubuntu-24.04", "macos-15", "windows-2025"):
            self.assertIn(platform, job)
        self.assertIn("--features mcp-server --locked --bin shield", job)
        self.assertIn("-- --test-threads=1 --show-output", job)
        self.assertIn("cargo clippy", job)
        for forbidden in ("setup-java", "setup-android", "build-stub", "tauri", "gh release", "cargo-ndk"):
            self.assertNotIn(forbidden, job)

    def test_完整桌面入口仅在显式特性启用且包含原生验收(self) -> None:
        entry = (ROOT / "apps/shield-cli/src/main.rs").read_text(encoding="utf-8")
        mcp_guard = entry.split("mod mcp;", 1)[0].rsplit("#[cfg(", 1)[1]
        self.assertIn('feature = "mcp-server"', mcp_guard)
        self.assertIn("windows", mcp_guard)
        modules = (ROOT / "apps/shield-cli/src/automation/mod.rs").read_text(encoding="utf-8")
        transport_guard = modules.split("pub(crate) mod transport;", 1)[0].rsplit("#[cfg(", 1)[1]
        self.assertIn('feature = "mcp-server"', transport_guard)
        self.assertIn("windows", transport_guard)
        self.assertNotIn("all(windows, test)", transport_guard)
        execution_guard = modules.split("pub(crate) mod execution;", 1)[0].rsplit("#[cfg(", 1)[1]
        self.assertIn('feature = "agent-executor"', execution_guard)
        self.assertIn("windows", execution_guard)
        controller = (ROOT / "apps/shield-cli/src/automation/execution/controller.rs").read_text(encoding="utf-8")
        self.assertIn("task_files::WorkDirectory::new", controller)
        self.assertNotIn("Permissions::from_mode", controller)
        for name in ("src/mcp/session.rs", "src/automation/execution/interface.rs"):
            self.assertNotIn("/dev/urandom", (ROOT / "apps/shield-cli" / name).read_text(encoding="utf-8"))
        entropy = (ROOT / "apps/shield-cli/src/automation/random_id.rs").read_text(encoding="utf-8")
        self.assertIn("BCryptGenRandom", entropy)

    def test_三平台Java探针仅手动使用自有源码且不增加发布任务(self) -> None:
        content = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        self.assertIn("          - agent_java", content)
        jobs = dict(re.findall(r"(?ms)^  ([\w-]+):\n(.*?)(?=^  [\w-]+:|\Z)", content.split("jobs:\n", 1)[1]))
        job = jobs["agent-java"]
        self.assertIn("if: github.event_name == 'workflow_dispatch' && inputs.validation_scope == 'agent_java'", job)
        for platform in ("ubuntu-24.04", "macos-15", "windows-2025"):
            self.assertIn(platform, job)
        for version in ("8", "17"):
            self.assertIn(f'java-version: "{version}"', job)
        self.assertEqual(job.count("-- --ignored --exact --show-output"), 2)
        self.assertEqual(job.count("SHIELD_TEST_JAVA_HOME: ${{ env.JAVA_HOME }}"), 2)
        self.assertIn("-- --test-threads=1 --show-output", job)
        self.assertIn("cargo clippy", job)
        self.assertIn("automation::java_environment::tests::真实jdk中文空格目录属性往返一致", job)
        for forbidden in ("setup-android", "build-stub", "tauri", "gh release", "cargo-ndk"):
            self.assertNotIn(forbidden, job)

    def test_android_setup_explicitly_selects_available_packages(self) -> None:
        for workflow in (ROOT / ".github/workflows").glob("*.yml"):
            content = workflow.read_text(encoding="utf-8")
            steps = re.split(r"(?m)^      - ", content)
            for step in steps:
                if "uses: android-actions/setup-android@" not in step:
                    continue
                with self.subTest(workflow=workflow.name, step=step.splitlines()[0]):
                    packages = re.search(r"(?m)^        with:\n          packages: ([^\n]+)$", step)
                    self.assertIsNotNone(packages, "必须显式指定可用包，避免默认安装已不可用的 tools")
                    names = packages.group(1).split()
                    self.assertIn("platform-tools", names)
                    for name in names:
                        self.assertIn(name, {"platform-tools", "platforms;android-35", "build-tools;35.0.0"})

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
