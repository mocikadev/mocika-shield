"""完整加固专项材料提取边界；合成容器仅测提取器，不当作真实壳。"""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("protection_resources", ROOT / "scripts/extract_protection_test_resources.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def archive(variant=""):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode="w:gz") as tar:
        for name, api in MODULE.MEMBERS.items():
            zipped = io.BytesIO()
            with zipfile.ZipFile(zipped, "w") as zip_file:
                zip_file.writestr("metadata.json", json.dumps({"version": "0.0.0" if variant == "version" else "1.4.3", "min_android_api": api + (variant == "api")}))
            member = tarfile.TarInfo(name)
            member.size = len(zipped.getvalue())
            if variant == "link":
                member.type = tarfile.SYMTYPE
            tar.addfile(member, io.BytesIO(zipped.getvalue()))
            if variant == "duplicate":
                tar.addfile(member, io.BytesIO(zipped.getvalue()))
            if variant == "missing":
                break
    return output.getvalue()


class ProtectionResourceTests(unittest.TestCase):
    def test_两套版本和类型必须齐全(self):
        self.assertEqual(set(MODULE.resources(archive())), set(MODULE.MEMBERS))
        for defect in ("version", "api", "link", "duplicate", "missing"):
            with self.subTest(defect=defect), self.assertRaises(ValueError):
                MODULE.resources(archive(defect))

    def test_固定发布包摘要必须匹配(self):
        with self.assertRaises(ValueError):
            MODULE.payload(b"not-the-published-package")

    def test_AR边界重复及缺失明确拒绝(self):
        def entry(content):
            header = b"data.tar.gz/    " + b"0           0     0     100644  " + str(len(content)).encode().ljust(10) + b"`\n"
            self.assertEqual(len(header), 60)
            return header + content + (b"\n" if len(content) % 2 else b"")

        valid = b"!<arch>\n" + entry(b"data")
        for package, expected in ((valid, b"data"), (b"!<arch>\n", None), (valid + entry(b"data"), None), (valid[:-1], None)):
            with self.subTest(package=package), patch.object(MODULE, "PACKAGE_SHA256", hashlib.sha256(package).hexdigest()):
                if expected:
                    self.assertEqual(MODULE.payload(package), expected)
                else:
                    with self.assertRaises(ValueError):
                        MODULE.payload(package)
