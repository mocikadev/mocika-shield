#!/usr/bin/env bash

# 验证内存候选资源在 API 28～30 的认证文件路径和 API 31 以上的内存路径。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FIXTURE="$ROOT/tests/fixtures/android-smoke-app"
PACKAGE_NAME="dev.mocika.shield.smoke"
COMPONENT="$PACKAGE_NAME/.MainActivity"
RESOURCES="$ROOT/shield-stub/build/outputs/resources/resources-memory.zip"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/mocika-memory-runtime-e2e.XXXXXX")"
PASSWORD="mocika-test-123"
trap 'rm -rf "$WORK"' EXIT

for command in adb cargo java keytool unzip; do
    command -v "$command" >/dev/null || { echo "缺少命令：$command" >&2; exit 1; }
done

ADB_ARGS=()
if [[ -n "${ANDROID_SERIAL:-}" ]]; then
    ADB_ARGS=(-s "$ANDROID_SERIAL")
fi

adb "${ADB_ARGS[@]}" get-state >/dev/null
SDK_VERSION="$(adb "${ADB_ARGS[@]}" shell getprop ro.build.version.sdk | tr -d '\r')"
if (( SDK_VERSION < 28 )); then
    echo "错误：内存候选回归要求 API 28 以上，当前为 API $SDK_VERSION" >&2
    exit 1
fi

if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
    make -C "$ROOT" build-stub
    "$ROOT/shield-stub/gradlew" -p "$FIXTURE" :app:assembleDebug --no-daemon
    cargo build --release -p shield-cli --manifest-path "$ROOT/Cargo.toml"
fi

test -f "$RESOURCES" || { echo "缺少内存候选资源：$RESOURCES" >&2; exit 1; }
BASE_UNSIGNED="$FIXTURE/app/build/outputs/apk/debug/app-debug.apk"
test -f "$BASE_UNSIGNED" || { echo "缺少测试 APK，请取消 SKIP_BUILD 后重试" >&2; exit 1; }

MULTIDEX_UNSIGNED="$WORK/input-multidex-unsigned.apk"
SIGNED="$WORK/input-signed.apk"
PROTECTED="$WORK/output-protected.apk"
FINAL="$WORK/output-protected-signed.apk"
KEYSTORE="$WORK/smoke.p12"

"$ROOT/tests/scripts/build-smoke-multidex.sh" "$BASE_UNSIGNED" "$MULTIDEX_UNSIGNED" 28
keytool -genkeypair -noprompt -storetype PKCS12 \
    -keystore "$KEYSTORE" -storepass "$PASSWORD" -keypass "$PASSWORD" \
    -alias smoke -keyalg RSA -keysize 2048 -validity 3650 \
    -dname "CN=Mocika Shield Memory Runtime, O=MocikaDev, C=CN"
java -jar "$ROOT/tools/apksigner.jar" sign \
    --ks "$KEYSTORE" --ks-pass "pass:$PASSWORD" --ks-key-alias smoke \
    --out "$SIGNED" "$MULTIDEX_UNSIGNED"
"$ROOT/target/release/shield" protect \
    --input "$SIGNED" --output "$PROTECTED" --resources "$RESOURCES"
java -jar "$ROOT/tools/apksigner.jar" sign \
    --ks "$KEYSTORE" --ks-pass "pass:$PASSWORD" --ks-key-alias smoke \
    --out "$FINAL" "$PROTECTED"
java -jar "$ROOT/tools/apksigner.jar" verify --verbose "$FINAL" >/dev/null

adb "${ADB_ARGS[@]}" uninstall "$PACKAGE_NAME" >/dev/null 2>&1 || true
adb "${ADB_ARGS[@]}" install "$FINAL"
adb "${ADB_ARGS[@]}" logcat -c
adb "${ADB_ARGS[@]}" shell am start -W -n "$COMPONENT" >/dev/null

EXPECTED_MARKERS=(
    MOCIKA_SMOKE_APPLICATION_OK
    MOCIKA_SMOKE_PROVIDER_OK
    MOCIKA_SMOKE_ACTIVITY_OK
    MOCIKA_SMOKE_RECEIVER_OK
    MOCIKA_SMOKE_REMOTE_SERVICE_OK
    MOCIKA_SMOKE_SECONDARY_OK
    MOCIKA_SMOKE_FACTORY_APPLICATION
    MOCIKA_SMOKE_FACTORY_PROVIDER
    MOCIKA_SMOKE_FACTORY_ACTIVITY
    MOCIKA_SMOKE_FACTORY_RECEIVER
    MOCIKA_SMOKE_FACTORY_SERVICE
)

MISSING=1
for _ in $(seq 1 30); do
    LOGS="$(adb "${ADB_ARGS[@]}" logcat -d -s MocikaSmoke:I AndroidRuntime:E '*:S')"
    MISSING=0
    for marker in "${EXPECTED_MARKERS[@]}"; do
        grep -q "$marker" <<< "$LOGS" || MISSING=1
    done
    if (( MISSING == 0 )); then
        break
    fi
    sleep 1
done
if (( MISSING != 0 )); then
    echo "错误：API $SDK_VERSION 未观察到完整组件和多 DEX 标记" >&2
    printf '%s\n' "$LOGS" >&2
    exit 1
fi

PRIVATE_FILES="$(adb "${ADB_ARGS[@]}" shell run-as "$PACKAGE_NAME" find . -type f 2>/dev/null | tr -d '\r')"
if (( SDK_VERSION <= 30 )); then
    grep -q '^./app_app_dex/.*\.dex$' <<< "$PRIVATE_FILES" || {
        echo "错误：API $SDK_VERSION 未生成认证文件缓存" >&2
        printf '%s\n' "$PRIVATE_FILES" >&2
        exit 1
    }
    if grep -q '^./no_backup/runtime_state/' <<< "$PRIVATE_FILES"; then
        echo "错误：API $SDK_VERSION 不应生成内存运行状态" >&2
        exit 1
    fi
    MODE="认证文件"
else
    STATE_COUNT="$(grep -c '^./no_backup/runtime_state/' <<< "$PRIVATE_FILES" || true)"
    if (( STATE_COUNT < 2 )); then
        echo "错误：API $SDK_VERSION 缺少主进程和远程进程认证状态" >&2
        printf '%s\n' "$PRIVATE_FILES" >&2
        exit 1
    fi
    if grep -Eq '(^|/)(app_app_dex|[^/]*\.dex$)' <<< "$PRIVATE_FILES"; then
        echo "错误：API $SDK_VERSION 内存路径产生了明文 DEX 文件" >&2
        printf '%s\n' "$PRIVATE_FILES" >&2
        exit 1
    fi
    MODE="内存"
fi

echo "Android API $SDK_VERSION ${MODE}路径端到端回归通过"
