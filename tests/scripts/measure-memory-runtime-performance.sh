#!/usr/bin/env bash

# 对比标准认证文件路径与内存候选路径的稳态冷启动和双进程 PSS。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FIXTURE="$ROOT/tests/fixtures/android-smoke-app"
PACKAGE_NAME="dev.mocika.shield.smoke"
COMPONENT="$PACKAGE_NAME/.MainActivity"
SAMPLE_COUNT="${SAMPLE_COUNT:-7}"
VARIANT_ORDER="${VARIANT_ORDER:-standard-first}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/mocika-memory-runtime-perf.XXXXXX")"
OUTPUT="${PERF_OUTPUT:-$ROOT/target/test-results/memory-runtime-performance.tsv}"
trap 'rm -rf "$WORK"' EXIT

for command in adb awk python3; do
    command -v "$command" >/dev/null || { echo "缺少命令：$command" >&2; exit 1; }
done
if ! [[ "$SAMPLE_COUNT" =~ ^[0-9]+$ ]] || (( SAMPLE_COUNT < 5 || SAMPLE_COUNT % 2 == 0 )); then
    echo "错误：SAMPLE_COUNT 必须是大于等于 5 的奇数" >&2
    exit 1
fi
if [[ "$VARIANT_ORDER" != "standard-first" && "$VARIANT_ORDER" != "memory-first" ]]; then
    echo "错误：VARIANT_ORDER 只支持 standard-first 或 memory-first" >&2
    exit 1
fi

ADB=(adb)
if [[ -n "${ANDROID_SERIAL:-}" ]]; then
    ADB+=(-s "$ANDROID_SERIAL")
fi
"${ADB[@]}" get-state >/dev/null
SDK_VERSION="$("${ADB[@]}" shell getprop ro.build.version.sdk | tr -d '\r')"
if (( SDK_VERSION < 31 )); then
    echo "错误：性能门禁只比较 API 31 以上内存路径，当前为 API $SDK_VERSION" >&2
    exit 1
fi

if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
    make -C "$ROOT" build-stub
    "$ROOT/shield-stub/gradlew" -p "$FIXTURE" :app:assembleDebug --no-daemon
    cargo build --release -p shield-cli --manifest-path "$ROOT/Cargo.toml"
fi
"$ROOT/tests/scripts/prepare-memory-runtime-e2e-apks.sh" "$WORK"
STANDARD_APK="$WORK/output-standard-signed.apk"
MEMORY_APK="$WORK/output-memory-signed.apk"

median() {
    printf '%s\n' "$@" | sort -n | awk '{ values[NR] = $1 } END { print values[int((NR + 1) / 2)] }'
}

wait_for_processes() {
    for _ in $(seq 1 30); do
        if [[ -n "$("${ADB[@]}" shell pidof "$PACKAGE_NAME" 2>/dev/null | tr -d '\r')" \
            && -n "$("${ADB[@]}" shell pidof "$PACKAGE_NAME:remote" 2>/dev/null | tr -d '\r')" ]]; then
            return 0
        fi
        sleep 0.2
    done
    echo "错误：主进程或远程进程未在限定时间内启动" >&2
    return 1
}

process_pss() {
    local process_name="$1"
    local pid value
    pid="$("${ADB[@]}" shell pidof "$process_name" | tr -d '\r' | awk '{print $1}')"
    value="$("${ADB[@]}" shell dumpsys meminfo "$pid" | awk '
        /TOTAL PSS:/ { print $3; exit }
        /^TOTAL[[:space:]]/ { print $2; exit }
    ' | tr -d '\r ')"
    [[ "$value" =~ ^[0-9]+$ ]] || {
        echo "错误：无法解析 $process_name 的 TOTAL PSS" >&2
        return 1
    }
    echo "$value"
}

total_pss() {
    local main remote
    main="$(process_pss "$PACKAGE_NAME")"
    remote="$(process_pss "$PACKAGE_NAME:remote")"
    echo $((main + remote))
}

