#!/usr/bin/env bash
# 构建带独立自动化 CLI 的 macOS 应用包；不安装、不改 PATH、不生成独立 CLI 发布包。
# 用法：bash scripts/build-macos-app.sh [universal]
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BUILD_UNIVERSAL="${1:-}"

if [[ "$OSTYPE" != darwin* ]]; then
  echo "此脚本仅支持 macOS。" >&2
  exit 1
fi
if [[ "$#" -gt 1 || ( -n "$BUILD_UNIVERSAL" && "$BUILD_UNIVERSAL" != universal ) ]]; then
  echo "用法：bash scripts/build-macos-app.sh [universal]" >&2
  exit 2
fi
if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
  echo "共包入口固定使用仓库 target 目录，请移除本次命令的 CARGO_TARGET_DIR。" >&2
  exit 1
fi

cd "$ROOT"
VERSION=$(sed -n 's/^version = "\([^"]*\)"/\1/p' apps/shield-cli/Cargo.toml | head -n 1)
RESOURCE_DIR="$ROOT/shield-stub/build/outputs/resources"
python3 -c 'import json,sys; sys.exit(0 if json.load(open(sys.argv[2]))["version"] == sys.argv[1] else "GUI 与 CLI 版本不一致")' \
  "$VERSION" "$ROOT/apps/shield-gui/src-tauri/tauri.conf.json"
python3 scripts/check_resource_version.py "$VERSION" \
  "$RESOURCE_DIR/resources.zip" "$RESOURCE_DIR/resources-api19.zip"
for name in apktool_3.0.1.jar apksigner.jar; do
  [[ -f "$ROOT/tools/$name" ]] || { echo "缺少工具：tools/$name" >&2; exit 1; }
done

python3 "$ROOT/scripts/build_java_adapter.py"

TAURI_ARGS=(--bundles app)
if [[ "$BUILD_UNIVERSAL" == universal ]]; then
  for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --locked --release -p shield-cli --features mcp-server --target "$target"
  done
  mkdir -p "$ROOT/target/universal/release"
  lipo -create "$ROOT/target/aarch64-apple-darwin/release/shield" \
    "$ROOT/target/x86_64-apple-darwin/release/shield" \
    -output "$ROOT/target/universal/release/shield"
  CLI_BIN="$ROOT/target/universal/release/shield"
  BUNDLE_BASE="$ROOT/target/universal-apple-darwin/release/bundle"
  TAURI_ARGS+=(--target universal-apple-darwin)
  ARCHES=(arm64 x86_64)
else
  cargo build --locked --release -p shield-cli --features mcp-server
  CLI_BIN="$ROOT/target/release/shield"
  BUNDLE_BASE="$ROOT/target/release/bundle"
  ARCHES=("$(uname -m)")
fi

cd "$ROOT/apps/shield-gui"
cargo tauri build "${TAURI_ARGS[@]}"
APP_DIR="$BUNDLE_BASE/macos/MocikaShield.app"
GUI_BIN="$APP_DIR/Contents/MacOS/mocika-shield"
[[ -f "$GUI_BIN" ]] || { echo "未生成预期 GUI 应用包：$APP_DIR" >&2; exit 1; }
lipo "$GUI_BIN" -verify_arch "${ARCHES[@]}"
lipo "$CLI_BIN" -verify_arch "${ARCHES[@]}"

# 资源只保留一份；先比对实际打包内容，避免对过期或不完整的应用重新签名。
cmp "$ROOT/tools/apktool_3.0.1.jar" "$APP_DIR/Contents/Resources/tools/apktool.jar"
cmp "$ROOT/tools/apksigner.jar" "$APP_DIR/Contents/Resources/tools/apksigner.jar"
cmp "$ROOT/tools/java-adapter/build/java-adapter.jar" "$APP_DIR/Contents/Resources/tools/java-adapter.jar"
for name in resources.zip resources-api19.zip; do
  cmp "$RESOURCE_DIR/$name" "$APP_DIR/Contents/Resources/resources/$name"
done
install -m 755 "$CLI_BIN" "$APP_DIR/Contents/MacOS/shield"
# Skill 独立分发，既不依赖源码技能目录，也不允许旧构建残留进入新应用。
for path in "$APP_DIR/Contents/Resources/skills" "$APP_DIR/Contents/Resources/integrations/skills"; do
  if [[ -e "$path" || -L "$path" ]]; then
    echo "应用包残留旧 Skill 资源，请使用干净的应用输出重新构建。" >&2
    exit 1
  fi
done
# 内层可执行文件先签名，然后签完整应用；发布脚本只能从这个最终应用制作 DMG。
codesign --force --sign - "$APP_DIR/Contents/MacOS/shield"
codesign --force --deep --sign - "$APP_DIR"
codesign --verify --deep --strict --verbose=2 "$APP_DIR"
"$APP_DIR/Contents/MacOS/shield" --version
"$APP_DIR/Contents/MacOS/shield" agent --help >/dev/null
"$APP_DIR/Contents/MacOS/shield" mcp serve --help >/dev/null
echo "已完成 GUI 与独立 CLI 共包：$APP_DIR"
