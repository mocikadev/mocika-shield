#!/usr/bin/env bash
# macOS 平台发布脚本（必须在 macOS 机器上原生运行）
# 生成：CLI（tar.gz，支持 universal binary）、GUI（.app + .dmg）
#
# 用法:
#   ./scripts/release-macos.sh [VERSION] [universal]
#   VERSION=1.2.3 ./scripts/release-macos.sh 1.2.3 universal
#
# 前置要求:
#   macOS 12+ (Monterey)
#   Xcode Command Line Tools: xcode-select --install
#   Rust: rustup target add aarch64-apple-darwin x86_64-apple-darwin
#   Tauri CLI: cargo install tauri-cli
#   Java 17+（shield-stub 构建需要）
#
# 注意:
#   - 暂不做 Apple 公证，使用 adhoc 签名（本地运行需关闭 Gatekeeper 或手动信任）
#   - universal binary 需在同一台 Mac 上分别为 arm64 和 x86_64 构建后合并

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."
VERSION="${VERSION:-${1:-1.0.0}}"
BUILD_UNIVERSAL="${2:-}"
DIST_DIR="$ROOT/dist/macos"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

info()    { echo -e "${BLUE}==> $1${NC}"; }
success() { echo -e "${GREEN}✓ $1${NC}"; }
warn()    { echo -e "${YELLOW}⚠ $1${NC}"; }
error()   { echo -e "${RED}✗ $1${NC}" >&2; exit 1; }

APP_NAME="Mocika Shield"
BUNDLE_ID="dev.mocika.shield-gui"
GUI_BINARY="shield-gui"

# ========== 检查运行环境 ==========
check_env() {
  info "检查运行环境..."

  if [[ "$OSTYPE" != "darwin"* ]]; then
    error "此脚本仅支持 macOS，当前系统: $OSTYPE"
  fi

  if ! command -v cargo &>/dev/null; then
    error "Rust 未安装，请访问 https://rustup.rs/ 安装"
  fi

  if ! command -v java &>/dev/null; then
    error "未安装 Java，shield-stub 构建需要 Java 17+"
  fi

  if ! command -v npm &>/dev/null; then
    error "未安装 Node.js/npm，Tauri React 前端构建需要 npm"
  fi

  if [[ "$BUILD_UNIVERSAL" == "universal" ]]; then
    for target in aarch64-apple-darwin x86_64-apple-darwin; do
      if ! rustup target list --installed | grep -q "$target"; then
        warn "未安装 $target，正在安装..."
        rustup target add "$target"
      fi
    done
  fi

  success "环境检查通过"
  echo "  Rust: $(rustc --version)"
  echo "  架构: $(uname -m)$([ "$BUILD_UNIVERSAL" = "universal" ] && echo " → universal" || echo "")"
  echo "  版本: $VERSION"
}

# ========== 构建 shield-stub（resources.zip）==========
build_stub() {
  info "构建 shield-stub（resources.zip）..."
  cd "$ROOT"
  bash scripts/build-stub.sh
  RESOURCES_ZIP="$ROOT/shield-stub/build/outputs/resources/resources.zip"
  if [[ ! -f "$RESOURCES_ZIP" ]]; then
    error "shield-stub 构建失败，resources.zip 未生成"
  fi
  success "shield-stub 构建完成"
}

# ========== 构建 CLI ==========
build_cli() {
  info "构建 CLI..."
  cd "$ROOT"

  if [[ "$BUILD_UNIVERSAL" == "universal" ]]; then
    cargo build --release -p shield-cli --target aarch64-apple-darwin
    cargo build --release -p shield-cli --target x86_64-apple-darwin

    mkdir -p "$ROOT/target/universal/release"
    lipo -create \
      "$ROOT/target/aarch64-apple-darwin/release/shield" \
      "$ROOT/target/x86_64-apple-darwin/release/shield" \
      -output "$ROOT/target/universal/release/shield"
    CLI_BIN="$ROOT/target/universal/release/shield"
    success "CLI universal binary 构建完成"
  else
    cargo build --release -p shield-cli
    CLI_BIN="$ROOT/target/release/shield"
    success "CLI 构建完成（$(uname -m)）"
  fi
  echo "  大小: $(du -h "$CLI_BIN" | cut -f1)"
}

