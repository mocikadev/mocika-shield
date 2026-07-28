#!/usr/bin/env bash

# 验证现有 Rust Native Stub 能否使用 NDK r25c、API 19 构建为 armeabi-v7a。
# 该脚本只输出实验产物，不修改正式 resources.zip 或 build/jniLibs。

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RUST_DIR="$PROJECT_ROOT/shield-stub/compat/api19-rust"
OUTPUT_ROOT="$PROJECT_ROOT/shield-stub/build/experiments/api19"
NDK_VERSION="25.2.9519653"
TARGET="armeabi-v7a"
RUST_TARGET="armv7-linux-androideabi"
RUST_TOOLCHAIN="${ANDROID_API19_RUST_TOOLCHAIN:-1.77.2}"

ANDROID_SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
if [[ -z "$ANDROID_SDK" ]]; then
    echo "错误：未设置 ANDROID_HOME 或 ANDROID_SDK_ROOT"
    exit 1
fi

NDK_ROOT="${ANDROID_NDK_API19_ROOT:-$ANDROID_SDK/ndk/$NDK_VERSION}"
if [[ ! -f "$NDK_ROOT/source.properties" ]]; then
    echo "错误：未找到 Android NDK r25c（$NDK_VERSION）"
    echo "期望路径：$NDK_ROOT"
    echo "可执行：sdkmanager \"ndk;$NDK_VERSION\""
    exit 1
fi

if ! grep -q "Pkg.Revision = $NDK_VERSION" "$NDK_ROOT/source.properties"; then
    echo "错误：NDK 路径与要求的版本 $NDK_VERSION 不一致：$NDK_ROOT"
    exit 1
fi

if ! command -v cargo-ndk >/dev/null 2>&1; then
    echo "错误：未安装 cargo-ndk"
    exit 1
fi

if ! rustup run "$RUST_TOOLCHAIN" rustc --version >/dev/null 2>&1; then
    echo "错误：未安装 Android 4.4 兼容 Rust 工具链 $RUST_TOOLCHAIN"
    echo "可执行：rustup toolchain install $RUST_TOOLCHAIN --profile minimal --target $RUST_TARGET"
    exit 1
fi

if ! rustup target list --toolchain "$RUST_TOOLCHAIN" --installed | grep -qx "$RUST_TARGET"; then
    echo "错误：未安装 Rust 目标 $RUST_TARGET"
    echo "可执行：rustup target add --toolchain $RUST_TOOLCHAIN $RUST_TARGET"
    exit 1
fi


READELF_CANDIDATES=("$NDK_ROOT"/toolchains/llvm/prebuilt/*/bin/llvm-readelf)
READELF="${READELF_CANDIDATES[0]}"
if [[ ! -x "$READELF" ]]; then
    echo "错误：NDK 中未找到 llvm-readelf"
    exit 1
fi

JNI_OUTPUT="$OUTPUT_ROOT/jniLibs"
TARGET_OUTPUT="$OUTPUT_ROOT/rust-target"
SO_FILE="$JNI_OUTPUT/$TARGET/libmocikashield.so"
REPORT_FILE="$OUTPUT_ROOT/elf-report.txt"

mkdir -p "$JNI_OUTPUT" "$TARGET_OUTPUT"

echo "使用 NDK：$NDK_ROOT"
echo "使用 Rust：$RUST_TOOLCHAIN"
echo "目标平台：Android API 19 / $TARGET"
echo "实验输出：$OUTPUT_ROOT"

(
    cd "$RUST_DIR"
    env -u ANDROID_NDK_HOME -u NDK_HOME \
        ANDROID_NDK_ROOT="$NDK_ROOT" \
        CARGO_TARGET_DIR="$TARGET_OUTPUT" \
        RUSTFLAGS="--remap-path-prefix $PROJECT_ROOT=." \
        cargo "+$RUST_TOOLCHAIN" ndk \
            --platform 19 \
            --target "$TARGET" \
            -o "$JNI_OUTPUT" \
            build --release --locked
)

if [[ ! -f "$SO_FILE" ]]; then
    echo "错误：构建完成后未找到 $SO_FILE"
    exit 1
fi

MACHINE="$($READELF -h "$SO_FILE" | awk -F: '/Machine:/{sub(/^[[:space:]]+/, "", $2); print $2}')"
if [[ "$MACHINE" != "ARM" ]]; then
    echo "错误：ELF 架构不是 ARM：$MACHINE"
    exit 1
fi

ANDROID_NOTE="$($READELF -n "$SO_FILE")"
if ! grep -q 'description data: 13 00 00 00' <<< "$ANDROID_NOTE"; then
    echo "错误：ELF 未标记 Android API 19"
    exit 1
fi

ARM_ATTRIBUTES="$($READELF -A "$SO_FILE")"
if ! grep -q 'Description: ARM v7' <<< "$ARM_ATTRIBUTES"; then
    echo "错误：ELF 未标记为 ARMv7"
    exit 1
fi
if ! grep -q 'Description: NEONv1' <<< "$ARM_ATTRIBUTES"; then
    echo "错误：ELF 未包含预期的 NEON 属性"
    exit 1
fi

NEEDED_LIBS=()
while IFS= read -r needed; do
    NEEDED_LIBS+=("$needed")
done < <(
    "$READELF" -d "$SO_FILE" \
        | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' \
        | sort -u
)
ALLOWED_LIBS=("libc.so" "libdl.so" "liblog.so" "libm.so")
for needed in "${NEEDED_LIBS[@]}"; do
    allowed=false
    for expected in "${ALLOWED_LIBS[@]}"; do
        if [[ "$needed" == "$expected" ]]; then
            allowed=true
            break
        fi
    done
    if [[ "$allowed" != true ]]; then
        echo "错误：发现未纳入 API 19 验证范围的动态依赖：$needed"
        exit 1
    fi
done


if "$READELF" --dyn-syms --wide "$SO_FILE" \
    | awk '$7 == "UND" {print $8}' \
    | grep -Eq '^dl_iterate_phdr(@.*)?$'; then
    echo "错误：产物引用 Android 5.0 才提供的 dl_iterate_phdr，不能在 API 19 加载"
    exit 1
fi

{
    echo "Android API 19 Native Stub ELF 审计"
    echo "NDK：${NDK_VERSION}（r25c）"
    echo "ABI：${TARGET}"
    echo "Rust 目标：${RUST_TARGET}"
    echo "Rust 工具链：${RUST_TOOLCHAIN}"
    echo
    "$READELF" -h "$SO_FILE"
    echo
    echo "动态依赖："
    "$READELF" -d "$SO_FILE" | grep -E 'NEEDED|SONAME|FLAGS' || true
    echo
    echo "Android 标记："
    printf '%s\n' "$ANDROID_NOTE"
    echo
    echo "ARM 属性："
    printf '%s\n' "$ARM_ATTRIBUTES"
    echo
    echo "加载段与对齐："
    "$READELF" -l "$SO_FILE" | grep -E 'LOAD|Align|GNU_RELRO|GNU_STACK' || true
    echo
    echo "未定义动态符号："
    "$READELF" --dyn-syms --wide "$SO_FILE" \
        | awk '$7 == "UND" {print $8}' \
        | sed '/^$/d' \
        | sort -u
} > "$REPORT_FILE"

echo "验证通过：$SO_FILE"
echo "动态依赖：${NEEDED_LIBS[*]}"
echo "审计报告：$REPORT_FILE"
echo "注意：r25c 的 armeabi-v7a 产物要求目标 CPU 支持 NEON。"
