#!/usr/bin/env bash

# 在显式指定的模拟器上验证旧壳升级、新字段、双 DEX 和缓存复用。
# 只操作项目自有测试应用；复用已审计资源，不下载镜像或重建 Native。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SERIAL="${ANDROID_SERIAL:?请通过 ANDROID_SERIAL 指定测试模拟器}"
PREVIOUS="${PREVIOUS_RESOURCES:?请指定旧版本资源目录}"
EXPECTED_API="${EXPECTED_API:?请指定本次验证的 API 级别}"
EXPECTED_ABI="${EXPECTED_ABI:?请指定本次验证的 CPU ABI}"
EXPECTED_PAGE_SIZE="${EXPECTED_PAGE_SIZE:-}"
PACKAGE=dev.mocika.shield.smoke
COMPONENT="$PACKAGE/.MainActivity"
ADB=(adb -s "$SERIAL")
CURRENT="$ROOT/shield-stub/build/outputs/resources"
FIXTURE="$ROOT/tests/fixtures/android-smoke-app/app/build/outputs/apk/release/app-release-unsigned.apk"
CLI="$ROOT/target/release/shield"
MAPPING="$ROOT/shield-stub/build/outputs/mapping/release/mapping.txt"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/mocika-stub-upgrade.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

case "$SERIAL" in emulator-*) ;; *) echo '本脚本只允许显式指定的模拟器，不自动操作连接的真机' >&2; exit 1 ;; esac
for command in adb java keytool python3 shasum; do
    command -v "$command" >/dev/null || { echo "缺少命令：$command" >&2; exit 1; }
done
for file in "$CLI" "$FIXTURE" "$MAPPING"; do
    test -f "$file" || { echo "缺少构建产物：$file" >&2; exit 1; }
done
"${ADB[@]}" get-state >/dev/null
API="$("${ADB[@]}" shell getprop ro.build.version.sdk | tr -d '\r')"
ABI="$("${ADB[@]}" shell getprop ro.product.cpu.abi | tr -d '\r')"
[[ "$API" == "$EXPECTED_API" && "$ABI" == "$EXPECTED_ABI" ]] || {
    echo "模拟器与预期不符：API=${API}，ABI=${ABI}" >&2; exit 1;
}
if [[ "$API" == 19 ]]; then
    "${ADB[@]}" shell cat /proc/cpuinfo | grep -Eq '(^|[[:space:]])neon([[:space:]]|$)' || {
        echo 'API 19 工控基线要求 NEON，当前模拟器 CPU 不符合要求' >&2; exit 1;
    }
fi
if [[ -n "$EXPECTED_PAGE_SIZE" ]]; then
    PAGE_SIZE="$("${ADB[@]}" shell getconf PAGE_SIZE | tr -d '\r')"
    [[ "$PAGE_SIZE" == "$EXPECTED_PAGE_SIZE" ]] || { echo "页大小不符：$PAGE_SIZE" >&2; exit 1; }
fi
[[ "$("${ADB[@]}" shell id | tr -d '\r')" == uid=0* ]] || {
    echo '缓存复用证据需要模拟器的 Root ADB；请单独配置该测试模拟器，脚本不自动开启 Root' >&2; exit 1;
}
echo "环境确认：API=${API}，ABI=${ABI}，页大小=${EXPECTED_PAGE_SIZE:-不作断言}"

cache_state() {
    # API 19 的 toolbox 未必提供 stat，使用其支持的 inode、长度、权限和时间列表。
    "${ADB[@]}" shell "ls -li /data/data/$PACKAGE/app_app_dex/v*/c*.dex" | tr -d '\r' | sort
}

assert_cache_pair() {
    local state="$1"
    [[ "$(printf '%s\n' "$state" | awk 'NF { n++ } END { print n+0 }')" == 2 ]] || {
        echo '未取得两份有效缓存文件的状态' >&2; return 1;
    }
    [[ "$state" == *c1.dex* && "$state" == *c2.dex* ]] || return 1
}

launch() {
    local label="$1" logs valid
    "${ADB[@]}" shell am force-stop "$PACKAGE" >/dev/null
    "${ADB[@]}" logcat -c
    "${ADB[@]}" shell am start -W -n "$COMPONENT" >/dev/null
    for attempt in $(seq 1 40); do
        logs="$("${ADB[@]}" logcat -d -s MocikaSmoke:I AndroidRuntime:E '*:S')"
        valid=1
        for marker in PROVIDER APPLICATION ACTIVITY SECONDARY RECEIVER REMOTE_SERVICE; do
            grep -q "MOCIKA_SMOKE_${marker}_OK" <<< "$logs" || valid=0
        done
        if [[ "$valid" == 1 ]]; then echo "通过：$label"; return 0; fi
        sleep 1
    done
    printf '%s\n' "$logs" >&2
    echo "未观察到完整启动标记：$label" >&2
    return 1
}

