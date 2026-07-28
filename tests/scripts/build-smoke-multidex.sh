#!/usr/bin/env bash

# 为 Smoke 测试 APK 注入独立的 classes2.dex，确保端到端回归真实覆盖多 DEX 加载。

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FIXTURE="$PROJECT_ROOT/tests/fixtures/android-smoke-app"

if [[ $# -lt 2 || $# -gt 3 ]]; then
    echo "用法：$0 <输入未签名 APK> <输出未签名 APK> [最低 API]" >&2
    exit 1
fi

INPUT_APK="$1"
OUTPUT_APK="$2"
MIN_API="${3:-19}"
WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/mocika-smoke-multidex.XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT

for command in javac unzip zip; do
    command -v "$command" >/dev/null || { echo "缺少命令：$command" >&2; exit 1; }
done

if [[ -z "${ANDROID_HOME:-}" ]]; then
    echo "缺少 ANDROID_HOME 环境变量" >&2
    exit 1
fi

if [[ ! -f "$INPUT_APK" ]]; then
    echo "输入 APK 不存在：$INPUT_APK" >&2
    exit 1
fi

BUILD_TOOLS_VERSION="$(find "$ANDROID_HOME/build-tools" -mindepth 1 -maxdepth 1 -type d -exec basename {} \; | sort -V | tail -n 1)"
PLATFORM_VERSION="$(find "$ANDROID_HOME/platforms" -mindepth 1 -maxdepth 1 -type d -name 'android-*' -exec basename {} \; | sort -t- -k2,2n | tail -n 1)"
D8="$ANDROID_HOME/build-tools/$BUILD_TOOLS_VERSION/d8"
ANDROID_JAR="$ANDROID_HOME/platforms/$PLATFORM_VERSION/android.jar"

if [[ ! -x "$D8" || ! -f "$ANDROID_JAR" ]]; then
    echo "Android SDK 缺少可用的 d8 或 android.jar" >&2
    exit 1
fi

SECONDARY_CLASSES="$WORK_DIR/secondary-classes"
SECONDARY_DEX="$WORK_DIR/secondary-dex"
mkdir -p "$SECONDARY_CLASSES" "$SECONDARY_DEX"

javac -source 8 -target 8 -cp "$ANDROID_JAR" -d "$SECONDARY_CLASSES" \
    "$FIXTURE/secondary-src/dev/mocika/shield/smoke/SecondaryMarker.java"
"$D8" --min-api "$MIN_API" --lib "$ANDROID_JAR" --output "$SECONDARY_DEX" \
    "$SECONDARY_CLASSES/dev/mocika/shield/smoke/SecondaryMarker.class"

mkdir -p "$(dirname "$OUTPUT_APK")"
cp "$INPUT_APK" "$OUTPUT_APK"
cp "$SECONDARY_DEX/classes.dex" "$WORK_DIR/classes2.dex"
zip -qj "$OUTPUT_APK" "$WORK_DIR/classes2.dex"

if ! unzip -l "$OUTPUT_APK" | awk '$NF == "classes2.dex" { found = 1 } END { exit !found }'; then
    echo "生成的 Smoke APK 缺少 classes2.dex" >&2
    exit 1
fi

echo "Smoke 双 DEX APK 已生成：$OUTPUT_APK"
