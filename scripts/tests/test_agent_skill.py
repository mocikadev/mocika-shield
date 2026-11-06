"""验证对外 Skill 可独立搬迁；不把结构校验当成真实模型行为验收。"""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest
from urllib.parse import unquote, urlsplit


ROOT = Path(__file__).resolve().parents[2]
SKILL = ROOT / "skills/mocika-shield"


def local_targets(root):
    """解析文档的资源引用，要求安装副本无需逃出技能目录即可读取。"""
    root = root.resolve()
    targets = []
    for source in root.rglob("*.md"):
        for target in re.findall(r"\[[^\]]*\]\(([^)]+)\)", source.read_text(encoding="utf-8")):
            parsed = urlsplit(target)
            if parsed.scheme or parsed.netloc:
                raise ValueError("技能操作指引应在包内闭合，不依赖在线内容")
            resolved = (source.parent / unquote(parsed.path)).resolve()
            if root != resolved and root not in resolved.parents:
                raise ValueError("引用逃出技能目录")
            if not resolved.is_file():
                raise ValueError("技能引用缺失")
            targets.append(resolved)
    return targets


class AgentSkillTests(unittest.TestCase):
    def test_整个技能复制到其他项目后引用仍闭合(self):
        with tempfile.TemporaryDirectory(prefix="技能搬迁 空格") as directory:
            copied = Path(directory) / "Android 工程/.agents/skills/mocika-shield"
            shutil.copytree(SKILL, copied)
            files = {path.relative_to(copied) for path in copied.rglob("*") if path.is_file()}
            self.assertEqual(files, {Path("SKILL.md"), Path("references/cli.md"), Path("agents/openai.yaml"), Path("scripts/resolve-cli.sh")})
            self.assertTrue(local_targets(copied))
            for name in files:
                self.assertEqual((copied / name).read_bytes(), (SKILL / name).read_bytes())

    def test_独立更新技能不覆盖宿主路径或签名配置(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory)
            installed = project / ".agents/skills/mocika-shield"
            installed.mkdir(parents=True)
            (installed / "SKILL.md").write_text("旧技能副本", encoding="utf-8")
            settings = project / "agent-local.env"
            config = project / "shield-cli.toml"
            settings.write_text("MOCIKA_SHIELD_CLI=/local/app/Contents/MacOS/shield\n", encoding="utf-8")
            config.write_text("schema_version=1\n", encoding="utf-8")
            before = (settings.read_bytes(), config.read_bytes())
            shutil.copytree(SKILL, installed, dirs_exist_ok=True)
            self.assertTrue(local_targets(installed))
            self.assertEqual((installed / "SKILL.md").read_bytes(), (SKILL / "SKILL.md").read_bytes())
            self.assertEqual(before, (settings.read_bytes(), config.read_bytes()))

    def test_缺失或越界引用不能被搬迁检查放行(self):
        for defect in ("missing.md", "../../outside.md", "https://example.invalid/setup"):
            with self.subTest(defect=defect), tempfile.TemporaryDirectory() as directory:
                copied = Path(directory) / "skill"
                copied.mkdir()
                (copied / "SKILL.md").write_text(f"[必要步骤]({defect})", encoding="utf-8")
                with self.assertRaises(ValueError):
                    local_targets(copied)

    def test_技能没有机器私有路径或分发外材料(self):
        self.assertFalse(any(path.is_symlink() for path in SKILL.rglob("*")))
        for path in SKILL.rglob("*"):
            if path.is_file():
                content = path.read_text(encoding="utf-8")
                self.assertNotIn("/Users/", content)
                self.assertNotIn("/tmp/mocika-", content)
                self.assertNotIn("/home/", content)

    def test_验收场景可移交且明确观察证据(self):
        data = json.loads((ROOT / "tests/agent-skill/cases.json").read_text(encoding="utf-8"))
        self.assertEqual(data["schema_version"], 1)
        cases = data["场景"]
        self.assertEqual(len({case["名称"] for case in cases}), len(cases))
        for case in cases:
            self.assertTrue(all(isinstance(case[key], str) and case[key].strip()
                                for key in ("名称", "请求", "预期", "证据")))


