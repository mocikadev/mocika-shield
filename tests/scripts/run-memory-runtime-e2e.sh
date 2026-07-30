#!/usr/bin/env bash

# 验证内存候选资源的系统边界、双向迁移、认证状态与失败关闭语义。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FIXTURE="$ROOT/tests/fixtures/android-smoke-app"
PACKAGE_NAME="dev.mocika.shield.smoke"
COMPONENT="$PACKAGE_NAME/.MainActivity"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/mocika-memory-runtime-e2e.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

for command in adb python3 shasum; do
    command -v "$command" >/dev/null || { echo "缺少命令：$command" >&2; exit 1; }
done

ADB=(adb)
if [[ -n "${ANDROID_SERIAL:-}" ]]; then
    ADB+=(-s "$ANDROID_SERIAL")
fi
"${ADB[@]}" get-state >/dev/null
SDK_VERSION="$("${ADB[@]}" shell getprop ro.build.version.sdk | tr -d '\r')"
if (( SDK_VERSION < 28 )); then
    echo "错误：内存候选回归要求 API 28 以上，当前为 API $SDK_VERSION" >&2
    exit 1
fi

if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
    make -C "$ROOT" build-stub
    "$ROOT/shield-stub/gradlew" -p "$FIXTURE" :app:assembleDebug --no-daemon
    cargo build --release -p shield-cli --manifest-path "$ROOT/Cargo.toml"
fi

STANDARD_APK="$WORK/output-standard-signed.apk"
MEMORY_APK="$WORK/output-memory-signed.apk"
"$ROOT/tests/scripts/prepare-memory-runtime-e2e-apks.sh" "$WORK"

BASE_MARKERS=(
    MOCIKA_SMOKE_APPLICATION_OK
    MOCIKA_SMOKE_PROVIDER_OK
    MOCIKA_SMOKE_ACTIVITY_OK
    MOCIKA_SMOKE_RECEIVER_OK
    MOCIKA_SMOKE_REMOTE_SERVICE_OK
    MOCIKA_SMOKE_SECONDARY_OK
)
FACTORY_MARKERS=(
    MOCIKA_SMOKE_FACTORY_APPLICATION
    MOCIKA_SMOKE_FACTORY_PROVIDER
    MOCIKA_SMOKE_FACTORY_ACTIVITY
    MOCIKA_SMOKE_FACTORY_RECEIVER
    MOCIKA_SMOKE_FACTORY_SERVICE
)

install_clean() {
    "${ADB[@]}" uninstall "$PACKAGE_NAME" >/dev/null 2>&1 || true
    "${ADB[@]}" install "$1" >/dev/null
}

install_replace() {
    "${ADB[@]}" install -r "$1" >/dev/null
}

verify_full_launch() {
    local scenario="$1"
    local expect_factory="${2:-1}"
    "${ADB[@]}" shell am force-stop "$PACKAGE_NAME" >/dev/null 2>&1 || true
    "${ADB[@]}" logcat -c
    "${ADB[@]}" shell am start -W -n "$COMPONENT" >/dev/null
    local logs missing=1
    for _ in $(seq 1 30); do
        logs="$("${ADB[@]}" logcat -d -s MocikaSmoke:I AndroidRuntime:E '*:S')"
        missing=0
        for marker in "${BASE_MARKERS[@]}"; do
            grep -q "$marker" <<< "$logs" || missing=1
        done
        if [[ "$expect_factory" == "1" ]]; then
            for marker in "${FACTORY_MARKERS[@]}"; do
                grep -q "$marker" <<< "$logs" || missing=1
            done
        fi
        if (( missing == 0 )); then
            echo "$scenario 验证通过"
            return 0
        fi
        sleep 1
    done
    echo "错误：$scenario 未观察到完整组件和多 DEX 标记" >&2
    printf '%s\n' "$logs" >&2
    return 1
}

verify_rejected() {
    local scenario="$1"
    local code="$2"
    "${ADB[@]}" shell am force-stop "$PACKAGE_NAME" >/dev/null 2>&1 || true
    "${ADB[@]}" logcat -c
    "${ADB[@]}" shell am start -W -n "$COMPONENT" >/dev/null 2>&1 || true
    local logs
    for _ in $(seq 1 15); do
        logs="$("${ADB[@]}" logcat -d -s AndroidRuntime:E '*:S')"
        if grep -Fq "$code" <<< "$logs"; then
            echo "$scenario 验证通过"
            return 0
        fi
        sleep 1
    done
    echo "错误：$scenario 未观察到 $code" >&2
    printf '%s\n' "$logs" >&2
    return 1
}

