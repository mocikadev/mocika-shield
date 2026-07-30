#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROJECT_DIR="$ROOT_DIR/tests/fixtures/android-memory-loader-probe"
GRADLE="$ROOT_DIR/shield-stub/gradlew"
PACKAGE="dev.mocika.shield.memoryprobe"
COMPONENT="$PACKAGE/dev.mocika.shield.memorypayload.PayloadActivity"
TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/mocika-memory-probe.XXXXXX")"
ASSET_DIR="$TEMP_DIR/assets"

cleanup() {
  rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

ADB=(adb)
if [[ -n "${ANDROID_SERIAL:-}" ]]; then
  ADB+=("-s" "$ANDROID_SERIAL")
fi

SDK_INT="$("${ADB[@]}" shell getprop ro.build.version.sdk | tr -d '\r')"
if [[ ! "$SDK_INT" =~ ^[0-9]+$ ]] || (( SDK_INT < 29 )); then
  echo "内存 DEX 加载探针只支持 API 29 及以上设备，当前 API：${SDK_INT:-未知}" >&2
  exit 1
fi

ANDROID_SDK_DIR="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}"
if [[ -z "$ANDROID_SDK_DIR" ]]; then
  ANDROID_SDK_DIR="$HOME/Library/Android/sdk"
fi
D8="$(find "$ANDROID_SDK_DIR/build-tools" -type f -name d8 | sort -V | tail -1)"
if [[ -z "$D8" ]]; then
  echo "未找到 Android SDK d8" >&2
  exit 1
fi

"$GRADLE" -p "$PROJECT_DIR" :payload-main:assembleDebug :payload-secondary:assembleDebug --no-daemon
mkdir -p "$ASSET_DIR" "$TEMP_DIR/main-aar" "$TEMP_DIR/secondary-aar" "$TEMP_DIR/main-dex" "$TEMP_DIR/secondary-dex"
unzip -q "$PROJECT_DIR/payload-main/build/outputs/aar/payload-main-debug.aar" classes.jar -d "$TEMP_DIR/main-aar"
unzip -q "$PROJECT_DIR/payload-secondary/build/outputs/aar/payload-secondary-debug.aar" classes.jar -d "$TEMP_DIR/secondary-aar"
"$D8" --min-api 29 --output "$TEMP_DIR/main-dex" "$TEMP_DIR/main-aar/classes.jar"
"$D8" --min-api 29 --output "$TEMP_DIR/secondary-dex" "$TEMP_DIR/secondary-aar/classes.jar"
cp "$TEMP_DIR/main-dex/classes.dex" "$ASSET_DIR/payload-main.dex"
cp "$TEMP_DIR/secondary-dex/classes.dex" "$ASSET_DIR/payload-secondary.dex"

MEMORY_PROBE_ASSETS="$ASSET_DIR" "$GRADLE" -p "$PROJECT_DIR" :app:assembleDebug --no-daemon
APK="$PROJECT_DIR/app/build/outputs/apk/debug/app-debug.apk"

"${ADB[@]}" uninstall "$PACKAGE" >/dev/null 2>&1 || true
"${ADB[@]}" logcat -c
"${ADB[@]}" install "$APK" >/dev/null
"${ADB[@]}" shell am start -W -n "$COMPONENT" >/dev/null
sleep 2

LOGS="$("${ADB[@]}" logcat -d -s MOCIKA_MEMORY_PROBE:I AndroidRuntime:E '*:S')"
printf '%s\n' "$LOGS"

for marker in LOADER_REPLACED APPLICATION_OK:SECONDARY_OK PROVIDER_OK ACTIVITY_OK:SECONDARY_OK:NATIVE_OK SERVICE_OK DELAYED_OK:AFTER_GC; do
  if ! grep -Fq "$marker" <<<"$LOGS"; then
    echo "内存 DEX 加载探针缺少标记：$marker" >&2
    exit 1
  fi
done

LOADER_PROCESS_COUNT="$(grep -F "LOADER_REPLACED" <<<"$LOGS" | awk '{print $3}' | sort -u | wc -l | tr -d ' ')"
if (( LOADER_PROCESS_COUNT < 2 )); then
  echo "内存 DEX 加载探针未在主进程和远程 Service 进程分别替换加载器" >&2
  exit 1
fi

PRIVATE_DEX="$("${ADB[@]}" shell run-as "$PACKAGE" find . -type f -name '*.dex' 2>/dev/null | tr -d '\r')"
if [[ -n "$PRIVATE_DEX" ]]; then
  echo "应用私有目录发现明文 DEX：" >&2
  printf '%s\n' "$PRIVATE_DEX" >&2
  exit 1
fi

echo "API $SDK_INT 替换 ClassLoader 内存双 DEX 探针通过，应用私有目录未发现 DEX 文件"
