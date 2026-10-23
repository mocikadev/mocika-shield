#!/usr/bin/env bash

# 构建 Android 4.4 兼容资源包，不覆盖标准 resources.zip。

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MAPPING_FILE="$PROJECT_ROOT/shield-stub/build/outputs/mapping/release/mapping.txt"
STANDARD_RESOURCES="$PROJECT_ROOT/shield-stub/build/outputs/resources/resources.zip"
API19_OUTPUT="$PROJECT_ROOT/shield-stub/build/experiments/api19"
LEGACY_RESOURCES="$PROJECT_ROOT/shield-stub/build/outputs/resources/resources-api19.zip"
CANDIDATE_RESOURCES="$API19_OUTPUT/resources-api19-candidate.zip"

if [[ "${SKIP_STANDARD_STUB_BUILD:-0}" != "1" ]]; then
    "$PROJECT_ROOT/scripts/build-stub.sh"
fi

ANDROID_SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
if [[ -z "$ANDROID_SDK" ]]; then
    echo "错误：未设置 ANDROID_HOME 或 ANDROID_SDK_ROOT"
    exit 1
fi
if [[ ! -f "$ANDROID_SDK/ndk/25.2.9519653/source.properties" ]]; then
    echo "错误：未安装 Android 4.4 兼容构建所需的 NDK r25c（25.2.9519653）"
    echo '请先执行：sdkmanager "ndk;25.2.9519653"'
    exit 1
fi
if ! rustup run 1.77.2 rustc --version >/dev/null 2>&1; then
    echo "错误：未安装 Android 4.4 兼容构建所需的 Rust 1.77.2"
    echo "请先执行：rustup toolchain install 1.77.2 --profile minimal --target armv7-linux-androideabi"
    exit 1
fi
if ! rustup target list --toolchain 1.77.2 --installed | grep -qx armv7-linux-androideabi; then
    echo "错误：Rust 1.77.2 未安装 armv7-linux-androideabi target"
    echo "请先执行：rustup target add --toolchain 1.77.2 armv7-linux-androideabi"
    exit 1
fi

for required in "$MAPPING_FILE" "$STANDARD_RESOURCES"; do
    if [[ ! -f "$required" ]]; then
        echo "错误：缺少标准 Stub 构建产物：$required"
        exit 1
    fi
done

PYTHON_CMD="${PYTHON:-python3}"
BINDINGS_FILE="$PROJECT_ROOT/shield-stub/build/outputs/mapping/release/jni-bindings.env"
"$PYTHON_CMD" "$PROJECT_ROOT/scripts/stub_bindings.py" env "$MAPPING_FILE" > "$BINDINGS_FILE"
(
    while IFS='=' read -r binding_key binding_value; do
        binding_value="${binding_value%$'\r'}"
        export "$binding_key=$binding_value"
    done < "$BINDINGS_FILE"
    "$PROJECT_ROOT/scripts/verify-android-api19-native.sh"
)

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/mocika-api19-resources.XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT

unzip -q "$STANDARD_RESOURCES" -d "$WORK_DIR"
find "$WORK_DIR" -type f -name '.DS_Store' -delete
rm -rf "$WORK_DIR/__MACOSX"
cp "$API19_OUTPUT/jniLibs/armeabi-v7a/libmocikashield.so" \
    "$WORK_DIR/lib/armeabi-v7a/libmocikashield.so"
perl -pi -e 's/"min_android_api": 21/"min_android_api": 19/' "$WORK_DIR/metadata.json"
if ! grep -q '"min_android_api": 19' "$WORK_DIR/metadata.json"; then
    echo "错误：Android 4.4 兼容资源元数据未正确设置 min_android_api=19"
    exit 1
fi

mkdir -p "$API19_OUTPUT"
rm -f "$CANDIDATE_RESOURCES"
if [[ "${OS:-}" == "Windows_NT" ]]; then
    WINDOWS_WORK_DIR="$(cygpath -w "$WORK_DIR")"
    WINDOWS_RESOURCES="$(cygpath -w "$CANDIDATE_RESOURCES")"
    MOCIKA_WORK_DIR="$WINDOWS_WORK_DIR" \
    MOCIKA_RESOURCES="$WINDOWS_RESOURCES" \
        powershell.exe -NoProfile -NonInteractive -Command '
            $ErrorActionPreference = "Stop"
            $files = @(
                (Join-Path $env:MOCIKA_WORK_DIR "stub-classes.dex"),
                (Join-Path $env:MOCIKA_WORK_DIR "lib"),
                (Join-Path $env:MOCIKA_WORK_DIR "metadata.json")
            )
            Compress-Archive -Path $files -DestinationPath $env:MOCIKA_RESOURCES -Force
        '
else
    (
        cd "$WORK_DIR"
        zip -qr "$CANDIDATE_RESOURCES" stub-classes.dex lib metadata.json
    )
fi

if ! unzip -tq "$CANDIDATE_RESOURCES" >/dev/null; then
    echo "错误：Android 4.4 兼容资源包 ZIP 完整性校验失败"
    exit 1
fi
for required_entry in stub-classes.dex lib/armeabi-v7a/libmocikashield.so metadata.json; do
    if ! unzip -Z1 "$CANDIDATE_RESOURCES" \
        | awk -v required="$required_entry" '$0 == required { found = 1 } END { exit !found }'; then
        echo "错误：Android 4.4 兼容资源包缺少 $required_entry"
        exit 1
    fi
done

"$PYTHON_CMD" "$PROJECT_ROOT/scripts/stub_bindings.py" audit "$MAPPING_FILE" \
    "$CANDIDATE_RESOURCES" --standard "$STANDARD_RESOURCES"
mv -f "$CANDIDATE_RESOURCES" "$LEGACY_RESOURCES"
echo "Android API 19 兼容资源包构建完成：$LEGACY_RESOURCES"
