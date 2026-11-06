#!/bin/bash
# 只定位候选 CLI，不执行、不安装、不保存配置；由调用者确认来源、接口及协议。
set -eu

if [[ $# -gt 1 ]]; then
  echo "只接受一个可选的 CLI 绝对路径。" >&2
  exit 2
fi

normalize() {
  local path="$1" parent directory
  case "$path" in
    *$'\n'*|*$'\r'*) return 1 ;;
  esac
  [[ -f "$path" && -x "$path" ]] || return 1
  directory="${path%/*}"
  # 只用 shell 内建，PATH 无系统工具时也可定位；哨兵保留路径末尾换行供拒绝检查。
  parent=$(cd -P -- "${directory:-/}" 2>/dev/null && printf '%s/.' "$PWD") || return 1
  parent="${parent%/.}"
  case "$parent" in
    *$'\n'*|*$'\r'*) return 1 ;;
  esac
  printf '%s/%s\n' "$parent" "${path##*/}"
}

explicit() {
  local candidate="$1" resolved
  if [[ "$candidate" != /* ]] || ! resolved=$(normalize "$candidate"); then
    echo "已指定的 CLI 路径无效，需可执行文件的绝对路径；不会自动回退。" >&2
    exit 2
  fi
  printf '%s\n' "$resolved"
}

if [[ $# -eq 1 ]]; then
  explicit "$1"
  exit 0
fi
if [[ ${MOCIKA_SHIELD_CLI+x} ]]; then
  explicit "$MOCIKA_SHIELD_CLI"
  exit 0
fi

remaining="${PATH-}"
selected=""
while :; do
  case "$remaining" in
    *:*) directory="${remaining%%:*}"; remaining="${remaining#*:}"; more=1 ;;
    *) directory="$remaining"; more=0 ;;
  esac
  # 空 PATH 段按通常含义指向当前目录，不递归搜索其他位置。
  if candidate=$(normalize "${directory:-.}/shield"); then
    if [[ -z "$selected" ]]; then
      selected="$candidate"
    elif [[ ! "$selected" -ef "$candidate" ]]; then
      echo "PATH 中存在多个不同 CLI，请明确指定本次路径或配置 MOCIKA_SHIELD_CLI。" >&2
      exit 3
    fi
  fi
  [[ "$more" == 1 ]] || break
done
if [[ -z "$selected" ]]; then
  echo "未找到可用 CLI，请提供实际路径或配置 MOCIKA_SHIELD_CLI。" >&2
  exit 1
fi
printf '%s\n' "$selected"
