import io
import struct
import tempfile
import unittest
import zipfile
from pathlib import Path

from assemble_dexb_aab import assemble, extract_original_dex
from prepare_multidex_aab import SECOND_DEX_ENTRY, add_second_dex
from prepare_runtime_jni import prepare
from verify_tail_preservation import MARKER, inject_marker, inspect_apks


def make_dex(size: int = 128) -> bytes:
    data = bytearray(size)
    data[:8] = b"dex\n035\0"
    struct.pack_into("<I", data, 32, size)
    return bytes(data)


class TailPreservationTests(unittest.TestCase):
    def test_inject_marker_keeps_header_file_size(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.aab"
            target = root / "target.aab"
            with zipfile.ZipFile(source, "w") as archive:
                archive.writestr("base/dex/classes.dex", make_dex())

            report = inject_marker(source, target)

            with zipfile.ZipFile(target) as archive:
                injected = archive.read("base/dex/classes.dex")
            self.assertEqual(report["dex_header_file_size"], 128)
            self.assertEqual(len(injected), 128 + len(MARKER))
            self.assertTrue(injected.endswith(MARKER))

    def test_inspect_apks_finds_nested_marker(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            apks = root / "probe.apks"
            apk_buffer = io.BytesIO()
            with zipfile.ZipFile(apk_buffer, "w") as apk:
                apk.writestr("classes.dex", make_dex() + MARKER)
            with zipfile.ZipFile(apks, "w") as apk_set:
                apk_set.writestr("splits/base-master.apk", apk_buffer.getvalue())

            result = inspect_apks(apks)

            self.assertEqual(len(result), 1)
            self.assertTrue(result[0]["marker_preserved"])
            self.assertTrue(result[0]["marker_at_end"])
            self.assertEqual(result[0]["marker_offset"], 128)

    def test_assemble_real_container_layout(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            original_aab = root / "original.aab"
            shell_aab = root / "shell.aab"
            resources = root / "resources.zip"
            dexb = root / "app.dexb"
            output = root / "protected.aab"

            with zipfile.ZipFile(original_aab, "w") as archive:
                archive.writestr("base/dex/classes.dex", make_dex())
            with zipfile.ZipFile(shell_aab, "w") as archive:
                archive.writestr("base/manifest/AndroidManifest.xml", b"manifest")
                archive.writestr("base/lib/arm64-v8a/libmocikashield.so", b"elf")
            with zipfile.ZipFile(resources, "w") as archive:
                archive.writestr("stub-classes.dex", make_dex(256))
                archive.writestr("lib/arm64-v8a/libmocikashield.so", b"elf")
                archive.writestr("metadata.json", '{"stub_application":"msk.b"}')
            dexb.write_bytes(b"DEXB" + struct.pack("<I", 5) + b"payload")

            extracted = extract_original_dex(original_aab, root / "dex")
            report = assemble(shell_aab, resources, dexb, output)

            self.assertEqual(extracted[0].read_bytes(), make_dex())
            self.assertEqual(report["dex_header_file_size"], 256)
            with zipfile.ZipFile(output) as archive:
                protected = archive.read("base/dex/classes.dex")
                self.assertEqual(archive.read("base/lib/arm64-v8a/libmocikashield.so"), b"elf")
            self.assertEqual(struct.unpack_from("<I", protected, 32)[0], 256)
            self.assertTrue(protected[256:].startswith(b"MSHD"))
            self.assertTrue(protected.endswith(dexb.read_bytes()))

    def test_prepare_runtime_jni_extracts_abi_directories(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            resources = root / "resources.zip"
            output = root / "jni"
            with zipfile.ZipFile(resources, "w") as archive:
                archive.writestr("lib/arm64-v8a/libmocikashield.so", b"arm64")
                archive.writestr("lib/x86_64/libmocikashield.so", b"x86_64")

            abis = prepare(resources, output)

            self.assertEqual(abis, ["arm64-v8a", "x86_64"])
            self.assertEqual((output / "arm64-v8a/libmocikashield.so").read_bytes(), b"arm64")
            self.assertEqual((output / "x86_64/libmocikashield.so").read_bytes(), b"x86_64")

    def test_assemble_rejects_shell_without_native_libraries(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            shell_aab = root / "shell.aab"
            resources = root / "resources.zip"
            dexb = root / "app.dexb"
            with zipfile.ZipFile(shell_aab, "w") as archive:
                archive.writestr("base/manifest/AndroidManifest.xml", b"manifest")
            with zipfile.ZipFile(resources, "w") as archive:
                archive.writestr("stub-classes.dex", make_dex(256))
                archive.writestr("lib/arm64-v8a/libmocikashield.so", b"elf")
                archive.writestr("metadata.json", '{"stub_application":"msk.b"}')
            dexb.write_bytes(b"DEXB" + struct.pack("<I", 5) + b"payload")

            with self.assertRaisesRegex(ValueError, "缺少 Gradle 打包的 JNI 运行库"):
                assemble(shell_aab, resources, dexb, root / "protected.aab")

    def test_add_second_dex_preserves_existing_bundle_entries(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.aab"
            output = root / "multidex.aab"
            with zipfile.ZipFile(source, "w") as archive:
                archive.writestr("base/dex/classes.dex", make_dex())
                archive.writestr("base/manifest/AndroidManifest.xml", b"manifest")

            add_second_dex(source, b"second-dex", output)

            with zipfile.ZipFile(output) as archive:
                self.assertEqual(archive.read(SECOND_DEX_ENTRY), b"second-dex")
                self.assertEqual(archive.read("base/dex/classes.dex"), make_dex())


if __name__ == "__main__":
    unittest.main()
