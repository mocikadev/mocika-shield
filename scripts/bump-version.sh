#!/usr/bin/env bash
# scripts/bump-version.sh — 一键将版本号同步到所有需要的位置
#
# 用法:
#   bash scripts/bump-version.sh <版本号>
#   bash scripts/bump-version.sh 1.2.0
#   bash scripts/bump-version.sh 1.2.0-rc.1
#
# 修改的文件（shield-cli/Cargo.toml 为单一来源，其余自动跟随）:
#   shield-cli/Cargo.toml
#   shield-gui/src-tauri/Cargo.toml
#   shield-stub/src/main/rust/Cargo.toml
#   shield-gui/src-tauri/tauri.conf.json
#   shield-gui/package.json
#   shield-gui/package-lock.json

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."

VERSION="${1:-}"
if [[ -z "$VERSION" ]]; then
  echo "错误：缺少版本号参数"
  echo "用法: bash scripts/bump-version.sh <版本号>"
  echo "例如: bash scripts/bump-version.sh 1.2.0"
  exit 1
fi

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
  echo "错误：版本号格式不合法，期望 x.y.z 或 x.y.z-预发布，实际: $VERSION"
  exit 1
fi

# macOS(BSD) sed 需要 -i ''，Linux(GNU) sed 只需 -i
if [[ "$(uname)" == "Darwin" ]]; then
  si() { sed -i '' "$@"; }
else
  si() { sed -i "$@"; }
fi

echo "==> 同步版本号到 $VERSION"

# [package] 段的 version 字段以 ^ 锁定行首，不会误改依赖声明
si "s/^version = \"[^\"]*\"/version = \"$VERSION\"/" "$ROOT/shield-cli/Cargo.toml"
echo "  ✓ shield-cli/Cargo.toml"

si "s/^version = \"[^\"]*\"/version = \"$VERSION\"/" "$ROOT/shield-gui/src-tauri/Cargo.toml"
echo "  ✓ shield-gui/src-tauri/Cargo.toml"

si "s/^version = \"[^\"]*\"/version = \"$VERSION\"/" "$ROOT/shield-stub/src/main/rust/Cargo.toml"
echo "  ✓ shield-stub/src/main/rust/Cargo.toml"

# tauri.conf.json 顶层 "version" 字段缩进固定为 2 空格
si "s/^  \"version\": \"[^\"]*\"/  \"version\": \"$VERSION\"/" "$ROOT/shield-gui/src-tauri/tauri.conf.json"
echo "  ✓ shield-gui/src-tauri/tauri.conf.json"

(cd "$ROOT/shield-gui" && npm version "$VERSION" --no-git-tag-version --allow-same-version >/dev/null)
echo "  ✓ shield-gui/package.json"
echo "  ✓ shield-gui/package-lock.json"

si "s/version: \"[^\"]*\", git_hash: \"dev\"/version: \"$VERSION\", git_hash: \"dev\"/" "$ROOT/shield-gui/src/App.tsx"
echo "  ✓ shield-gui/src/App.tsx"

echo ""
echo "验证结果："
grep '^version'   "$ROOT/shield-cli/Cargo.toml"                | head -1 | sed 's/^/    shield-cli\/Cargo.toml          : /'
grep '^version'   "$ROOT/shield-gui/src-tauri/Cargo.toml"      | head -1 | sed 's/^/    shield-gui\/Cargo.toml          : /'
grep '^version'   "$ROOT/shield-stub/src/main/rust/Cargo.toml" | head -1 | sed 's/^/    shield-stub\/Cargo.toml         : /'
grep '"version"'  "$ROOT/shield-gui/src-tauri/tauri.conf.json" | head -1 | sed 's/^/    tauri.conf.json                 : /'
grep '"version"'  "$ROOT/shield-gui/package.json"              | head -1 | sed 's/^/    shield-gui\/package.json       : /'
echo ""
echo "完成。下一步：git add + git commit，然后 make release-linux/macos/windows"