@unittest.skipUnless(os.name == "posix" and Path("/bin/bash").exists(), "路径定位专项需要 Bash")
class CliLocatorTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="命令定位 空格")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name).resolve()
        self.first = self.executable("工具甲/shield")
        self.second = self.executable("工具乙/shield")
        self.env = dict(os.environ, PATH=str(self.first.parent), LOCATOR_EXECUTION_MARKER=str(self.root / "executed"))
        self.env.pop("MOCIKA_SHIELD_CLI", None)
        self.env.pop("BASH_ENV", None)

    def executable(self, name):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('#!/bin/sh\nprintf executed > "$LOCATOR_EXECUTION_MARKER"\nexit 77\n', encoding="utf-8")
        path.chmod(0o755)
        return path

    def locate(self, *args, expected=0):
        output = subprocess.run(["/bin/bash", str(SKILL / "scripts/resolve-cli.sh"), *map(str, args)],
                                env=self.env, cwd=self.root, capture_output=True, timeout=5)
        self.assertEqual(output.returncode, expected, output.stderr.decode())
        self.assertFalse((self.root / "executed").exists(), "定位不能执行任何候选")
        if expected:
            self.assertEqual(output.stdout, b"")
        return output.stdout.decode().rstrip("\n")

    def test_本次路径优先于环境和PATH(self):
        self.env["MOCIKA_SHIELD_CLI"] = str(self.root / "missing")
        self.assertEqual(self.locate(self.second), str(self.second))

    def test_无效显式路径不借有效环境回退(self):
        self.env["MOCIKA_SHIELD_CLI"] = str(self.first)
        self.locate(self.root / "missing", expected=2)
        self.locate("工具甲/shield", expected=2)
        self.locate("", expected=2)

    def test_环境路径优先且空值或失效不能回退(self):
        self.env["MOCIKA_SHIELD_CLI"] = str(self.second)
        self.assertEqual(self.locate(), str(self.second))
        for invalid in ("", "relative/shield", str(self.root / "missing")):
            self.env["MOCIKA_SHIELD_CLI"] = invalid
            self.locate(expected=2)

    def test_PATH单一候选和重复项(self):
        self.assertEqual(self.locate(), str(self.first))
        self.env["PATH"] += os.pathsep + self.env["PATH"]
        self.assertEqual(self.locate(), str(self.first))

    def test_PATH多个物理文件要求明确选择(self):
        self.env["PATH"] += os.pathsep + str(self.second.parent)
        self.locate(expected=3)

    def test_PATH链接到同一文件不算歧义(self):
        linked = self.root / "链接/shield"
        linked.parent.mkdir()
        linked.symlink_to(self.first)
        self.env["PATH"] += os.pathsep + str(linked.parent)
        self.assertEqual(self.locate(), str(self.first))

    def test_相对PATH在当前目录定位但不递归遍历(self):
        self.env["PATH"] = "工具甲"
        self.assertEqual(self.locate(), str(self.first))
        self.env["PATH"] = "missing"
        self.locate(expected=1)

    def test_不可执行和悬空链接拒绝(self):
        self.second.chmod(0o644)
        self.locate(self.second, expected=2)
        broken = self.root / "悬空"
        broken.symlink_to(self.root / "missing")
        self.locate(broken, expected=2)

    def test_命令字符仅当路径数据且换行拒绝(self):
        path = self.executable("特殊;字符/shield$(任意指令)")
        self.assertEqual(self.locate(path), str(path))
        newline = self.executable("换行\n目录/shield")
        self.locate(newline, expected=2)

    def test_额外参数拒绝(self):
        self.locate(self.first, self.second, expected=2)


if __name__ == "__main__":
    unittest.main()
