#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FIXTURE="$ROOT/tests/fixtures/android-smoke-app"
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

UNSIGNED="$FIXTURE/app/build/outputs/apk/release/app-release-unsigned.apk"
SIGNED="$WORK/input-signed.apk"
PROTECTED="$WORK/output-protected.apk"
FINAL="$WORK/output-protected-signed.apk"
KEYSTORE="$WORK/smoke.p12"
PASSWORD="mocika-test-123"

keytool -genkeypair -noprompt -storetype PKCS12 \
  -keystore "$KEYSTORE" -storepass "$PASSWORD" -keypass "$PASSWORD" \
  -alias smoke -keyalg RSA -keysize 2048 -validity 3650 \
  -dname "CN=Mocika Shield Smoke, O=MocikaDev, C=CN"

java -jar "$ROOT/tools/apksigner.jar" sign \
  --ks "$KEYSTORE" --ks-pass "pass:$PASSWORD" --ks-key-alias smoke \
  --out "$SIGNED" "$UNSIGNED"

"$ROOT/target/release/shield" protect --input "$SIGNED" --output "$PROTECTED"

java -jar "$ROOT/tools/apksigner.jar" sign \
  --ks "$KEYSTORE" --ks-pass "pass:$PASSWORD" --ks-key-alias smoke \
  --out "$FINAL" "$PROTECTED"
java -jar "$ROOT/tools/apksigner.jar" verify --verbose "$FINAL"

CHECK_RESULT="$("$ROOT/target/release/shield" check-apk "$FINAL")"
printf '%s\n' "$CHECK_RESULT" | grep -q '"already_protected":true'
printf '%s\n' "$CHECK_RESULT" | grep -q '"is_signed":true'

unzip -p "$FINAL" classes.dex > "$WORK/classes.dex"
grep -a -q 'MSHD' "$WORK/classes.dex"
unzip -l "$FINAL" | grep -q 'lib/arm64-v8a/libmocikashield.so'

echo "端到端加固回归测试通过"