measure_variant() {
    local variant="$1"
    local apk="$2"
    local -a times=()
    local -a immediate_values=()
    local -a settled_values=()

    "${ADB[@]}" uninstall "$PACKAGE_NAME" >/dev/null 2>&1 || true
    "${ADB[@]}" install "$apk" >/dev/null
    "${ADB[@]}" shell am start -W -n "$COMPONENT" >/dev/null
    wait_for_processes
    "${ADB[@]}" shell am force-stop "$PACKAGE_NAME" >/dev/null
    sleep 2

    for iteration in $(seq 1 "$SAMPLE_COUNT"); do
        local launch total_time immediate_pss settled_pss
        launch="$("${ADB[@]}" shell am start -W -n "$COMPONENT")"
        total_time="$(awk -F': ' '/^TotalTime:/{print $2}' <<< "$launch" | tr -d '\r ')"
        wait_for_processes
        immediate_pss="$(total_pss)"
        "${ADB[@]}" shell am send-trim-memory "$PACKAGE_NAME" RUNNING_LOW >/dev/null
        sleep 2
        settled_pss="$(total_pss)"
        [[ "$total_time" =~ ^[0-9]+$ ]] || {
            echo "错误：无法解析 $variant 第 $iteration 次 TotalTime" >&2
            exit 1
        }
        times+=("$total_time")
        immediate_values+=("$immediate_pss")
        settled_values+=("$settled_pss")
        printf '%s\t%s\t%s\t%s\t%s\n' \
            "$variant" "$iteration" "$total_time" "$immediate_pss" "$settled_pss" >> "$OUTPUT"
        "${ADB[@]}" shell am force-stop "$PACKAGE_NAME" >/dev/null
        sleep 2
    done

    printf '%s\t%s\t%s\t%s\n' "$variant" \
        "$(median "${times[@]}")" \
        "$(median "${immediate_values[@]}")" \
        "$(median "${settled_values[@]}")" >> "$WORK/summary.tsv"
}

mkdir -p "$(dirname "$OUTPUT")"
printf '形态\t轮次\t冷启动毫秒\t即时双进程PSS_KB\t整理后双进程PSS_KB\n' > "$OUTPUT"
if [[ "$VARIANT_ORDER" == "standard-first" ]]; then
    measure_variant standard "$STANDARD_APK"
    measure_variant memory "$MEMORY_APK"
else
    measure_variant memory "$MEMORY_APK"
    measure_variant standard "$STANDARD_APK"
fi
"${ADB[@]}" uninstall "$PACKAGE_NAME" >/dev/null 2>&1 || true

STANDARD_LINE="$(awk -F '\t' '$1=="standard" {print; exit}' "$WORK/summary.tsv")"
MEMORY_LINE="$(awk -F '\t' '$1=="memory" {print; exit}' "$WORK/summary.tsv")"
IFS=$'\t' read -r _ STANDARD_TIME STANDARD_IMMEDIATE STANDARD_SETTLED <<< "$STANDARD_LINE"
IFS=$'\t' read -r _ MEMORY_TIME MEMORY_IMMEDIATE MEMORY_SETTLED <<< "$MEMORY_LINE"

python3 - "$STANDARD_TIME" "$MEMORY_TIME" "$STANDARD_SETTLED" "$MEMORY_SETTLED" <<'PY'
import sys
standard_time, memory_time, standard_pss, memory_pss = map(int, sys.argv[1:])
time_delta = (memory_time - standard_time) * 100 / standard_time
pss_delta = memory_pss - standard_pss
print(f"冷启动变化：{time_delta:+.1f}%")
print(f"整理后双进程 PSS 变化：{pss_delta:+d} KB")
PY
echo "标准资源：冷启动中位数 ${STANDARD_TIME} ms，即时双进程 PSS ${STANDARD_IMMEDIATE} KB，整理后 ${STANDARD_SETTLED} KB"
echo "内存候选：冷启动中位数 ${MEMORY_TIME} ms，即时双进程 PSS ${MEMORY_IMMEDIATE} KB，整理后 ${MEMORY_SETTLED} KB"
echo "原始样本：$OUTPUT"