private_files() {
    "${ADB[@]}" shell run-as "$PACKAGE_NAME" find . -type f 2>/dev/null | tr -d '\r'
}

assert_file_mode() {
    local files
    files="$(private_files)"
    grep -q '^./app_app_dex/.*\.dex$' <<< "$files" || {
        echo "错误：未生成认证文件缓存" >&2
        printf '%s\n' "$files" >&2
        exit 1
    }
}

assert_memory_mode_without_plaintext() {
    local files state_count
    files="$(private_files)"
    state_count="$(grep -c '^./no_backup/runtime_state/' <<< "$files" || true)"
    if (( state_count < 2 )); then
        echo "错误：缺少主进程和远程进程认证状态" >&2
        printf '%s\n' "$files" >&2
        exit 1
    fi
    if grep -Eq '(^|/)(app_app_dex|[^/]*\.dex$)' <<< "$files"; then
        echo "错误：内存路径产生了明文 DEX 文件" >&2
        printf '%s\n' "$files" >&2
        exit 1
    fi
}

process_id() {
    printf '%s' "$1" | shasum -a 256 | cut -c1-24
}

state_path() {
    echo "no_backup/runtime_state/$(process_id "$1").bin"
}

state_contains() {
    local process_name="$1"
    local expected="$2"
    "${ADB[@]}" exec-out run-as "$PACKAGE_NAME" cat "$(state_path "$process_name")" \
        | grep -aFq "$expected"
}

trigger_memory_crash() {
    "${ADB[@]}" shell run-as "$PACKAGE_NAME" touch files/crash_memory_once
    "${ADB[@]}" shell am force-stop "$PACKAGE_NAME" >/dev/null
    "${ADB[@]}" logcat -c
    "${ADB[@]}" shell am start -W -n "$COMPONENT" >/dev/null 2>&1 || true
    sleep 1
    local logs
    logs="$("${ADB[@]}" logcat -d -s MocikaSmoke:I AndroidRuntime:E '*:S')"
    grep -Fq 'MOCIKA_SMOKE_CRASH_ONCE' <<< "$logs" || {
        echo "错误：未触发真实 Application 一次性退出" >&2
        printf '%s\n' "$logs" >&2
        exit 1
    }
}

mutate_state() {
    local process_name="$1"
    local mode="$2"
    local path
    local source_file="$WORK/state-source.bin"
    local mutated_file="$WORK/state-mutated.bin"
    path="$(state_path "$process_name")"
    "${ADB[@]}" exec-out run-as "$PACKAGE_NAME" cat "$path" > "$source_file"
    test -s "$source_file" || { echo "错误：未读取到恢复状态" >&2; exit 1; }
    python3 -c '
import struct, sys
data = bytearray(open(sys.argv[1], "rb").read())
mode = sys.argv[2]
if mode == "mac":
    data[-1] ^= 1
elif mode == "wrapped":
    pos = 8
    for _ in range(3):
        length = struct.unpack_from(">H", data, pos)[0]
        pos += 2 + length
    iv_length = struct.unpack_from(">I", data, pos)[0]
    pos += 4 + iv_length
    key_length = struct.unpack_from(">I", data, pos)[0]
    pos += 4
    if key_length < 1:
        raise SystemExit("包装密文为空")
    data[pos] ^= 1
else:
    raise SystemExit("未知损坏模式")
open(sys.argv[3], "wb").write(data)
' "$source_file" "$mode" "$mutated_file"
    "${ADB[@]}" shell "run-as $PACKAGE_NAME sh -c 'cat > $path'" < "$mutated_file"
}

verify_basic_boundary() {
    install_clean "$MEMORY_APK"
    verify_full_launch "候选资源首次安装"
    if (( SDK_VERSION <= 30 )); then
        assert_file_mode
        if private_files | grep -q '^./no_backup/runtime_state/'; then
            echo "错误：API $SDK_VERSION 不应生成内存运行状态" >&2
            exit 1
        fi
        echo "Android API $SDK_VERSION 认证文件路径端到端回归通过"
        return
    fi
    assert_memory_mode_without_plaintext
    "${ADB[@]}" shell pm clear "$PACKAGE_NAME" >/dev/null
    verify_full_launch "清除数据后内存重新初始化"
    assert_memory_mode_without_plaintext
}

