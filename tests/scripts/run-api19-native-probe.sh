#!/usr/bin/env bash

# 在已经连接的 Android 4.4/API 19 armeabi-v7a 设备上验证生产 Native Stub。
# 只验证系统加载与 JNI_OnLoad，不执行 DEX 解密或 Dalvik 注入。

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FIXTURE="$PROJECT_ROOT/tests/fixtures/android-api19-native-probe"
PACKAGE_NAME="dev.mocika.shield.api19probe"
COMPONENT="$PACKAGE_NAME/.MainActivity"
APK="$FIXTURE/app/build/outputs/apk/debug/app-debug.apk"
LOG_MARKER="MOCIKA_API19_NATIVE_OK"

for command in adb java cargo rustup; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "错误：缺少命令 $command"
        exit 1
    fi
done

ADB_ARGS=()
if [[ -n "${ANDROID_SERIAL:-}" ]]; then
    ADB_ARGS=(-s "$ANDROID_SERIAL")
fi

DEVICE_COUNT="$(adb devices | awk 'NR > 1 && $2 == "device" {count++} END {print count + 0}')"
if [[ -z "${ANDROID_SERIAL:-}" && "$DEVICE_COUNT" -ne 1 ]]; then
    echo "错误：当前在线设备数为 $DEVICE_COUNT，请只连接一个设备或设置 ANDROID_SERIAL"
    exit 1
fi

SDK_VERSION="$(adb "${ADB_ARGS[@]}" shell getprop ro.build.version.sdk | tr -d '\r')"
CPU_ABI="$(adb "${ADB_ARGS[@]}" shell getprop ro.product.cpu.abi | tr -d '\r')"
if [[ "$SDK_VERSION" != "19" ]]; then
    echo "错误：当前设备 API 为 $SDK_VERSION，要求 API 19"
    exit 1
fi
if [[ "$CPU_ABI" != "armeabi-v7a" ]]; then
    echo "错误：当前设备 ABI 为 $CPU_ABI，要求 armeabi-v7a"
    exit 1
fi

"$PROJECT_ROOT/scripts/verify-android-api19-native.sh"
"$PROJECT_ROOT/shield-stub/gradlew" \
    -p "$FIXTURE" \
    :app:assembleDebug \
    --no-daemon

if [[ ! -f "$APK" ]]; then
    echo "错误：未找到测试 APK：$APK"
    exit 1
fi

adb "${ADB_ARGS[@]}" install -r "$APK" >/dev/null
adb "${ADB_ARGS[@]}" shell am force-stop "$PACKAGE_NAME" >/dev/null 2>&1 || true
adb "${ADB_ARGS[@]}" logcat -c
adb "${ADB_ARGS[@]}" shell am start -W -n "$COMPONENT" >/dev/null

for _ in $(seq 1 30); do
    if adb "${ADB_ARGS[@]}" logcat -d -s MocikaApi19Probe:I AndroidRuntime:E '*:S' \
        | grep -q "$LOG_MARKER"; then
        echo "Android API 19 Native 加载验证通过"
        echo "设备：API $SDK_VERSION / $CPU_ABI"
        adb "${ADB_ARGS[@]}" logcat -d -s MocikaApi19Probe:I AndroidRuntime:E '*:S'
        exit 0
    fi
    sleep 1
done

echo "错误：未在日志中找到 $LOG_MARKER"
adb "${ADB_ARGS[@]}" logcat -d -s MocikaApi19Probe:V AndroidRuntime:E linker:V '*:S'
exit 1
