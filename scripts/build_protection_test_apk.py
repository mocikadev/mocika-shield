"""使用显式 JDK／Android SDK 生成包含真实 DEX 和资源表的自有加固夹具。"""
import argparse
import os
from pathlib import Path
import subprocess
import sys
import zipfile

ROOT = Path(__file__).resolve().parent.parent


def build(jdk, sdk, output):
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    tools = sdk / "build-tools/35.0.0"
    android = sdk / "platforms/android-35/android.jar"
    extension = ".exe" if os.name == "nt" else ""
    fixture = ROOT / "tests/fixtures/agent-protection"
    classes, dex = output / "classes", output / "dex"
    classes.mkdir()
    dex.mkdir()

    def run(program, *args):
        subprocess.run([str(program), *map(str, args)], check=True,
                       creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)

    run(jdk / f"bin/javac{extension}", "-encoding", "UTF-8", "--release", "8",
        "-cp", android, "-d", classes, fixture / "MainApplication.java")
    run(jdk / f"bin/java{extension}", "-cp", tools / "lib/d8.jar",
        "com.android.tools.r8.D8", "--lib", android, "--min-api", "19", "--output", dex,
        classes / "dev/mocika/shield/fixture/MainApplication.class")
    run(tools / f"aapt2{extension}", "compile", "--dir",
        ROOT / "tests/fixtures/agent-signing/res", "-o", output / "resources.zip")
    apk = output / "unsigned.apk"
    run(tools / f"aapt2{extension}", "link", "--manifest", fixture / "AndroidManifest.xml",
        "-I", android, output / "resources.zip", "-o", apk)
    with zipfile.ZipFile(apk, "a", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.write(dex / "classes.dex", "classes.dex")
    print(f"已生成真实 DEX 自有夹具：{apk}")


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jdk", required=True, type=Path)
    parser.add_argument("--sdk", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    build(args.jdk.resolve(), args.sdk.resolve(), args.output)