sign() {
    java -jar "$ROOT/tools/apksigner.jar" sign --ks "$WORK/smoke.p12" \
        --ks-pass pass:mocika-test-123 --ks-key-alias smoke --out "$2" "$1"
    java -jar "$ROOT/tools/apksigner.jar" verify "$2" >/dev/null
    if [[ "$EXPECTED_PAGE_SIZE" == 16384 ]]; then
        "${ANDROID_HOME:?}/build-tools/35.0.0/zipalign" -c -P 16 -v 4 "$2" > "$WORK/zip-alignment.log"
    fi
}

install() {
    local result
    result="$("${ADB[@]}" install -r "$1")"
    grep -q '^Success' <<< "$result" || { printf '%s\n' "$result" >&2; return 1; }
}

bash "$ROOT/tests/scripts/build-smoke-multidex.sh" "$FIXTURE" "$WORK/input-unsigned.apk" 19 >/dev/null
keytool -genkeypair -noprompt -storetype PKCS12 -keystore "$WORK/smoke.p12" \
    -storepass mocika-test-123 -keypass mocika-test-123 -alias smoke \
    -keyalg RSA -keysize 2048 -validity 3650 -dname 'CN=Mocika Shield Regression, O=MocikaDev, C=CN' >/dev/null 2>&1
sign "$WORK/input-unsigned.apk" "$WORK/input.apk"
INPUT_SHA="$(shasum -a 256 "$WORK/input.apk" | awk '{print $1}')"

PROFILES=(resources.zip resources-api19.zip)
if [[ "$API" == 19 ]]; then PROFILES=(resources-api19.zip); fi
for resource in "${PROFILES[@]}"; do
    test -f "$PREVIOUS/$resource" || { echo "缺少旧资源：$resource" >&2; exit 1; }
    python3 "$ROOT/scripts/stub_bindings.py" audit "$MAPPING" "$CURRENT/$resource" >/dev/null
    echo "开始：$resource"
    for generation in old new; do
        source="$PREVIOUS/$resource"
        [[ "$generation" != new ]] || source="$CURRENT/$resource"
        "$CLI" protect -i "$WORK/input.apk" -o "$WORK/$generation-unsigned.apk" \
            --resources "$source" > "$WORK/$generation-protect.log" 2>&1 || {
                tail -60 "$WORK/$generation-protect.log" >&2; exit 1;
            }
        sign "$WORK/$generation-unsigned.apk" "$WORK/$generation.apk"
    done
    # 只清除测试包，不重置模拟器或其他应用的数据。
    "${ADB[@]}" uninstall "$PACKAGE" >/dev/null 2>&1 || true
    # 夹具没有接入旧版 MultiDex，API 19 的原生类加载器不能直接读取 classes2.dex。
    if [[ "$API" != 19 ]]; then
        install "$WORK/input.apk"
        launch "$resource 未加固双 DEX 基线"
    else
        echo 'API 19 不将未接入 MultiDex 的原始夹具作为启动基线；旧壳和新壳仍须加载两份 DEX'
    fi
    install "$WORK/old.apk"
    launch "$resource 旧壳首次启动"
    before="$(cache_state)"
    assert_cache_pair "$before"
    install "$WORK/new.apk"
    launch "$resource 同签名旧壳覆盖新壳"
    [[ "$before" == "$(cache_state)" ]] || { echo '覆盖升级重写了缓存' >&2; exit 1; }
    launch "$resource 缓存命中重启"
    [[ "$before" == "$(cache_state)" ]] || { echo '重启重写了缓存' >&2; exit 1; }
    "${ADB[@]}" shell pm clear "$PACKAGE" | grep -q '^Success'
    launch "$resource 清数据后重新解密"
    before="$(cache_state)"
    assert_cache_pair "$before"
    launch "$resource 清数据后的缓存复用"
    [[ "$before" == "$(cache_state)" ]] || { echo '清数据后重启未复用缓存' >&2; exit 1; }
    [[ "$INPUT_SHA" == "$(shasum -a 256 "$WORK/input.apk" | awk '{print $1}')" ]] || {
        echo '输入 APK 被修改' >&2; exit 1;
    }
done
echo '指定模拟器的壳资源覆盖升级回归通过'
