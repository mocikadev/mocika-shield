#!/usr/bin/env bash

# 构建 Android 4.4 兼容资源包，不覆盖标准 resources.zip。

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MAPPING_FILE="$PROJECT_ROOT/shield-stub/build/outputs/mapping/release/mapping.txt"
STANDARD_RESOURCES="$PROJECT_ROOT/shield-stub/build/outputs/resources/resources.zip"
API19_OUTPUT="$PROJECT_ROOT/shield-stub/build/experiments/api19"
LEGACY_RESOURCES="$PROJECT_ROOT/shield-stub/build/outputs/resources/resources-api19.zip"

if [[ "${SKIP_STANDARD_STUB_BUILD:-0}" != "1" ]]; then
    "$PROJECT_ROOT/scripts/build-stub.sh"
fi

ANDROID_SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
if [[ -z "$ANDROID_SDK" ]]; then
    echo "错误：未设置 ANDROID_HOME 或 ANDROID_SDK_ROOT"
    exit 1
fi
if [[ ! -f "$ANDROID_SDK/ndk/25.2.9519653/source.properties" ]]; then
    echo "正在安装 Android 4.4 兼容构建所需的 NDK r25c..."
    sdkmanager "ndk;25.2.9519653"
fi
if ! rustup run 1.77.2 rustc --version >/dev/null 2>&1; then
    rustup toolchain install 1.77.2 --profile minimal
fi
if ! rustup target list --toolchain 1.77.2 --installed | grep -qx armv7-linux-androideabi; then
    rustup target add --toolchain 1.77.2 armv7-linux-androideabi
fi

for required in "$MAPPING_FILE" "$STANDARD_RESOURCES"; do
    if [[ ! -f "$required" ]]; then
        echo "错误：缺少标准 Stub 构建产物：$required"
        exit 1
    fi
done

parse_mapping_class() {
    local original_class="$1"
    grep "^${original_class} ->" "$MAPPING_FILE" | sed 's/.*-> //' | tr -d ':'
}

parse_mapping_method() {
    local original_class="$1"
    local original_method="$2"
    awk "/^${original_class} ->/{found=1} found && / ${original_method}\\(/{print \$NF; exit}" \
        "$MAPPING_FILE"
}

ORIGINAL_LD="dev.mocika.shield.loader.Ld"
OBFUSCATED_LD="$(parse_mapping_class "$ORIGINAL_LD")"
OBFUSCATED_INJECT="$(parse_mapping_method "$ORIGINAL_LD" "p")"
OBFUSCATED_EXTRACT="$(parse_mapping_method "$ORIGINAL_LD" "q")"
OBFUSCATED_SIGNATURE="$(parse_mapping_method "$ORIGINAL_LD" "getSignatureSha256")"

if [[ -z "$OBFUSCATED_LD" ]]; then
    echo "错误：无法从 mapping.txt 解析 Ld 类名"
    exit 1
fi

STUB_BINLOADER_CLASS="${OBFUSCATED_LD//.//}" \
STUB_METHOD_INJECT_DEX="${OBFUSCATED_INJECT:-p}" \
STUB_METHOD_EXTRACT_DECRYPT="${OBFUSCATED_EXTRACT:-q}" \
STUB_METHOD_GET_SIG="${OBFUSCATED_SIGNATURE:-getSignatureSha256}" \
    "$PROJECT_ROOT/scripts/verify-android-api19-native.sh"

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/mocika-api19-resources.XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT

unzip -q "$STANDARD_RESOURCES" -d "$WORK_DIR"
cp "$API19_OUTPUT/jniLibs/armeabi-v7a/libmocikashield.so" \
    "$WORK_DIR/lib/armeabi-v7a/libmocikashield.so"
perl -pi -e 's/"min_android_api": 21/"min_android_api": 19/' "$WORK_DIR/metadata.json"

mkdir -p "$API19_OUTPUT"
rm -f "$LEGACY_RESOURCES"
(
    cd "$WORK_DIR"
    zip -qr "$LEGACY_RESOURCES" stub-classes.dex lib metadata.json
)

echo "Android API 19 兼容资源包构建完成：$LEGACY_RESOURCES"
