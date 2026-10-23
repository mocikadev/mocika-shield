#!/usr/bin/env python3
"""使用本机 JVM 验证生产 JNI_OnLoad 的完整混淆绑定，不替代 Android 启动回归。"""

from __future__ import annotations

import os
import platform
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from scripts.stub_bindings import binding_environment, parse_mapping


def main() -> None:
    if platform.system() not in ("Darwin", "Linux"):
        raise SystemExit("此探针仅用于本机 macOS／Linux JVM，Windows 使用 Android 资源回归")
    mapping = ROOT / "shield-stub/build/outputs/mapping/release/mapping.txt"
    bindings = binding_environment(parse_mapping(mapping.read_text(encoding="utf-8")))
    environment = {**os.environ, **bindings}
    subprocess.run(["cargo", "build", "--release", "-p", "mocikashield"], cwd=ROOT, env=environment, check=True)
    suffix = "dylib" if platform.system() == "Darwin" else "so"
    native = ROOT / f"target/release/libmocikashield.{suffix}"
    package, name = bindings["STUB_BINLOADER_CLASS"].rsplit("/", 1)
    java_package = package.replace("/", ".")
    with tempfile.TemporaryDirectory(prefix="mocika-jni-host-") as directory:
        work = Path(directory)
        context = work / "android/content/Context.java"
        context.parent.mkdir(parents=True)
        context.write_text("package android.content; public class Context {}", encoding="utf-8")
        bridge = work / package / f"{name}.java"
        bridge.parent.mkdir(parents=True)
        # 只提供真实最终名称和签名，使用生产 Native 的注册实现。
        bridge.write_text(f"""package {java_package};
public class {name} {{
  public static native boolean {bindings['STUB_METHOD_INJECT_DEX']}(ClassLoader l, String[] p, String o);
  public static native byte[][] {bindings['STUB_METHOD_EXTRACT_DECRYPT']}(android.content.Context c, byte[] d);
  public static native int {bindings['STUB_METHOD_CHECK_ENV']}();
  public static native java.nio.ByteBuffer[] {bindings['STUB_METHOD_DECRYPT_BUFFERS']}(android.content.Context c, byte[] d);
  public static String {bindings['STUB_METHOD_GET_SIG']}(android.content.Context c) {{ return "probe"; }}
}}
""", encoding="utf-8")
        launcher = work / "BindingProbe.java"
        launcher.write_text(f"""public class BindingProbe {{
  public static void main(String[] args) throws Exception {{
    System.load(args[0]);
    Class.forName("{java_package}.{name}").getDeclaredMethod("{bindings['STUB_METHOD_CHECK_ENV']}").invoke(null);
    System.out.println("混淆 JNI 注册与调用通过");
  }}
}}
""", encoding="utf-8")
        subprocess.run(["javac", "-encoding", "UTF-8", "-d", str(work), str(context), str(bridge), str(launcher)], check=True)
        subprocess.run(["java", "-Xcheck:jni", "-cp", str(work), "BindingProbe", str(native)], check=True)


if __name__ == "__main__":
    main()
