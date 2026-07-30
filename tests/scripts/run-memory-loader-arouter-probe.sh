#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROJECT_DIR="$ROOT_DIR/tests/fixtures/android-memory-loader-probe"
GRADLE="$ROOT_DIR/shield-stub/gradlew"
INPUT_APK="${1:-${AROUTER_APK:-}}"
SIGNER="${2:-${APK_SIGNER_SCRIPT:-}}"
PACKAGE="com.example.arouterdemo"
LAUNCH_COMPONENT="$PACKAGE/.LauncherActivity"
HOME_ACTIVITY="$PACKAGE.home.HomeActivity"
TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/mocika-arouter-memory.XXXXXX")"

cleanup() {
  rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

if [[ -z "$INPUT_APK" || -z "$SIGNER" ]]; then
  echo "用法：$0 <同签名的原始 ARouter APK> <APK 签名脚本>" >&2
  exit 1
fi
test -f "$INPUT_APK" || { echo "ARouter APK 不存在：$INPUT_APK" >&2; exit 1; }
test -x "$SIGNER" || { echo "签名脚本不可执行：$SIGNER" >&2; exit 1; }
test -f "$ROOT_DIR/shield-stub/build/outputs/resources/resources.zip" || {
  echo "缺少正式 Stub 资源，请先执行 make build-stub" >&2
  exit 1
}

ADB=(adb)
if [[ -n "${ANDROID_SERIAL:-}" ]]; then
  ADB+=("-s" "$ANDROID_SERIAL")
fi
"${ADB[@]}" get-state >/dev/null

ORIGINAL_DIR="$TEMP_DIR/original"
PROTECTED_DIR="$TEMP_DIR/protected"
SHARED_JAVA="$TEMP_DIR/shared-java/dev/mocika/shield/loader"
ASSET_DIR="$TEMP_DIR/assets"
PROTECTED_UNSIGNED="$TEMP_DIR/protected-unsigned.apk"
PROTECTED_SIGNED="$TEMP_DIR/protected-signed.apk"
PROTOTYPE_UNSIGNED="$TEMP_DIR/arouter-memory-unsigned.apk"
PROTOTYPE_SIGNED="$TEMP_DIR/arouter-memory-signed.apk"
PERFORMANCE_UNSIGNED="$TEMP_DIR/arouter-memory-performance-unsigned.apk"
PERFORMANCE_SIGNED="$TEMP_DIR/arouter-memory-performance-signed.apk"
DIAGNOSTIC_UNSIGNED="$TEMP_DIR/arouter-memory-diagnostic-unsigned.apk"
DIAGNOSTIC_SIGNED="$TEMP_DIR/arouter-memory-diagnostic-signed.apk"
NORMALIZED_INPUT="$TEMP_DIR/arouter-input-signed.apk"
mkdir -p "$ORIGINAL_DIR" "$PROTECTED_DIR" "$SHARED_JAVA" "$ASSET_DIR"

"$SIGNER" "$INPUT_APK" "$NORMALIZED_INPUT" >/dev/null
java -jar "$ROOT_DIR/tools/apktool_3.0.1.jar" d "$NORMALIZED_INPUT" \
  -o "$ORIGINAL_DIR" -f --no-src >/dev/null
ORIGINAL_APPLICATION="$(sed -n 's/.*<application[^>]*android:name="\([^"]*\)".*/\1/p' \
  "$ORIGINAL_DIR/AndroidManifest.xml" | head -1)"
ORIGINAL_FACTORY="$(sed -n 's/.*<application[^>]*android:appComponentFactory="\([^"]*\)".*/\1/p' \
  "$ORIGINAL_DIR/AndroidManifest.xml" | head -1)"
if [[ -z "$ORIGINAL_APPLICATION" || -z "$ORIGINAL_FACTORY" ]]; then
  echo "样本必须声明原 Application 和 appComponentFactory" >&2
  exit 1
fi

cargo build --release -p shield-cli --manifest-path "$ROOT_DIR/Cargo.toml"
"$ROOT_DIR/target/release/shield" protect --input "$NORMALIZED_INPUT" --output "$PROTECTED_UNSIGNED"
"$SIGNER" "$PROTECTED_UNSIGNED" "$PROTECTED_SIGNED" >/dev/null
java -jar "$ROOT_DIR/tools/apktool_3.0.1.jar" d "$PROTECTED_UNSIGNED" \
  -o "$PROTECTED_DIR" -f --no-src >/dev/null

cp "$PROTECTED_DIR/classes.dex" "$ASSET_DIR/protected-payload.dex"
cp "$PROTECTED_DIR/classes.dex" "$PROTECTED_DIR/assets/protected-payload.dex"
cp "$ROOT_DIR/shield-stub/src/main/java/dev/mocika/shield/loader/ARouterCompat.java" \
  "$SHARED_JAVA/ARouterCompat.java"

MEMORY_PROBE_ASSETS="$ASSET_DIR" \
MEMORY_PROBE_NATIVE_LIBS="$ROOT_DIR/shield-stub/build/jniLibs" \
MEMORY_PROBE_SHARED_JAVA="$TEMP_DIR/shared-java" \
MEMORY_PROBE_ORIGINAL_FACTORY="$ORIGINAL_FACTORY" \
  "$GRADLE" -p "$PROJECT_DIR" :app:clean :app:assembleFactoryDebug --no-daemon >/dev/null

SHELL_APK="$PROJECT_DIR/app/build/outputs/apk/factory/debug/app-factory-debug.apk"
while IFS= read -r shell_dex; do
  unzip -p "$SHELL_APK" "$shell_dex" > "$PROTECTED_DIR/$shell_dex"
done < <(unzip -Z1 "$SHELL_APK" | awk '$0 ~ /^classes[0-9]*\.dex$/')
for library in "$ROOT_DIR"/shield-stub/build/jniLibs/*/libmocikashield.so; do
  abi="$(basename "$(dirname "$library")")"
  mkdir -p "$PROTECTED_DIR/lib/$abi"
  cp "$library" "$PROTECTED_DIR/lib/$abi/libmocikashield.so"
done

MANIFEST="$PROTECTED_DIR/AndroidManifest.xml"
ORIGINAL_APPLICATION="$ORIGINAL_APPLICATION" ORIGINAL_FACTORY="$ORIGINAL_FACTORY" \
perl -0pi -e '
  s{(<application\b[^>]*?)\sandroid:name="[^"]*"}{$1 android:name="dev.mocika.shield.memoryprobe.ProbeApplication"};
  s{<application\b}{<application android:appComponentFactory="dev.mocika.shield.memoryprobe.ProbeAppComponentFactory"};
  s{</application>}{
    <meta-data android:name="REAL_APPLICATION" android:value="$ENV{ORIGINAL_APPLICATION}"/>
    <meta-data android:name="ORIGINAL_COMPONENT_FACTORY" android:value="$ENV{ORIGINAL_FACTORY}"/>
    <meta-data android:name="PROBE_CONCURRENT_CLASSES" android:value="com.example.arouterdemo.LauncherActivity,com.example.arouterdemo.home.HomeActivity"/>
    <meta-data android:name="PROBE_AROUTER_ROUTE" android:value="/home/main"/>
  </application>};
' "$MANIFEST"

java -jar "$ROOT_DIR/tools/apktool_3.0.1.jar" b "$PROTECTED_DIR" \
  -o "$PROTOTYPE_UNSIGNED" >/dev/null
"$SIGNER" "$PROTOTYPE_UNSIGNED" "$PROTOTYPE_SIGNED" >/dev/null
java -jar "$ROOT_DIR/tools/apksigner.jar" verify --verbose "$PROTOTYPE_SIGNED" >/dev/null

perl -0pi -e '
  s{android:name="PROBE_CONCURRENT_CLASSES" android:value="[^"]*"}{android:name="PROBE_CONCURRENT_CLASSES" android:value="com.example.arouterdemo.LauncherActivity,com.example.arouterdemo.DemoApplication"};
  s{android:name="PROBE_AROUTER_ROUTE" android:value="[^"]*"}{android:name="PROBE_AROUTER_ROUTE" android:value=""};
' "$MANIFEST"
java -jar "$ROOT_DIR/tools/apktool_3.0.1.jar" b "$PROTECTED_DIR" \
  -o "$PERFORMANCE_UNSIGNED" >/dev/null
"$SIGNER" "$PERFORMANCE_UNSIGNED" "$PERFORMANCE_SIGNED" >/dev/null
java -jar "$ROOT_DIR/tools/apksigner.jar" verify --verbose "$PERFORMANCE_SIGNED" >/dev/null

perl -0pi -e '
  s{</application>}{<meta-data android:name="PROBE_MEMORY_METRICS" android:value="true"/></application>};
' "$MANIFEST"
java -jar "$ROOT_DIR/tools/apktool_3.0.1.jar" b "$PROTECTED_DIR" \
  -o "$DIAGNOSTIC_UNSIGNED" >/dev/null
"$SIGNER" "$DIAGNOSTIC_UNSIGNED" "$DIAGNOSTIC_SIGNED" >/dev/null
java -jar "$ROOT_DIR/tools/apksigner.jar" verify --verbose "$DIAGNOSTIC_SIGNED" >/dev/null

verify_route() {
  local scenario="$1"
  "${ADB[@]}" logcat -c
  "${ADB[@]}" shell am force-stop "$PACKAGE" >/dev/null 2>&1 || true
  "${ADB[@]}" shell am start -W -n "$LAUNCH_COMPONENT" >/dev/null
  sleep 3
  local logs activities
  logs="$("${ADB[@]}" logcat -d -s MOCIKA_MEMORY_PROBE:I AndroidRuntime:E '*:S')"
  activities="$("${ADB[@]}" shell dumpsys activity activities)"
  for marker in DEXB_NATIVE_DECRYPT_OK "ORIGINAL_FACTORY_METADATA:$ORIGINAL_FACTORY" AROUTER_PREPARE_OK AROUTER_NAVIGATION_INVOKED:/home/main; do
    grep -Fq "$marker" <<<"$logs" || {
      echo "$scenario 缺少标记：$marker" >&2
      printf '%s\n' "$logs" >&2
      exit 1
    }
  done
  grep -Fq "$HOME_ACTIVITY" <<<"$activities" || {
    echo "$scenario 未进入 ARouter 首页：$HOME_ACTIVITY" >&2
    exit 1
  }
  echo "$scenario 验证通过"
}

verify_file_start() {
  local scenario="$1"
  "${ADB[@]}" shell am force-stop "$PACKAGE" >/dev/null 2>&1 || true
  "${ADB[@]}" shell am start -W -n "$LAUNCH_COMPONENT" >/dev/null
  sleep 3
  local activities
  activities="$("${ADB[@]}" shell dumpsys activity activities)"
  grep -Fq "$LAUNCH_COMPONENT" <<<"$activities" || {
    echo "$scenario 未进入原始启动页：$LAUNCH_COMPONENT" >&2
    exit 1
  }
  echo "$scenario 验证通过"
}

median() {
  printf '%s\n' "$@" | sort -n | awk '{ values[NR] = $1 } END { print values[int((NR + 1) / 2)] }'
}

measure_variant() {
  local name="$1"
  local apk="$2"
  local -a times=()
  local -a immediate_pss_values=()
  local -a settled_pss_values=()
  "${ADB[@]}" uninstall "$PACKAGE" >/dev/null 2>&1 || true
  "${ADB[@]}" install "$apk" >/dev/null
  for _ in 1 2 3 4 5; do
    "${ADB[@]}" shell am force-stop "$PACKAGE" >/dev/null 2>&1 || true
    local launch total_time immediate_pss settled_pss
    launch="$("${ADB[@]}" shell am start -W -n "$LAUNCH_COMPONENT")"
    total_time="$(awk -F': ' '/^TotalTime:/{print $2}' <<<"$launch" | tr -d '\r')"
    immediate_pss="$("${ADB[@]}" shell dumpsys meminfo "$PACKAGE" | \
      awk '/TOTAL PSS:/{print $3; exit} /^TOTAL[[:space:]]/{print $2; exit}' | tr -d '\r')"
    "${ADB[@]}" shell am send-trim-memory "$PACKAGE" RUNNING_LOW >/dev/null
    sleep 1
    settled_pss="$("${ADB[@]}" shell dumpsys meminfo "$PACKAGE" | \
      awk '/TOTAL PSS:/{print $3; exit} /^TOTAL[[:space:]]/{print $2; exit}' | tr -d '\r')"
    [[ "$total_time" =~ ^[0-9]+$ && "$immediate_pss" =~ ^[0-9]+$ \
        && "$settled_pss" =~ ^[0-9]+$ ]] || {
      echo "$name 性能采样解析失败" >&2
      exit 1
    }
    times+=("$total_time")
    immediate_pss_values+=("$immediate_pss")
    settled_pss_values+=("$settled_pss")
  done
  echo "${name}：冷启动中位数 $(median "${times[@]}") 毫秒，即时 TOTAL PSS 中位数 $(median "${immediate_pss_values[@]}") KB，统一内存整理后中位数 $(median "${settled_pss_values[@]}") KB"
}

inspect_memory_lifecycle() {
  "${ADB[@]}" uninstall "$PACKAGE" >/dev/null 2>&1 || true
  "${ADB[@]}" install "$DIAGNOSTIC_SIGNED" >/dev/null
  "${ADB[@]}" logcat -c
  "${ADB[@]}" shell am start -W -n "$LAUNCH_COMPONENT" >/dev/null
  sleep 3
  local snapshots
  snapshots="$("${ADB[@]}" logcat -d -s MOCIKA_MEMORY_METRICS:I '*:S')"
  grep -Fq 'SNAPSHOT:post_startup_gc' <<<"$snapshots" || {
    echo "内存对象生命周期探针未输出 GC 后快照" >&2
    printf '%s\n' "$snapshots" >&2
    exit 1
  }
  printf '%s\n' "$snapshots"
}

"${ADB[@]}" uninstall "$PACKAGE" >/dev/null 2>&1 || true
"${ADB[@]}" install "$NORMALIZED_INPUT" >/dev/null
"${ADB[@]}" shell am start -W -n "$LAUNCH_COMPONENT" >/dev/null
"${ADB[@]}" install -r "$PROTOTYPE_SIGNED" >/dev/null
verify_route "同签名覆盖升级"

"${ADB[@]}" install -r "$PROTECTED_SIGNED" >/dev/null
verify_file_start "内存路径回退到认证文件路径"

"${ADB[@]}" install -r "$PROTOTYPE_SIGNED" >/dev/null
verify_route "认证文件路径再次迁移到内存路径"

"${ADB[@]}" shell pm clear "$PACKAGE" >/dev/null
verify_route "清除数据后首次启动"

"${ADB[@]}" uninstall "$PACKAGE" >/dev/null 2>&1 || true
"${ADB[@]}" install "$PROTOTYPE_SIGNED" >/dev/null
verify_route "全新安装首次启动"

inspect_memory_lifecycle
measure_variant "未加固基线" "$NORMALIZED_INPUT"
measure_variant "认证文件路径" "$PROTECTED_SIGNED"
measure_variant "内存路径原型" "$PERFORMANCE_SIGNED"

"${ADB[@]}" uninstall "$PACKAGE" >/dev/null 2>&1 || true
echo "正式 DEXB 内存加载、双向路径迁移、AndroidX 组件工厂、ARouter 与性能基线验证通过"
