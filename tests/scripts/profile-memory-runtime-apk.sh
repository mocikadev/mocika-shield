#!/usr/bin/env bash

# 对同签名标准/内存 APK 采集日常冷启动、内存整理前后数据和候选阶段日志。

set -euo pipefail

STANDARD_APK="${1:?请提供标准模式已签名 APK}"
MEMORY_APK="${2:?请提供内存模式已签名 APK}"
PACKAGE_NAME="${3:?请提供应用包名}"
COMPONENT="${4:?请提供启动组件}"
SAMPLE_COUNT="${SAMPLE_COUNT:-3}"
SETTLE_SECONDS="${SETTLE_SECONDS:-15}"
OUTPUT_DIR="${PROFILE_OUTPUT_DIR:-$(pwd)/target/test-results/memory-runtime-profile}"

for command in adb awk sed; do
    command -v "$command" >/dev/null || { echo "缺少命令：$command" >&2; exit 1; }
done
for apk in "$STANDARD_APK" "$MEMORY_APK"; do
    [[ -f "$apk" ]] || { echo "APK 不存在：$apk" >&2; exit 1; }
done
if ! [[ "$SAMPLE_COUNT" =~ ^[0-9]+$ ]] || (( SAMPLE_COUNT < 1 )); then
    echo "SAMPLE_COUNT 必须是正整数" >&2
    exit 1
fi

ADB=(adb)
if [[ -n "${ANDROID_SERIAL:-}" ]]; then
    ADB+=(-s "$ANDROID_SERIAL")
fi
"${ADB[@]}" get-state >/dev/null
mkdir -p "$OUTPUT_DIR"
SUMMARY="$OUTPUT_DIR/summary.tsv"
printf '形态\t轮次\t启动毫秒\t计时来源\t阶段\tPSS_KB\tRSS_KB\t匿名映射_KB\t私有脏页_KB\t进程数\t崩溃\n' > "$SUMMARY"

process_ids() {
    "${ADB[@]}" shell ps -A -o PID,NAME | awk -v package="$PACKAGE_NAME" '
        $2 == package || index($2, package ":") == 1 { print $1 }
    ' | tr -d '\r'
}

meminfo_value() {
    local pid="$1"
    local pattern="$2"
    "${ADB[@]}" shell dumpsys meminfo "$pid" | awk -v pattern="$pattern" '
        index($0, pattern) {
            value = substr($0, index($0, pattern) + length(pattern))
            if (match(value, /[0-9]+/)) print substr(value, RSTART, RLENGTH)
            exit
        }
    ' | tr -d '\r '
}

rollup_value() {
    local pid="$1"
    local field="$2"
    "${ADB[@]}" shell "cat /proc/$pid/smaps_rollup" 2>/dev/null | awk -v field="$field" '
        $1 == field ":" { print $2; exit }
    ' | tr -d '\r '
}

snapshot() {
    local variant="$1"
    local iteration="$2"
    local phase="$3"
    local launch_time="$4"
    local timing_source="$5"
    local total_pss=0 total_rss=0 total_anon=0 total_private_dirty=0 process_count=0
    local pid pss rss anon private_dirty
    local raw_dir="$OUTPUT_DIR/$variant-$iteration-$phase"
    mkdir -p "$raw_dir"

    while read -r pid; do
        [[ -n "$pid" ]] || continue
        process_count=$((process_count + 1))
        "${ADB[@]}" shell dumpsys meminfo "$pid" > "$raw_dir/meminfo-$pid.txt"
        pss="$(meminfo_value "$pid" "TOTAL PSS:")"
        rss="$(meminfo_value "$pid" "TOTAL RSS:")"
        [[ "$pss" =~ ^[0-9]+$ ]] || pss=0
        [[ "$rss" =~ ^[0-9]+$ ]] || rss=0
        total_pss=$((total_pss + pss))
        total_rss=$((total_rss + rss))

        if "${ADB[@]}" shell "cat /proc/$pid/smaps_rollup" > "$raw_dir/smaps-rollup-$pid.txt" 2>/dev/null; then
            anon="$(rollup_value "$pid" Anonymous)"
            private_dirty="$(rollup_value "$pid" Private_Dirty)"
            [[ "$anon" =~ ^[0-9]+$ ]] || anon=0
            [[ "$private_dirty" =~ ^[0-9]+$ ]] || private_dirty=0
            total_anon=$((total_anon + anon))
            total_private_dirty=$((total_private_dirty + private_dirty))
        fi
    done < <(process_ids)

    local crashes
    crashes="$("${ADB[@]}" logcat -d -v brief | awk -v package="$PACKAGE_NAME" '
        /FATAL EXCEPTION/ { fatal++ }
        index($0, "Process: " package) { process++ }
        END { print fatal + process }
    ')"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$variant" "$iteration" "$launch_time" "$timing_source" "$phase" \
        "$total_pss" "$total_rss" "$total_anon" "$total_private_dirty" \
        "$process_count" "$crashes" >> "$SUMMARY"
}

measure_variant() {
    local variant="$1"
    local apk="$2"
    "${ADB[@]}" install -r "$apk" >/dev/null
    "${ADB[@]}" shell pm clear "$PACKAGE_NAME" >/dev/null
    "${ADB[@]}" shell am start -W -n "$COMPONENT" >/dev/null
    sleep 20

    for iteration in $(seq 1 "$SAMPLE_COUNT"); do
        local launch launch_time timing_source
        "${ADB[@]}" shell am force-stop "$PACKAGE_NAME" >/dev/null
        "${ADB[@]}" logcat -c
        launch="$("${ADB[@]}" shell am start -W -n "$COMPONENT")"
        launch_time="$(awk -F': ' '/^TotalTime:/ { gsub("\r", "", $2); print $2; exit }' <<< "$launch")"
        timing_source="TotalTime"
        if ! [[ "$launch_time" =~ ^[0-9]+$ ]]; then
            launch_time="$(awk -F': ' '/^WaitTime:/ { gsub("\r", "", $2); print $2; exit }' <<< "$launch")"
            timing_source="WaitTime"
        fi
        [[ "$launch_time" =~ ^[0-9]+$ ]] || { launch_time=0; timing_source="不可用"; }
        sleep "$SETTLE_SECONDS"
        snapshot "$variant" "$iteration" settled "$launch_time" "$timing_source"
        "${ADB[@]}" shell am send-trim-memory "$PACKAGE_NAME" RUNNING_LOW >/dev/null
        sleep 3
        snapshot "$variant" "$iteration" trimmed "$launch_time" "$timing_source"
        "${ADB[@]}" logcat -d -v threadtime -s mxp:I '*:S' \
            > "$OUTPUT_DIR/$variant-$iteration-runtime.log"
    done
}

measure_variant standard "$STANDARD_APK"
measure_variant memory "$MEMORY_APK"

echo "剖析完成：$SUMMARY"
