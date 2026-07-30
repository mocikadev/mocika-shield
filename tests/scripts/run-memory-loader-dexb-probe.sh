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
  "$GRADLE" -p "$PROJECT_DIR" :app:clean :app:assembleFactoryDebug \
  :app:assembleFactoryDebugAndroidTest :app:bundleFactoryDebug --no-daemon

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
  for marker in DEXB_NATIVE_DECRYPT_OK ORIGINAL_FACTORY_METADATA LOADER_READY:FACTORY APPLICATION_OK:SECONDARY_OK ACTIVITY_OK:SECONDARY_OK:NATIVE_OK SERVICE_OK DELAYED_OK:AFTER_GC; do
    grep -Fq "$marker" <<<"$logs" || { echo "同签名验证缺少标记：$marker" >&2; exit 1; }
  done
  local private_dex
  private_dex="$("${ADB[@]}" shell run-as "$PACKAGE" find . -type f -name '*.dex' 2>/dev/null | tr -d '\r')"
  if [[ -n "$private_dex" ]]; then
    echo "同签名探针在私有目录发现明文 DEX：$private_dex" >&2
    exit 1
  fi
}

verify_instrumentation() {
  local test_apk="$PROJECT_DIR/app/build/outputs/apk/androidTest/factory/debug/app-factory-debug-androidTest.apk"
  local signed_test_apk="$TEMP_DIR/memory-dexb-androidTest.apk"
  test -f "$test_apk" || { echo "缺少 Instrumentation 测试 APK" >&2; exit 1; }
  java -jar "$ROOT_DIR/tools/apksigner.jar" sign --ks "$GOOD_KEYSTORE" \
    --ks-pass "pass:$PASSWORD" --ks-key-alias probe --out "$signed_test_apk" "$test_apk"
  "${ADB[@]}" uninstall "$PACKAGE.test" >/dev/null 2>&1 || true
  "${ADB[@]}" install "$signed_test_apk" >/dev/null
  "${ADB[@]}" logcat -c
  local output logs
  output="$("${ADB[@]}" shell am instrument -w \
    "$PACKAGE.test/dev.mocika.shield.memoryprobe.ProbeInstrumentation")"
  logs="$("${ADB[@]}" logcat -d -s MOCIKA_MEMORY_PROBE:I AndroidRuntime:E '*:S')"
  grep -Fq 'INSTRUMENTATION_LOADER_OK' <<<"$logs" || {
    echo "Instrumentation 未能使用业务代理加载器" >&2
    printf '%s\n%s\n' "$output" "$logs" >&2
    exit 1
  }
  if grep -Fq 'INSTRUMENTATION_LOADER_FAILED' <<<"$logs"; then
    echo "Instrumentation 加载器验证失败" >&2
    printf '%s\n' "$logs" >&2
    exit 1
  fi
  echo "外部测试 APK 的 Instrumentation 加载器验证通过"
}

verify_split_installation() {
  local bundletool="${BUNDLETOOL_JAR:-}"
  if [[ -z "$bundletool" || ! -f "$bundletool" ]]; then
    echo "split 验证需要通过 BUNDLETOOL_JAR 指定 bundletool.jar" >&2
    exit 1
  fi
  local bundle="$PROJECT_DIR/app/build/outputs/bundle/factoryDebug/app-factory-debug.aab"
  local apks="$TEMP_DIR/memory-dexb-split.apks"
  java -jar "$bundletool" build-apks --bundle="$bundle" --output="$apks" \
    --ks="$GOOD_KEYSTORE" --ks-pass="pass:$PASSWORD" \
    --ks-key-alias=probe --key-pass="pass:$PASSWORD" >/dev/null
  unzip -q "$apks" -d "$TEMP_DIR/split-apks"
  local split_code_apk=""
  local candidate dex_entry
  while IFS= read -r candidate; do
    while IFS= read -r dex_entry; do
      if unzip -p "$candidate" "$dex_entry" | grep -aFq 'SplitMarker'; then
        split_code_apk="$candidate"
        break 2
      fi
    done < <(unzip -Z1 "$candidate" | awk '$0 ~ /^classes[0-9]*\.dex$/')
  done < <(find "$TEMP_DIR/split-apks" -type f -name '*.apk' | sort)
  if [[ -z "$split_code_apk" ]]; then
    echo "生成的 split 集中没有代码 split 标记" >&2
    exit 1
  fi
  "${ADB[@]}" uninstall "$PACKAGE" >/dev/null 2>&1 || true
  local install_args=(install-apks "--apks=$apks")
  if [[ -n "${ANDROID_SERIAL:-}" ]]; then
    install_args+=("--device-id=$ANDROID_SERIAL")
  fi
  java -jar "$bundletool" "${install_args[@]}" >/dev/null
  "${ADB[@]}" logcat -c
  "${ADB[@]}" shell am start -W -n "$COMPONENT" >/dev/null
  sleep 2
  local logs
  logs="$("${ADB[@]}" logcat -d -s MOCIKA_MEMORY_PROBE:I AndroidRuntime:E '*:S')"
  grep -Fq 'SPLIT_LOADER_OK:SPLIT_CODE_OK' <<<"$logs" || {
    echo "split 安装后业务代理加载器未能访问动态特性代码" >&2
    printf '%s\n' "$logs" >&2
    exit 1
  }
  echo "真实 split 安装与代理加载验证通过；代码 split 仍为明文生产阻塞项"
}

