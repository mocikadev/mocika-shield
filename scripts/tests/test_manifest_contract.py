import copy
import json
import tempfile
import unittest
from pathlib import Path

from scripts.manifest_contract import JAVA, ROOT, audit_resource, check_projections, load_contract, projections


class ManifestContractTests(unittest.TestCase):
    def test_生成投影与单一来源保持一致且_java_不含旧字段(self):
        contract = load_contract()
        check_projections(contract)
        current_java = projections(contract)[JAVA]
        for field in contract["fields"].values():
            self.assertIn(field["current"], current_java)
            self.assertNotIn(field["legacy"], current_java)

    def test_拒绝缺失重复非法字段和复用旧协议(self):
        original = load_contract()
        invalid = ["null", "[]"]
        for mutate in (
            lambda value: value["fields"].pop("policy"),
            lambda value: value["protocols"].update(standard=2),
            lambda value: value["protocols"].update(standard=True),
            lambda value: value["protocols"].update(legacy_standard=8),
            lambda value: value["fields"]["policy"].update(current="dev.mocika.bad"),
            lambda value: value["fields"]["policy"].update(current="rt.app"),
            lambda value: value["fields"]["policy"].update(current='bad"name'),
            lambda value: value["fields"].update(policy=None),
            lambda value: value.update(protocols=None),
        ):
            value = copy.deepcopy(original)
            mutate(value)
            invalid.append(json.dumps(value))
        invalid.append(json.dumps(original).replace('"standard": 4', '"standard": 4, "standard": 6'))
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "contract.json"
            for content in invalid:
                source.write_text(content, encoding="utf-8")
                with self.subTest(content=content), self.assertRaises(ValueError):
                    load_contract(source)

    def test_资源协议必须与实际_dex_字段及能力一致(self):
        contract = load_contract()
        fields = contract["fields"]
        strings = {field["current"] for field in fields.values()}
        for protocol, memory in ((4, False), (5, True)):
            audit_resource({"runtime_protocol": protocol, "memory_dex": memory}, strings)
        for metadata in (None, [], {}, {"runtime_protocol": 2, "memory_dex": False},
                         {"runtime_protocol": 4, "memory_dex": True},
                         {"runtime_protocol": 4, "memory_dex": 0},
                         {"runtime_protocol": "4", "memory_dex": False}):
            with self.subTest(metadata=metadata), self.assertRaises(ValueError):
                audit_resource(metadata, strings)
        metadata = {"runtime_protocol": 4, "memory_dex": False}
        for field in fields.values():
            for altered in (strings - {field["current"]}, strings | {field["legacy"]}):
                with self.assertRaises(ValueError):
                    audit_resource(metadata, altered)

    def test_构建入口消费共同定义且不另写协议号(self):
        bash = (ROOT / "scripts/build-stub.sh").read_text(encoding="utf-8")
        powershell = (ROOT / "scripts/build-stub.ps1").read_text(encoding="utf-8")
        for name, content in (("Bash", bash), ("PowerShell", powershell)):
            with self.subTest(entry=name):
                self.assertIn("manifest_contract.py", content)
                self.assertNotRegex(content, r'"runtime_protocol": [0-9]')
        self.assertIn("checkManifestContract", (ROOT / "shield-stub/build.gradle.kts").read_text(encoding="utf-8"))

    def test_旧协议字段映射不得改写历史含义(self):
        contract = load_contract()
        self.assertEqual({name: field["legacy"] for name, field in contract["fields"].items()}, {
            "application": "ORIGINAL_APPLICATION",
            "policy": "dev.mocika.shield.ENV_POLICY",
            "factory": "dev.mocika.shield.ORIGINAL_COMPONENT_FACTORY",
            "cache_schema": "dev.mocika.shield.CACHE_SCHEMA",
            "cache_count": "dev.mocika.shield.CACHE_DEX_COUNT",
            "cache_root": "dev.mocika.shield.CACHE_ROOT_SHA256",
            "payload_bytes": "dev.mocika.shield.PAYLOAD_DEX_BYTES",
        })


if __name__ == "__main__":
    unittest.main()
