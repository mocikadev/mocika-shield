#!/usr/bin/env bash

# 在 Android 4.4/API 19 armeabi-v7a 设备验证完整加固、解密、Dalvik 注入和 Application 恢复。

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FIXTURE="$PROJECT_ROOT/tests/fixtures/android-smoke-app"
PACKAGE_NAME="dev.mocika.shield.smoke"
COMPONENT="$PACKAGE_NAME/.MainActivity"
RESOURCES="$PROJECT_ROOT/shield-stub/build/experiments/api19/resources-api19.zip"
WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/mocika-api19-e2e.XXXXXX")"
PASSWORD="mocika-test-123"
trap 'rm -rf "$WORK_DIR"' EXIT

ADB_ARGS=()
if [[ -n "${ANDROID_SERIAL:-}" ]]; then
    ADB_ARGS=(-s "$ANDROID_SERIAL")
fi

SDK_VERSION="$(adb "${ADB_ARGS[@]}" shell getprop ro.build.version.sdk | tr -d '\r')"
CPU_ABI="$(adb "${ADB_ARGS[@]}" shell getprop ro.product.cpu.abi | tr -d '\r')"
if [[ "$SDK_VERSION" != "19" || "$CPU_ABI" != "armeabi-v7a" ]]; then
    echo "错误：要求 API 19 / armeabi-v7a，当前为 API $SDK_VERSION / $CPU_ABI"
    exit 1
fi

SKIP_STANDARD_STUB_BUILD="${SKIP_STANDARD_STUB_BUILD:-0}" \
    "$PROJECT_ROOT/scripts/build-android-api19-resources.sh"
"$PROJECT_ROOT/shield-stub/gradlew" -p "$FIXTURE" :app:assembleRelease --no-daemon
cargo build --release -p shield-cli --manifest-path "$PROJECT_ROOT/Cargo.toml"

UNSIGNED="$FIXTURE/app/build/outputs/apk/release/app-release-unsigned.apk"
MULTIDEX_UNSIGNED="$WORK_DIR/input-multidex-unsigned.apk"
SIGNED="$WORK_DIR/input-signed.apk"
PROTECTED="$WORK_DIR/output-protected.apk"
FINAL="$WORK_DIR/output-protected-signed.apk"
KEYSTORE="$WORK_DIR/smoke.p12"

"$PROJECT_ROOT/tests/scripts/build-smoke-multidex.sh" \
    "$UNSIGNED" "$MULTIDEX_UNSIGNED" 19

keytool -genkeypair -noprompt -storetype PKCS12 \
    -keystore "$KEYSTORE" -storepass "$PASSWORD" -keypass "$PASSWORD" \
    -alias smoke -keyalg RSA -keysize 2048 -validity 3650 \
    -dname "CN=Mocika Shield API 19, O=MocikaDev, C=CN"
java -jar "$PROJECT_ROOT/tools/apksigner.jar" sign \
    --ks "$KEYSTORE" --ks-pass "pass:$PASSWORD" --ks-key-alias smoke \
    --out "$SIGNED" "$MULTIDEX_UNSIGNED"

"$PROJECT_ROOT/target/release/shield" protect \
    --input "$SIGNED" --output "$PROTECTED" --resources "$RESOURCES"
java -jar "$PROJECT_ROOT/tools/apksigner.jar" sign \
    --ks "$KEYSTORE" --ks-pass "pass:$PASSWORD" --ks-key-alias smoke \
    --out "$FINAL" "$PROTECTED"

# 每次生成临时测试证书，先移除上一次测试包，避免签名不同导致覆盖安装被拒绝。
adb "${ADB_ARGS[@]}" uninstall "$PACKAGE_NAME" >/dev/null 2>&1 || true
INSTALL_RESULT="$(adb "${ADB_ARGS[@]}" install "$FINAL")"
if ! grep -q '^Success' <<< "$INSTALL_RESULT"; then
    echo "错误：安装 API 19 测试 APK 失败"
    printf '%s\n' "$INSTALL_RESULT"
    exit 1
fi

verify_launch() {
    local scenario="$1"
    adb "${ADB_ARGS[@]}" logcat -c
    adb "${ADB_ARGS[@]}" shell am force-stop "$PACKAGE_NAME" >/dev/null 2>&1 || true
    adb "${ADB_ARGS[@]}" shell am start -W -n "$COMPONENT" >/dev/null
    for _ in $(seq 1 30); do
        local logs
        logs="$(adb "${ADB_ARGS[@]}" logcat -d -s MocikaSmoke:I AndroidRuntime:E '*:S')"
        if grep -q 'MOCIKA_SMOKE_APPLICATION_OK' <<< "$logs" \
            && grep -q 'MOCIKA_SMOKE_ACTIVITY_OK' <<< "$logs" \
            && grep -q 'MOCIKA_SMOKE_SECONDARY_OK' <<< "$logs"; then
            echo "$scenario 验证通过"
            printf '%s\n' "$logs"
            return 0
        fi
        sleep 1
    done
    echo "错误：$scenario 未观察到完整启动标记"
    adb "${ADB_ARGS[@]}" logcat -d -s MocikaSmoke:V dx:V AndroidRuntime:E linker:V '*:S'
    return 1
}

verify_launch "首次安装"
adb "${ADB_ARGS[@]}" shell pm clear "$PACKAGE_NAME" >/dev/null
verify_launch "清除数据后启动"
UPGRADE_RESULT="$(adb "${ADB_ARGS[@]}" install -r "$FINAL")"
if ! grep -q '^Success' <<< "$UPGRADE_RESULT"; then
    echo "错误：同签名覆盖安装失败"
    printf '%s\n' "$UPGRADE_RESULT"
    exit 1
fi
verify_launch "同签名覆盖安装后启动"

if [[ -n "${API19_E2E_ARTIFACT:-}" ]]; then
    cp "$FINAL" "$API19_E2E_ARTIFACT"
    echo "保留跨版本测试产物：$API19_E2E_ARTIFACT"
fi

echo "Android API 19 端到端加固回归通过"
