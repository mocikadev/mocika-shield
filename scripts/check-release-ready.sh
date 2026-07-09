#!/usr/bin/env bash
# 发布前轻量检查：不构建产物，只检查仓库状态、版本同步、文档资源和敏感文件。

set -euo pipefail

ROOT="$(cd -- "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

failures=0

info() {
  printf '==> %s\n' "$1"
}

ok() {
  printf '  ✓ %s\n' "$1"
}

fail() {
  printf '  ✗ %s\n' "$1"
  failures=$((failures + 1))
}

warn() {
  printf '  ! %s\n' "$1"
}

read_toml_version() {
  sed -n 's/^version = "\(.*\)"/\1/p' "$1" | head -1
}

read_json_version() {
  sed -n 's/^[[:space:]]*"version": "\(.*\)",\{0,1\}/\1/p' "$1" | head -1
}

expect_file() {
  local path="$1"
  local label="$2"
  if [[ -f "$path" ]]; then
    ok "$label"
  else
    fail "$label 缺失: $path"
  fi
}

info "检查工作区状态"
if [[ -z "$(git status --porcelain)" ]]; then
  ok "工作区干净"
else
  fail "工作区存在未提交变更"
  git status --short
fi

info "检查版本号同步"
version_files=(
  "crates/shield-core/Cargo.toml"
  "apps/shield-cli/Cargo.toml"
  "shield-stub/src/main/rust/Cargo.toml"
  "apps/shield-gui/src-tauri/Cargo.toml"
  "apps/shield-gui/src-tauri/tauri.conf.json"
  "apps/shield-gui/package.json"
  "apps/shield-gui/package-lock.json"
)

base_version="$(read_toml_version crates/shield-core/Cargo.toml)"
if [[ -z "$base_version" ]]; then
  fail "无法读取 crates/shield-core/Cargo.toml 版本号"
else
  for file in "${version_files[@]}"; do
    case "$file" in
      *.toml) version="$(read_toml_version "$file")" ;;
      *.json) version="$(read_json_version "$file")" ;;
      *) version="" ;;
    esac
    if [[ "$version" == "$base_version" ]]; then
      ok "$file = $version"
    else
      fail "$file 版本不一致：$version，期望 $base_version"
    fi
  done
fi

info "检查构建必需资源"
expect_file "tools/apktool_3.0.1.jar" "开发工具 apktool 存在"
expect_file "tools/apksigner.jar" "开发工具 apksigner 存在"
expect_file "shield-stub/build/outputs/resources/resources.zip" "resources.zip 已生成"

info "检查 README 截图引用"
screenshot_refs=()
while IFS= read -r ref; do
  screenshot_refs+=("$ref")
done < <(grep -Eo 'docs/assets/screenshots/[^) ]+\.(png|jpg|jpeg|webp)' README.md | sort -u)
if [[ "${#screenshot_refs[@]}" -eq 0 ]]; then
  fail "README 未找到截图引用"
else
  for ref in "${screenshot_refs[@]}"; do
    expect_file "$ref" "截图存在: $ref"
  done
fi

info "检查开源仓库治理文件"
expect_file "SECURITY.md" "SECURITY.md 存在"
expect_file "docs/process/support.md" "支持与问题反馈文档存在"
expect_file ".github/ISSUE_TEMPLATE/bug_report.yml" "Bug issue 模板存在"
expect_file ".github/ISSUE_TEMPLATE/compatibility.yml" "兼容性 issue 模板存在"
expect_file ".github/ISSUE_TEMPLATE/config.yml" "Issue 模板配置存在"
expect_file ".github/workflows/ci.yml" "CI workflow 存在"
expect_file ".github/workflows/release.yml" "Release workflow 存在"

info "检查已跟踪敏感或本地产物"
tracked_sensitive="$(
  git ls-files \
    | grep -E '(^|/)(shield\.db|config\.toml|tool_config\.json|node_modules/)|\.(apk|aab|jks|p12|keystore|env)$' \
    | grep -Ev '(^|/)\.cargo/config\.toml$' \
    || true
)"
if [[ -z "$tracked_sensitive" ]]; then
  ok "未发现已跟踪的敏感文件或本地产物"
else
  fail "发现已跟踪的敏感文件或本地产物"
  printf '%s\n' "$tracked_sensitive"
fi

info "检查未跟踪敏感或本地产物"
untracked_sensitive="$(git ls-files --others --exclude-standard | grep -E '(^|/)(shield\.db|config\.toml|tool_config\.json)|\.(apk|aab|jks|p12|keystore|env)$' || true)"
if [[ -z "$untracked_sensitive" ]]; then
  ok "未发现未跟踪的敏感文件或本地产物"
else
  fail "发现未跟踪的敏感文件或本地产物"
  printf '%s\n' "$untracked_sensitive"
fi

if [[ -d apps/shield-gui/node_modules ]]; then
  warn "apps/shield-gui/node_modules 存在但已被 .gitignore 忽略"
fi

if [[ "$failures" -gt 0 ]]; then
  printf '\n发布前检查未通过：%d 项失败\n' "$failures"
  exit 1
fi

printf '\n发布前检查通过。\n'
