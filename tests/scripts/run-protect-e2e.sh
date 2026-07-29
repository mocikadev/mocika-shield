#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FIXTURE="$ROOT/tests/fixtures/android-smoke-app"
PACKAGE_NAME="dev.mocika.shield.smoke"
COMPONENT="$PACKAGE_NAME/.MainActivity"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/mocika-protect-e2e.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

for command in java keytool cargo unzip; do
  command -v "$command" >/dev/null || { echo "缺少命令: $command" >&2; exit 1; }
done

test -f "$ROOT/shield-stub/build/outputs/resources/resources.zip" || {
  echo "缺少 resources.zip，请先执行 make build-stub" >&2
  exit 1
}

"$ROOT/shield-stub/gradlew" -p "$FIXTURE" :app:assembleRelease --no-daemon
cargo build --release -p shield-cli --manifest-path "$ROOT/Cargo.toml"

BASE_UNSIGNED="$FIXTURE/app/build/outputs/apk/release/app-release-unsigned.apk"
UNSIGNED="$WORK/input-multidex-unsigned.apk"
SIGNED="$WORK/input-signed.apk"
PROTECTED="$WORK/output-protected.apk"
FINAL="$WORK/output-protected-signed.apk"
KEYSTORE="$WORK/smoke.p12"
PASSWORD="mocika-test-123"

"$ROOT/tests/scripts/build-smoke-multidex.sh" "$BASE_UNSIGNED" "$UNSIGNED" 19

keytool -genkeypair -noprompt -storetype PKCS12 \
  -keystore "$KEYSTORE" -storepass "$PASSWORD" -keypass "$PASSWORD" \
  -alias smoke -keyalg RSA -keysize 2048 -validity 3650 \
  -dname "CN=Mocika Shield Smoke, O=MocikaDev, C=CN"

java -jar "$ROOT/tools/apksigner.jar" sign \
  --ks "$KEYSTORE" --ks-pass "pass:$PASSWORD" --ks-key-alias smoke \
  --out "$SIGNED" "$UNSIGNED"

PROTECT_COMMAND=(
  "$ROOT/target/release/shield" protect
  --input "$SIGNED"
  --output "$PROTECTED"
)
if [[ "${ENVIRONMENT_POLICY:-compatible}" == "strict" ]]; then
  PROTECT_COMMAND+=(--environment-policy strict)
fi
"${PROTECT_COMMAND[@]}"

java -jar "$ROOT/tools/apksigner.jar" sign \
  --ks "$KEYSTORE" --ks-pass "pass:$PASSWORD" --ks-key-alias smoke \
  --out "$FINAL" "$PROTECTED"
java -jar "$ROOT/tools/apksigner.jar" verify --verbose "$FINAL"

CHECK_RESULT="$("$ROOT/target/release/shield" check-apk "$FINAL")"
printf '%s\n' "$CHECK_RESULT" | grep -q '"already_protected":true'
printf '%s\n' "$CHECK_RESULT" | grep -q '"is_signed":true'

unzip -p "$FINAL" classes.dex > "$WORK/classes.dex"
grep -a -q 'MSHD' "$WORK/classes.dex"
ARM64_STUB_LIBRARIES="$(unzip -Z1 "$FINAL" | awk '$0 ~ /^lib\/arm64-v8a\/libmodule[a-z]+\.so$/')"
if [[ "$(printf '%s\n' "$ARM64_STUB_LIBRARIES" | awk 'NF { count++ } END { print count + 0 }')" != "1" ]]; then
  echo "arm64-v8a 目录未包含唯一的任务级 Native 别名库：" >&2
  printf '%s\n' "$ARM64_STUB_LIBRARIES" >&2
  exit 1
fi
if unzip -Z1 "$FINAL" | grep -q '/libmocikashield\.so$'; then
  echo "最终 APK 仍包含固定名称 libmocikashield.so" >&2
  exit 1
fi

DECODED="$WORK/output-decoded"
java -jar "$ROOT/tools/apktool_3.0.1.jar" d "$FINAL" -o "$DECODED" -f --no-src >/dev/null
grep -q 'android:extractNativeLibs="false"' "$DECODED/AndroidManifest.xml"

COMPRESSED_SO="$(unzip -lv "$FINAL" | awk '$NF ~ /^lib\/.+\.so$/ && $2 != "Stored" { print $NF }')"
if [ -n "$COMPRESSED_SO" ]; then
  echo "extractNativeLibs=false 时存在压缩 Native 库：" >&2
  printf '%s\n' "$COMPRESSED_SO" >&2
  exit 1
fi

verify_launch() {
  local scenario="$1"
  "${ADB_COMMAND[@]}" logcat -c
  "${ADB_COMMAND[@]}" shell am force-stop "$PACKAGE_NAME" >/dev/null 2>&1 || true
  "${ADB_COMMAND[@]}" shell am start -W -n "$COMPONENT" >/dev/null
  for _ in $(seq 1 30); do
    local logs
    logs="$("${ADB_COMMAND[@]}" logcat -d -s MocikaSmoke:I AndroidRuntime:E '*:S')"
    if grep -q 'MOCIKA_SMOKE_APPLICATION_OK' <<< "$logs" \
      && grep -q 'MOCIKA_SMOKE_ACTIVITY_OK' <<< "$logs" \
      && grep -q 'MOCIKA_SMOKE_SECONDARY_OK' <<< "$logs"; then
      echo "$scenario 验证通过"
      return 0
    fi
    sleep 1
  done
  echo "错误：$scenario 未观察到完整启动标记" >&2
  "${ADB_COMMAND[@]}" logcat -d -s MocikaSmoke:V AndroidRuntime:E '*:S' >&2
  return 1
}

if [[ "${RUN_DEVICE_TEST:-0}" == "1" ]]; then
  command -v adb >/dev/null || { echo "缺少命令：adb" >&2; exit 1; }
  ADB_COMMAND=(adb)
  if [[ -n "${ANDROID_SERIAL:-}" ]]; then
    ADB_COMMAND+=(-s "$ANDROID_SERIAL")
  fi

  "${ADB_COMMAND[@]}" get-state >/dev/null
  "${ADB_COMMAND[@]}" uninstall "$PACKAGE_NAME" >/dev/null 2>&1 || true
  "${ADB_COMMAND[@]}" install "$SIGNED" | grep -q '^Success'
  verify_launch "未加固双 DEX 基线"
  "${ADB_COMMAND[@]}" install -r "$FINAL" | grep -q '^Success'
  verify_launch "同签名覆盖安装加固包首次启动"
  verify_launch "加固包缓存命中后二次启动"
fi

echo "端到端加固回归测试通过"
