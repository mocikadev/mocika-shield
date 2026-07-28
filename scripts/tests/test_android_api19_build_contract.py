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

    def test_build_script_only_validates_dependencies(self) -> None:
        content = (ROOT / "scripts/build-android-api19-resources.sh").read_text(
            encoding="utf-8"
        )
        commands = [line.strip() for line in content.splitlines()]
        self.assertNotIn('sdkmanager "ndk;25.2.9519653"', commands)
        self.assertFalse(any(line.startswith("rustup toolchain install") for line in commands))
        self.assertIn("未安装 Android 4.4 兼容构建所需的 NDK", content)


if __name__ == "__main__":
    unittest.main()