# ========== 构建 GUI（Tauri bundle：.app + .dmg）==========
build_gui() {
  info "构建 GUI（cargo tauri build --bundles app,dmg）..."
  cd "$ROOT/apps/shield-gui"

  npm ci
  if [[ "$BUILD_UNIVERSAL" == "universal" ]]; then
    cargo tauri build --bundles app,dmg --target universal-apple-darwin
  else
    cargo tauri build --bundles app,dmg
  fi

  success "GUI 构建完成"
}

# ========== 准备输出目录 ==========
prepare_dirs() {
  info "准备输出目录..."
  rm -rf "$DIST_DIR"
  mkdir -p "$DIST_DIR"/{cli,gui-app,gui-dmg}
  success "目录准备完成: $DIST_DIR"
}

# ========== 打包 CLI tar.gz ==========
package_cli() {
  info "打包 CLI..."

  local ARCH_LABEL
  if [[ "$BUILD_UNIVERSAL" == "universal" ]]; then
    ARCH_LABEL="universal"
  else
    ARCH_LABEL="$(uname -m)"
  fi

  local CLI_PKG_DIR="$DIST_DIR/cli/mocika-shield-cli-${VERSION}-macos-${ARCH_LABEL}"
  mkdir -p "$CLI_PKG_DIR"/{bin,lib,resources}

  cp "$CLI_BIN" "$CLI_PKG_DIR/bin/shield"
  chmod +x "$CLI_PKG_DIR/bin/shield"

  if [[ -f "$ROOT/tools/apktool_3.0.1.jar" ]]; then
    cp "$ROOT/tools/apktool_3.0.1.jar" "$CLI_PKG_DIR/lib/apktool.jar"
  else
    warn "apktool.jar 未找到，请手动复制到 tools/ 目录"
  fi
  if [[ -f "$ROOT/tools/apksigner.jar" ]]; then
    cp "$ROOT/tools/apksigner.jar" "$CLI_PKG_DIR/lib/apksigner.jar"
  else
    warn "apksigner.jar 未找到，请手动复制到 tools/ 目录"
  fi

  cp "$ROOT/shield-stub/build/outputs/resources/resources.zip" "$CLI_PKG_DIR/resources/resources.zip"

  cat > "$CLI_PKG_DIR/shield.sh" << 'RUNEOF'
#!/usr/bin/env bash
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$DIR/bin/shield" "$@"
RUNEOF
  chmod +x "$CLI_PKG_DIR/shield.sh"

  cat > "$CLI_PKG_DIR/README.md" << READMEEOF
# Mocika Shield CLI v${VERSION}

macOS ${ARCH_LABEL} 版本。

## 使用方法

\`\`\`bash
./bin/shield protect -i input.apk -o protected.apk
\`\`\`

## 要求

- macOS 10.13+
- Java 17+（需完整 JDK，`java` / `javac` / `keytool` 需可用）

## 目录结构

\`\`\`
mocika-shield-cli-${VERSION}-macos-${ARCH_LABEL}/
├── bin/shield          # 可执行文件
├── lib/
│   ├── apktool.jar
│   └── apksigner.jar
├── resources/
│   └── resources.zip   # Android 壳资源
├── shield.sh           # 快捷启动脚本
└── README.md
\`\`\`
READMEEOF

  cd "$DIST_DIR/cli"
  tar -czf "mocika-shield-cli-${VERSION}-macos-${ARCH_LABEL}.tar.gz" \
      "mocika-shield-cli-${VERSION}-macos-${ARCH_LABEL}"
  rm -rf "mocika-shield-cli-${VERSION}-macos-${ARCH_LABEL}"

  success "CLI tar.gz 打包完成"
  ls -lh "$DIST_DIR/cli/"*.tar.gz
}

# ========== 收集 GUI 产物（Tauri 生成的 .app 和 .dmg）==========
collect_gui() {
  info "收集 GUI 产物..."

  local TAURI_TARGET="$ROOT/target"
  local BUNDLE_BASE

  if [[ "$BUILD_UNIVERSAL" == "universal" ]]; then
    BUNDLE_BASE="$TAURI_TARGET/universal-apple-darwin/release/bundle"
  else
    BUNDLE_BASE="$TAURI_TARGET/release/bundle"
  fi

  # .app
  local APP_DIR
  APP_DIR=$(find "$BUNDLE_BASE/macos" -name "*.app" -maxdepth 1 2>/dev/null | head -n 1)
  if [[ -n "$APP_DIR" ]]; then
    cp -r "$APP_DIR" "$DIST_DIR/gui-app/"
    success ".app: $(basename "$APP_DIR")"
  else
    warn ".app 未找到: $BUNDLE_BASE/macos/"
  fi

  # .dmg（重命名为规范格式）
  local DMG
  DMG=$(find "$BUNDLE_BASE/dmg" -name "*.dmg" 2>/dev/null | head -n 1)
  if [[ -n "$DMG" ]]; then
    local ARCH_SUFFIX
    if [[ "$BUILD_UNIVERSAL" == "universal" ]]; then
      ARCH_SUFFIX="universal"
    else
      ARCH_SUFFIX="$(uname -m)"
    fi
    local DMG_OUT="MocikaShield_${VERSION}_macos_${ARCH_SUFFIX}.dmg"
    cp "$DMG" "$DIST_DIR/gui-dmg/$DMG_OUT"
    success ".dmg → $DMG_OUT"
  else
    warn ".dmg 未找到: $BUNDLE_BASE/dmg/"
  fi
}

# ========== 生成校验和 ==========
generate_checksums() {
  info "生成 SHA256 校验和..."
  cd "$DIST_DIR"
  find . -type f \( -name "*.tar.gz" -o -name "*.dmg" \) \
    -exec shasum -a 256 {} \; | sort > checksums-sha256.txt
  success "校验和已写入 checksums-sha256.txt"
  cat checksums-sha256.txt
}

# ========== 显示结果 ==========
show_results() {
  local ARCH_LABEL
  if [[ "$BUILD_UNIVERSAL" == "universal" ]]; then
    ARCH_LABEL="universal (ARM64 + x86_64)"
  else
    ARCH_LABEL="$(uname -m)"
  fi

  echo ""
  info "macOS 发布构建完成！v${VERSION}  架构: ${ARCH_LABEL}"
  echo ""
  find "$DIST_DIR" -type f | sort | while read -r f; do
    local size
    size=$(du -h "$f" | cut -f1)
    echo "  📄 ${f#"$DIST_DIR"/} ($size)"
  done
  echo ""
  echo "📦 CLI（tar.gz）:"
  ls "$DIST_DIR/cli/"*.tar.gz 2>/dev/null || echo "   (未生成)"
  echo ""
  echo "📦 GUI .app（直接运行）:"
  ls "$DIST_DIR/gui-app/"*.app 2>/dev/null || echo "   (未生成)"
  echo ""
  echo "📦 GUI .dmg（拖拽安装）:"
  ls "$DIST_DIR/gui-dmg/"*.dmg 2>/dev/null || echo "   (未生成)"
  echo ""
  warn "注意：使用 adhoc 签名，首次打开需在「系统设置 → 隐私与安全性」中允许"
  success "全部完成！"
}

# ========== 主流程 ==========
main() {
  echo ""
  echo "  Mocika Shield — macOS 发布脚本（必须在 macOS 上原生运行）"
  echo "  版本: $VERSION"
  echo ""

  cd "$ROOT"

  check_env
  build_stub
  prepare_dirs
  build_cli
  build_gui
  package_cli
  collect_gui
  generate_checksums
  show_results
}

main "$@"
