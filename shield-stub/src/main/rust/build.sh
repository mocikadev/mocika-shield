#!/bin/bash

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

echo "========================================"
echo "构建 Mocika Shield Native 库 (Rust)"
echo "========================================"

if ! command -v cargo-ndk &> /dev/null; then
    echo "错误: 未找到 cargo-ndk"
    echo "请安装: cargo install cargo-ndk"
    exit 1
fi

if [ -z "$ANDROID_NDK_ROOT" ] && [ -z "$NDK_HOME" ]; then
    echo "错误: 未设置 ANDROID_NDK_ROOT 或 NDK_HOME"
    exit 1
fi

OUTPUT_DIR="../../../build/jniLibs"
mkdir -p "$OUTPUT_DIR"

echo "构建目标: arm64-v8a, armeabi-v7a, x86, x86_64"
echo

cargo ndk \
    --platform 21 \
    --target aarch64-linux-android \
    --target armv7-linux-androideabi \
    --target i686-linux-android \
    --target x86_64-linux-android \
    -o "$OUTPUT_DIR" \
    build --release

echo
echo "========================================"
echo "✓ 构建完成"
echo "========================================"
echo "产物位置:"
ls -lh "$OUTPUT_DIR"/*/libmocikashield.so
