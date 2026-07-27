import tempfile
import unittest
from pathlib import Path

from scripts.normalize_release_checksums import (
    normalize_checksum_file,
    normalize_checksum_text,
)


class NormalizeReleaseChecksumsTests(unittest.TestCase):
    def test_去掉构建目录并统一换行(self):
        content = (
            f"{'a' * 64}  ./gui-deb/MocikaShield_1.2.7_linux_amd64.deb\r\n"
            f"{'b' * 64}  ./gui-appimage/MocikaShield_1.2.7_linux_amd64.AppImage\r\n"
        )

        normalized = normalize_checksum_text(content)

        self.assertEqual(
            normalized,
            f"{'b' * 64}  MocikaShield_1.2.7_linux_amd64.AppImage\n"
            f"{'a' * 64}  MocikaShield_1.2.7_linux_amd64.deb\n",
        )

    def test_兼容反斜杠和二进制模式标记(self):
        content = f"{'C' * 64}  *gui-nsis\\MocikaShield_setup.exe\r\n"

        self.assertEqual(
            normalize_checksum_text(content),
            f"{'c' * 64}  MocikaShield_setup.exe\n",
        )

    def test_拒绝重复的最终文件名(self):
        content = (
            f"{'a' * 64}  ./one/MocikaShield.dmg\n"
            f"{'b' * 64}  ./two/MocikaShield.dmg\n"
        )

        with self.assertRaisesRegex(ValueError, "重复文件名"):
            normalize_checksum_text(content)

    def test_拒绝无效校验记录(self):
        with self.assertRaisesRegex(ValueError, "有效的 SHA-256"):
            normalize_checksum_text("不是校验记录\n")

    def test_文件输出不含字节序标记和回车(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "checksums-sha256.txt"
            path.write_bytes(
                b"\xef\xbb\xbf" + ("d" * 64 + "  ./gui-dmg/MocikaShield.dmg\r\n").encode()
            )

            normalize_checksum_file(path)

            self.assertEqual(
                path.read_bytes(),
                ("d" * 64 + "  MocikaShield.dmg\n").encode(),
            )


if __name__ == "__main__":
    unittest.main()
