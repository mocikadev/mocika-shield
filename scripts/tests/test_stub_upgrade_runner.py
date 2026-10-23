import os
import subprocess
import unittest
from pathlib import Path


RUNNER = Path(__file__).resolve().parents[2] / "tests/scripts/run-stub-resource-upgrade-e2e.sh"


class StubUpgradeRunnerTests(unittest.TestCase):
    def run_guard(self, **overrides):
        environment = os.environ.copy()
        for name in ("ANDROID_SERIAL", "PREVIOUS_RESOURCES", "EXPECTED_API", "EXPECTED_ABI", "EXPECTED_PAGE_SIZE"):
            environment.pop(name, None)
        environment.update(overrides)
        return subprocess.run(["bash", str(RUNNER)], env=environment, capture_output=True, text=True, timeout=5)

    def test_脚本语法有效(self):
        subprocess.run(["bash", "-n", str(RUNNER)], check=True, timeout=5)

    def test_缺少目标模拟器不能隐式使用真机(self):
        result = self.run_guard()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("ANDROID_SERIAL", result.stderr)

    def test_缺少预期系统不能继续(self):
        result = self.run_guard(ANDROID_SERIAL="emulator-5570", PREVIOUS_RESOURCES="unused")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("EXPECTED_API", result.stderr)

    def test_明确拒绝真机且在调用设备命令前退出(self):
        result = self.run_guard(ANDROID_SERIAL="physical-device", PREVIOUS_RESOURCES="unused", EXPECTED_API="35", EXPECTED_ABI="arm64-v8a")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("不自动操作连接的真机", result.stderr)
        self.assertNotIn("缺少命令", result.stderr)

    def test_中文环境摘要使用明确变量边界(self):
        line = next(line for line in RUNNER.read_text(encoding="utf-8").splitlines() if line.startswith('echo "环境确认：'))
        result = subprocess.run(["bash", "-u", "-c", 'API=35; ABI=arm64-v8a; EXPECTED_PAGE_SIZE=16384; ' + line], capture_output=True, text=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("API=35，ABI=arm64-v8a，页大小=16384", result.stdout)


if __name__ == "__main__":
    unittest.main()
