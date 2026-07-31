#!/usr/bin/env bash

# 构建并注入仅供 DEX 方法代码分离研究使用的 ARM64 JNI 库。

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SOURCE="$PROJECT_ROOT/tests/fixtures/android-smoke-app/native-research/jni_dex_separation.c"

if [[ $# -ne 2 ]]; then
    echo "用法：$0 <输入 APK> <输出 APK>" >&2
    exit 1
fi
if [[ -z "${ANDROID_HOME:-}" ]]; then
    echo "缺少 ANDROID_HOME 环境变量" >&2
    exit 1
fi

INPUT_APK="$1"
OUTPUT_APK="$2"
NDK_ROOT="${ANDROID_NDK_ROOT:-$ANDROID_HOME/ndk/29.0.14206865}"
CLANG="$(find "$NDK_ROOT/toolchains/llvm/prebuilt" -path '*/bin/aarch64-linux-android21-clang' -print -quit)"
if [[ ! -f "$INPUT_APK" || ! -f "$SOURCE" || ! -x "$CLANG" ]]; then
    echo "JNI 研究输入、源码或 NDK r29 ARM64 编译器不存在" >&2
    exit 1
fi

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/mocika-dex-jni.XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT
LIB_DIR="$WORK_DIR/lib/arm64-v8a"
mkdir -p "$LIB_DIR" "$(dirname "$OUTPUT_APK")"

"$CLANG" -shared -fPIC -O2 -Wall -Wextra -Werror \
    -Wl,-z,max-page-size=16384 \
    -o "$LIB_DIR/libdexresearch.so" "$SOURCE"

cp "$INPUT_APK" "$OUTPUT_APK"
(
    cd "$WORK_DIR"
    zip -q "$OUTPUT_APK" lib/arm64-v8a/libdexresearch.so
)
unzip -Z1 "$OUTPUT_APK" | grep -qx 'lib/arm64-v8a/libdexresearch.so'
echo "JNI 研究库已注入：$OUTPUT_APK"
