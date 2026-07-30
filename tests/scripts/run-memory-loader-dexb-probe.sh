#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROJECT_DIR="$ROOT_DIR/tests/fixtures/android-memory-loader-probe"
GRADLE="$ROOT_DIR/shield-stub/gradlew"
PACKAGE="dev.mocika.shield.memoryprobe"
COMPONENT="$PACKAGE/dev.mocika.shield.memorypayload.PayloadActivity"
TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/mocika-memory-dexb.XXXXXX")"
PASSWORD="mocika-probe-123"

cleanup() {
  rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

for command in adb cargo java keytool unzip zip; do
  command -v "$command" >/dev/null || { echo "缺少命令：$command" >&2; exit 1; }
done

ADB=(adb)
if [[ -n "${ANDROID_SERIAL:-}" ]]; then
  ADB+=("-s" "$ANDROID_SERIAL")
fi
"${ADB[@]}" get-state >/dev/null

SDK_INT="$("${ADB[@]}" shell getprop ro.build.version.sdk | tr -d '\r')"
if [[ ! "$SDK_INT" =~ ^[0-9]+$ ]] || (( SDK_INT < 29 )); then
  echo "DEXB 内存加载探针只支持 API 29 及以上设备，当前 API：${SDK_INT:-未知}" >&2
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

test -f "$ROOT_DIR/tools/apksigner.jar" || { echo "缺少 tools/apksigner.jar" >&2; exit 1; }
test -f "$ROOT_DIR/shield-stub/build/outputs/resources/resources.zip" || {
  echo "缺少正式 Stub 资源，请先执行 make build-stub" >&2
  exit 1
}
test -f "$ROOT_DIR/shield-stub/build/jniLibs/arm64-v8a/libmocikashield.so" || {
  echo "缺少正式 Stub Native 库，请先执行 make build-stub" >&2
  exit 1
}

mkdir -p "$TEMP_DIR/final-assets" \
  "$TEMP_DIR/main-aar" "$TEMP_DIR/secondary-aar" \
  "$TEMP_DIR/main-dex" "$TEMP_DIR/secondary-dex"

"$GRADLE" -p "$PROJECT_DIR" :payload-main:assembleDebug :payload-secondary:assembleDebug --no-daemon
unzip -q "$PROJECT_DIR/payload-main/build/outputs/aar/payload-main-debug.aar" classes.jar -d "$TEMP_DIR/main-aar"
unzip -q "$PROJECT_DIR/payload-secondary/build/outputs/aar/payload-secondary-debug.aar" classes.jar -d "$TEMP_DIR/secondary-aar"
"$D8" --min-api 29 --output "$TEMP_DIR/main-dex" "$TEMP_DIR/main-aar/classes.jar"
"$D8" --min-api 29 --output "$TEMP_DIR/secondary-dex" "$TEMP_DIR/secondary-aar/classes.jar"
"$GRADLE" -p "$ROOT_DIR/tests/fixtures/android-smoke-app" :app:assembleRelease --no-daemon

TEMPLATE_APK="$ROOT_DIR/tests/fixtures/android-smoke-app/app/build/outputs/apk/release/app-release-unsigned.apk"
PAYLOAD_UNSIGNED="$TEMP_DIR/payload-unsigned.apk"
PAYLOAD_SIGNED="$TEMP_DIR/payload-signed.apk"
PROTECTED_UNSIGNED="$TEMP_DIR/payload-protected.apk"
GOOD_KEYSTORE="$TEMP_DIR/good.p12"
BAD_KEYSTORE="$TEMP_DIR/bad.p12"

cp "$TEMPLATE_APK" "$PAYLOAD_UNSIGNED"
zip -qd "$PAYLOAD_UNSIGNED" 'classes*.dex'
cp "$TEMP_DIR/main-dex/classes.dex" "$TEMP_DIR/classes.dex"
cp "$TEMP_DIR/secondary-dex/classes.dex" "$TEMP_DIR/classes2.dex"
(cd "$TEMP_DIR" && zip -q "$PAYLOAD_UNSIGNED" classes.dex classes2.dex)

keytool -genkeypair -noprompt -storetype PKCS12 -keystore "$GOOD_KEYSTORE" \
  -storepass "$PASSWORD" -keypass "$PASSWORD" -alias probe -keyalg RSA -keysize 2048 \
  -validity 3650 -dname "CN=Mocika DEXB Probe, O=MocikaDev, C=CN"
keytool -genkeypair -noprompt -storetype PKCS12 -keystore "$BAD_KEYSTORE" \
  -storepass "$PASSWORD" -keypass "$PASSWORD" -alias probe -keyalg RSA -keysize 2048 \
  -validity 3650 -dname "CN=Mocika Wrong Probe, O=MocikaDev, C=CN"

java -jar "$ROOT_DIR/tools/apksigner.jar" sign --ks "$GOOD_KEYSTORE" \
  --ks-pass "pass:$PASSWORD" --ks-key-alias probe --out "$PAYLOAD_SIGNED" "$PAYLOAD_UNSIGNED"

cargo build --release -p shield-cli --manifest-path "$ROOT_DIR/Cargo.toml"
"$ROOT_DIR/target/release/shield" protect --input "$PAYLOAD_SIGNED" --output "$PROTECTED_UNSIGNED"
unzip -p "$PROTECTED_UNSIGNED" classes.dex > "$TEMP_DIR/final-assets/protected-payload.dex"
grep -a -q 'MSHD' "$TEMP_DIR/final-assets/protected-payload.dex"

MEMORY_PROBE_ASSETS="$TEMP_DIR/final-assets" \
MEMORY_PROBE_NATIVE_LIBS="$ROOT_DIR/shield-stub/build/jniLibs" \
  "$GRADLE" -p "$PROJECT_DIR" :app:clean :app:assembleFactoryDebug --no-daemon

FINAL_UNSIGNED="$PROJECT_DIR/app/build/outputs/apk/factory/debug/app-factory-debug.apk"
FINAL_GOOD="$TEMP_DIR/memory-dexb-good.apk"
FINAL_BAD="$TEMP_DIR/memory-dexb-bad.apk"
java -jar "$ROOT_DIR/tools/apksigner.jar" sign --ks "$GOOD_KEYSTORE" \
  --ks-pass "pass:$PASSWORD" --ks-key-alias probe --out "$FINAL_GOOD" "$FINAL_UNSIGNED"
java -jar "$ROOT_DIR/tools/apksigner.jar" sign --ks "$BAD_KEYSTORE" \
  --ks-pass "pass:$PASSWORD" --ks-key-alias probe --out "$FINAL_BAD" "$FINAL_UNSIGNED"

verify_good_signature() {
  "${ADB[@]}" uninstall "$PACKAGE" >/dev/null 2>&1 || true
  "${ADB[@]}" logcat -c
  "${ADB[@]}" install "$FINAL_GOOD" >/dev/null
  "${ADB[@]}" shell am start -W -n "$COMPONENT" >/dev/null
  sleep 2
  local logs
  logs="$("${ADB[@]}" logcat -d -s MOCIKA_MEMORY_PROBE:I AndroidRuntime:E '*:S')"
  printf '%s\n' "$logs"
  for marker in DEXB_NATIVE_DECRYPT_OK LOADER_READY:FACTORY APPLICATION_OK:SECONDARY_OK ACTIVITY_OK:SECONDARY_OK:NATIVE_OK SERVICE_OK DELAYED_OK:AFTER_GC; do
    grep -Fq "$marker" <<<"$logs" || { echo "同签名验证缺少标记：$marker" >&2; exit 1; }
  done
  local private_dex
  private_dex="$("${ADB[@]}" shell run-as "$PACKAGE" find . -type f -name '*.dex' 2>/dev/null | tr -d '\r')"
  if [[ -n "$private_dex" ]]; then
    echo "同签名探针在私有目录发现明文 DEX：$private_dex" >&2
    exit 1
  fi
}

verify_wrong_signature() {
  "${ADB[@]}" uninstall "$PACKAGE" >/dev/null 2>&1 || true
  "${ADB[@]}" logcat -c
  "${ADB[@]}" install "$FINAL_BAD" >/dev/null
  "${ADB[@]}" shell am start -W -n "$COMPONENT" >/dev/null 2>&1 || true
  sleep 2
  local logs
  logs="$("${ADB[@]}" logcat -d -s MOCIKA_MEMORY_PROBE:I AndroidRuntime:E '*:S')"
  if grep -Fq 'DEXB_NATIVE_DECRYPT_OK' <<<"$logs" || grep -Fq 'APPLICATION_OK' <<<"$logs"; then
    echo "异签名探针错误地完成了解密或业务启动" >&2
    exit 1
  fi
  grep -Fq '解密解压失败' <<<"$logs" || {
    echo "异签名探针未观察到正式 Native 解密拒绝" >&2
    printf '%s\n' "$logs" >&2
    exit 1
  }
  echo "异签名 DEXB v5 已在业务类定义前拒绝"
}

verify_good_signature
verify_wrong_signature
"${ADB[@]}" uninstall "$PACKAGE" >/dev/null 2>&1 || true
echo "API ${SDK_INT} 正式 Stub Native、DEXB v5 与延迟内存加载代理验证通过"