verify_bidirectional_migration() {
    install_clean "$STANDARD_APK"
    verify_full_launch "标准文件资源首次安装" 0
    assert_file_mode

    install_replace "$MEMORY_APK"
    verify_full_launch "标准文件资源覆盖升级到内存候选"
    state_contains "$PACKAGE_NAME" memory_ready

    install_replace "$STANDARD_APK"
    verify_full_launch "内存候选回滚到标准文件资源" 0
    assert_file_mode

    install_replace "$MEMORY_APK"
    verify_full_launch "标准文件资源再次升级到内存候选"
    state_contains "$PACKAGE_NAME" memory_ready
    echo "标准与内存资源双向覆盖迁移验证通过"
}

verify_recovery_and_process_isolation() {
    install_clean "$MEMORY_APK"
    verify_full_launch "崩溃回退基线"
    trigger_memory_crash
    if state_contains "$PACKAGE_NAME" memory_pending; then
        verify_full_launch "内存启动异常后的认证文件回退"
    elif ! state_contains "$PACKAGE_NAME" file_fallback; then
        echo "错误：内存退出后未保持待回退或已回退状态" >&2
        exit 1
    fi
    assert_file_mode
    state_contains "$PACKAGE_NAME" file_fallback
    state_contains "$PACKAGE_NAME:remote" memory_ready
    verify_full_launch "认证文件回退粘性重启"
    echo "内存崩溃回退与主远程进程状态隔离验证通过"
}

verify_file_failure_closed() {
    install_clean "$MEMORY_APK"
    verify_full_launch "文件失败关闭基线"
    "${ADB[@]}" shell "run-as $PACKAGE_NAME sh -c 'rm -rf app_app_dex && touch app_app_dex'"
    trigger_memory_crash
    if state_contains "$PACKAGE_NAME" memory_pending; then
        "${ADB[@]}" shell am start -W -n "$COMPONENT" >/dev/null 2>&1 || true
        sleep 1
    fi
    state_contains "$PACKAGE_NAME" file_pending
    verify_rejected "认证文件回退再次失败后关闭启动" R01
}

verify_state_authentication_failures() {
    install_clean "$MEMORY_APK"
    verify_full_launch "状态 MAC 损坏基线"
    "${ADB[@]}" shell am force-stop "$PACKAGE_NAME" >/dev/null
    mutate_state "$PACKAGE_NAME" mac
    verify_rejected "状态 MAC 损坏后关闭启动" R07

    install_clean "$MEMORY_APK"
    verify_full_launch "包装密文损坏基线"
    "${ADB[@]}" shell am force-stop "$PACKAGE_NAME" >/dev/null
    mutate_state "$PACKAGE_NAME" wrapped
    verify_rejected "包装密文损坏后关闭启动" R11

    install_clean "$MEMORY_APK"
    verify_full_launch "Keystore 条目删除基线"
    "${ADB[@]}" logcat -c
    "${ADB[@]}" shell am force-stop "$PACKAGE_NAME" >/dev/null
    "${ADB[@]}" shell am start -W -n "$COMPONENT" \
        --ez dev.mocika.shield.smoke.DELETE_RECOVERY_KEYS true >/dev/null
    sleep 1
    local logs
    logs="$("${ADB[@]}" logcat -d -s MocikaSmoke:I AndroidRuntime:E '*:S')"
    grep -Fq 'MOCIKA_SMOKE_RECOVERY_KEYS_DELETED' <<< "$logs" || {
        echo "错误：测试夹具未删除恢复密钥" >&2
        printf '%s\n' "$logs" >&2
        exit 1
    }
    verify_rejected "Keystore 条目删除后关闭启动" R11
    echo "状态、包装密文和 Keystore 异常失败关闭验证通过"
}

verify_basic_boundary
if (( SDK_VERSION >= 31 )); then
    verify_bidirectional_migration
    verify_recovery_and_process_isolation
    verify_file_failure_closed
    verify_state_authentication_failures
fi

"${ADB[@]}" uninstall "$PACKAGE_NAME" >/dev/null 2>&1 || true
echo "Android API $SDK_VERSION 内存候选状态与迁移端到端回归通过"
