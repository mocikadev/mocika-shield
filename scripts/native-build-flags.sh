#!/usr/bin/env bash

# 只规范化 Native 源码位置，参数使用 Cargo 的分隔编码以保留空格和中文路径。
configure_native_source_paths() {
    local project_source_root="$1"
    local cargo_source_root="${CARGO_HOME:-$HOME/.cargo}"
    local rustup_source_root="${RUSTUP_HOME:-$HOME/.rustup}"
    local separator=$'\x1f'
    if [[ "${OS:-}" == "Windows_NT" ]]; then
        # Git Bash 的 POSIX 路径不能直接拿来匹配 Rust 的 Windows 源码位置。
        project_source_root="$(cygpath -w "$project_source_root")"
        cargo_source_root="$(cygpath -w "$cargo_source_root")"
        rustup_source_root="$(cygpath -w "$rustup_source_root")"
    fi
    export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix${separator}${project_source_root}=.${separator}--remap-path-prefix${separator}${cargo_source_root}=.cargo${separator}--remap-path-prefix${separator}${rustup_source_root}=.rustup"
    unset RUSTFLAGS
}
