#!/usr/bin/env bash

# 生成仅供内部剖析的内存候选资源，并恢复不含诊断代码的正常资源。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="$ROOT/shield-stub/build/outputs/resources"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/mocika-memory-profile.XXXXXX")"
LEGACY_BYTE_ARRAY="${MOCIKA_LEGACY_BYTE_ARRAY:-0}"
PROFILE_NAME="resources-memory-direct-profile.zip"
if [[ "$LEGACY_BYTE_ARRAY" == "1" ]]; then
    PROFILE_NAME="resources-memory-legacy-profile.zip"
fi

for command in cp grep make mktemp readlink strings unzip; do
    command -v "$command" >/dev/null || { echo "缺少命令：$command" >&2; exit 1; }
done

STANDARD_LINK="$OUTPUT_DIR/resources.zip"
MEMORY_RESOURCE="$OUTPUT_DIR/resources-memory.zip"
API19_RESOURCE="$OUTPUT_DIR/resources-api19.zip"
[[ -L "$STANDARD_LINK" && -f "$MEMORY_RESOURCE" && -f "$API19_RESOURCE" ]] || {
    echo "缺少正常资源，请先执行 make build-stub" >&2
    exit 1
}
STANDARD_NAME="$(readlink "$STANDARD_LINK")"
cp "$OUTPUT_DIR/$STANDARD_NAME" "$WORK/standard.zip"
cp "$MEMORY_RESOURCE" "$WORK/memory.zip"
cp "$API19_RESOURCE" "$WORK/api19.zip"

restore_normal_resources() {
    cp "$WORK/standard.zip" "$OUTPUT_DIR/$STANDARD_NAME"
    cp "$WORK/memory.zip" "$MEMORY_RESOURCE"
    cp "$WORK/api19.zip" "$API19_RESOURCE"
}
cleanup() {
    restore_normal_resources
    rm -rf "$WORK"
}
trap cleanup EXIT

echo "构建带阶段诊断的内存候选资源..."
MOCIKA_RUNTIME_PROFILE=1 MOCIKA_LEGACY_BYTE_ARRAY="$LEGACY_BYTE_ARRAY" make -C "$ROOT" build-stub
cp "$OUTPUT_DIR/resources-memory.zip" "$WORK/$PROFILE_NAME"

echo "恢复正常标准资源和内存候选资源..."
make -C "$ROOT" build-stub
cp "$WORK/$PROFILE_NAME" "$OUTPUT_DIR/$PROFILE_NAME"

if unzip -p "$OUTPUT_DIR/resources.zip" stub-classes.dex \
        | strings | grep -Eq 'stage=native_decrypt|stage=class_loader|mxp'; then
    echo "错误：正常资源仍包含内存剖析标记" >&2
    exit 1
fi
if ! unzip -p "$OUTPUT_DIR/$PROFILE_NAME" stub-classes.dex \
        | strings | grep -Eq 'native_decrypt|class_loader'; then
    echo "错误：剖析资源缺少阶段标记" >&2
    exit 1
fi

if [[ "$LEGACY_BYTE_ARRAY" != "1" ]] \
        && ! unzip -p "$OUTPUT_DIR/$PROFILE_NAME" stub-classes.dex \
            | strings | grep -q 'native_direct'; then
    echo "错误：直接缓冲区原型资源缺少阶段标记" >&2
    exit 1
fi

echo "内部剖析资源：$OUTPUT_DIR/$PROFILE_NAME"
