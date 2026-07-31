#!/usr/bin/env bash

# 使用同一临时证书生成标准资源与内存候选资源的双 DEX 测试 APK。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FIXTURE="$ROOT/tests/fixtures/android-smoke-app"
WORK="${1:?请提供测试产物目录}"
PASSWORD="mocika-test-123"

for command in cargo java keytool python3 unzip zip; do
    command -v "$command" >/dev/null || { echo "缺少命令：$command" >&2; exit 1; }
done

STANDARD_RESOURCES="$ROOT/shield-stub/build/outputs/resources/resources.zip"
MEMORY_RESOURCES="$ROOT/shield-stub/build/outputs/resources/resources-memory.zip"
for resource in "$STANDARD_RESOURCES" "$MEMORY_RESOURCES"; do
    test -f "$resource" || { echo "缺少运行资源：$resource" >&2; exit 1; }
done
BASE_APK="$FIXTURE/app/build/outputs/apk/debug/app-debug.apk"
test -f "$BASE_APK" || { echo "缺少测试 APK，请先构建测试夹具" >&2; exit 1; }

MULTIDEX_UNSIGNED="$WORK/input-multidex-unsigned.apk"
SIGNED_INPUT="$WORK/input-signed.apk"
KEYSTORE="$WORK/smoke.p12"

"$ROOT/tests/scripts/build-smoke-multidex.sh" "$BASE_APK" "$MULTIDEX_UNSIGNED" 28
keytool -genkeypair -noprompt -storetype PKCS12 \
    -keystore "$KEYSTORE" -storepass "$PASSWORD" -keypass "$PASSWORD" \
    -alias smoke -keyalg RSA -keysize 2048 -validity 3650 \
    -dname "CN=Mocika Shield Memory Runtime, O=MocikaDev, C=CN"
java -jar "$ROOT/tools/apksigner.jar" sign \
    --ks "$KEYSTORE" --ks-pass "pass:$PASSWORD" --ks-key-alias smoke \
    --out "$SIGNED_INPUT" "$MULTIDEX_UNSIGNED"

protect_and_sign() {
    local resources="$1"
    local name="$2"
    local unsigned="$WORK/output-$name-unsigned.apk"
    local final="$WORK/output-$name-signed.apk"
    "$ROOT/target/release/shield" protect \
        --input "$SIGNED_INPUT" --output "$unsigned" --resources "$resources"
    java -jar "$ROOT/tools/apksigner.jar" sign \
        --ks "$KEYSTORE" --ks-pass "pass:$PASSWORD" --ks-key-alias smoke \
        --out "$final" "$unsigned"
    java -jar "$ROOT/tools/apksigner.jar" verify --verbose "$final" >/dev/null
}

protect_and_sign "$STANDARD_RESOURCES" standard
protect_and_sign "$MEMORY_RESOURCES" memory

create_budget_variant() {
    local name="$1"
    local payload_bytes="$2"
    local only_32_bit="${3:-0}"
    local source="$WORK/output-memory-signed.apk"
    local decoded="$WORK/$name-decoded"
    local unsigned="$WORK/output-$name-unsigned.apk"
    local final="$WORK/output-$name-signed.apk"
    java -jar "$ROOT/tools/apktool_3.0.1.jar" d "$source" -o "$decoded" -f --no-src >/dev/null
    python3 - "$decoded/AndroidManifest.xml" "$payload_bytes" <<'PY'
import pathlib
import re
import sys

manifest = pathlib.Path(sys.argv[1])
payload_bytes = sys.argv[2]
content = manifest.read_text(encoding="utf-8")
updated, count = re.subn(
    r'(android:name="dev\.mocika\.shield\.PAYLOAD_DEX_BYTES"\s+android:value=")bytes:\d+("\s*/>)',
    rf'\1bytes:{payload_bytes}\2',
    content,
)
if count != 1:
    raise SystemExit(f"预算元数据替换数量异常：{count}")
manifest.write_text(updated, encoding="utf-8")
PY
    java -jar "$ROOT/tools/apktool_3.0.1.jar" b "$decoded" -o "$unsigned" -f >/dev/null
    if [[ "$only_32_bit" == "1" ]]; then
        zip -d "$unsigned" 'lib/arm64-v8a/*' 'lib/x86_64/*' >/dev/null
    fi
    java -jar "$ROOT/tools/apksigner.jar" sign \
        --ks "$KEYSTORE" --ks-pass "pass:$PASSWORD" --ks-key-alias smoke \
        --out "$final" "$unsigned"
    java -jar "$ROOT/tools/apksigner.jar" verify --verbose "$final" >/dev/null
}

create_budget_variant memory-budget-denied 402653185
create_budget_variant memory-budget-32bit 67108865 1