build_factory_variant() {
  local original_factory="$1"
  local output_apk="$2"
  MEMORY_PROBE_ORIGINAL_FACTORY="$original_factory" \
  MEMORY_PROBE_ASSETS="$TEMP_DIR/final-assets" \
  MEMORY_PROBE_NATIVE_LIBS="$ROOT_DIR/shield-stub/build/jniLibs" \
    "$GRADLE" -p "$PROJECT_DIR" :app:clean :app:assembleFactoryDebug --no-daemon >/dev/null
  java -jar "$ROOT_DIR/tools/apksigner.jar" sign --ks "$GOOD_KEYSTORE" \
    --ks-pass "pass:$PASSWORD" --ks-key-alias probe --out "$output_apk" \
    "$PROJECT_DIR/app/build/outputs/apk/factory/debug/app-factory-debug.apk"
}

verify_no_original_factory() {
  local apk="$TEMP_DIR/memory-dexb-no-factory.apk"
  build_factory_variant "" "$apk"
  "${ADB[@]}" uninstall "$PACKAGE" >/dev/null 2>&1 || true
  "${ADB[@]}" logcat -c
  "${ADB[@]}" install "$apk" >/dev/null
  "${ADB[@]}" shell am start -W -n "$COMPONENT" >/dev/null
  sleep 2
  local logs
  logs="$("${ADB[@]}" logcat -d -s MOCIKA_MEMORY_PROBE:I AndroidRuntime:E '*:S')"
  for marker in ORIGINAL_FACTORY_DEFAULT APPLICATION_OK:SECONDARY_OK ACTIVITY_OK:SECONDARY_OK:NATIVE_OK SERVICE_OK; do
    grep -Fq "$marker" <<<"$logs" || { echo "无原工厂验证缺少标记：$marker" >&2; exit 1; }
  done
  echo "未声明原组件工厂时默认委托验证通过"
}

verify_recursive_factory_rejected() {
  local apk="$TEMP_DIR/memory-dexb-recursive-factory.apk"
  build_factory_variant "dev.mocika.shield.memoryprobe.ProbeAppComponentFactory" "$apk"
  "${ADB[@]}" uninstall "$PACKAGE" >/dev/null 2>&1 || true
  "${ADB[@]}" logcat -c
  "${ADB[@]}" install "$apk" >/dev/null
  "${ADB[@]}" shell am start -W -n "$COMPONENT" >/dev/null 2>&1 || true
  sleep 2
  local logs
  logs="$("${ADB[@]}" logcat -d -s MOCIKA_MEMORY_PROBE:I AndroidRuntime:E '*:S')"
  grep -Fq 'MEMORY_PROBE_FACTORY_RECURSION' <<<"$logs" || {
    echo "递归原组件工厂未被拒绝" >&2
    printf '%s\n' "$logs" >&2
    exit 1
  }
  if grep -Fq 'APPLICATION_OK' <<<"$logs"; then
    echo "递归原组件工厂错误地进入业务生命周期" >&2
    exit 1
  fi
  echo "递归原组件工厂已在业务类定义前拒绝"
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
verify_instrumentation
verify_split_installation
verify_wrong_signature
verify_no_original_factory
verify_recursive_factory_rejected
"${ADB[@]}" uninstall "$PACKAGE" >/dev/null 2>&1 || true
echo "API ${SDK_INT} 正式 Stub Native、DEXB v5 与延迟内存加载代理验证通过"
