#!/usr/bin/env bash
# Linux 平台发布脚本
# 生成：CLI（静态链接 musl tar.gz）、GUI（AppImage + deb）
#
# 用法:
#   ./scripts/release-linux.sh [VERSION]
#   VERSION=1.2.3 ./scripts/release-linux.sh
#
# 前置要求（Ubuntu 22.04 推荐）:
#   rustup target add x86_64-unknown-linux-musl
#   sudo apt install musl-tools libwebkit2gtk-4.1-dev libgtk-3-dev \
#     libayatana-appindicator3-dev libssl-dev
#   cargo install cargo-deb
#
# 注意: shield-stub 的 resources.zip 将在构建过程中实时生成
#       首次运行需要 Android NDK 29.0.14206865 和 cargo-ndk

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/.."
VERSION="${VERSION:-${1:-1.0.0}}"
ARCH="x86_64"
DIST_DIR="$ROOT/dist/linux"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

info()    { echo -e "${BLUE}==> $1${NC}"; }
success() { echo -e "${GREEN}✓ $1${NC}"; }
warn()    { echo -e "${YELLOW}⚠ $1${NC}"; }
error()   { echo -e "${RED}✗ $1${NC}" >&2; exit 1; }

# ========== 检查依赖 ==========
check_deps() {
  info "检查构建依赖..."

  # musl 目标（CLI 静态链接用）
  if ! rustup target list --installed | grep -q "x86_64-unknown-linux-musl"; then
    warn "未安装 musl 目标，正在安装..."
    rustup target add x86_64-unknown-linux-musl
  fi

  # musl-gcc
  if ! command -v musl-gcc &>/dev/null; then
    error "未安装 musl-tools，请运行: sudo apt install musl-tools"
  fi

  # cargo-deb（deb 打包）
  if ! command -v cargo-deb &>/dev/null; then
    warn "未安装 cargo-deb，正在安装..."
    cargo install cargo-deb
  fi

  # Java（shield-stub 构建需要）
  if ! command -v java &>/dev/null; then
    error "未安装 Java，shield-stub 构建需要 Java 17+"
  fi

  if ! command -v npm &>/dev/null; then
    error "未安装 Node.js/npm，Tauri React 前端构建需要 npm"
  fi

  success "依赖检查完成"
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

# ========== 构建 CLI（静态链接 musl）==========
build_cli() {
  info "构建 CLI（x86_64-unknown-linux-musl 静态链接）..."
  cd "$ROOT"
  cargo build --release \
    --manifest-path shield-cli/Cargo.toml \
    --target x86_64-unknown-linux-musl

  CLI_BIN="$ROOT/target/x86_64-unknown-linux-musl/release/shield"
  if [[ ! -f "$CLI_BIN" ]]; then
    error "CLI 构建失败，产物不存在"
  fi

  local ldd_out
  ldd_out=$(ldd "$CLI_BIN" 2>&1)
  if echo "$ldd_out" | grep -qE "statically linked|not a dynamic executable"; then
    success "CLI 构建完成（完全静态链接）"
  else
    warn "CLI 构建完成（包含动态链接，musl 模式可能未生效）"
    echo "$ldd_out" | head -5
  fi
}

# ========== 构建 GUI（Tauri bundle：AppImage + deb）==========
build_gui() {
  info "构建 GUI（cargo tauri build --bundles appimage,deb）..."
  cd "$ROOT/shield-gui"

  npm ci
  cargo tauri build --bundles appimage,deb

  success "GUI 构建完成"
}

# ========== 准备输出目录 ==========
prepare_dirs() {
  info "准备输出目录..."
  rm -rf "$DIST_DIR"
  mkdir -p "$DIST_DIR"/{cli,gui-appimage,gui-deb}
  success "目录准备完成: $DIST_DIR"
}

# ========== 打包 CLI tar.gz ==========
package_cli() {
  info "打包 CLI..."

  local CLI_PKG_DIR="$DIST_DIR/cli/mocika-shield-cli-${VERSION}-linux-${ARCH}"
  mkdir -p "$CLI_PKG_DIR"/{bin,lib,resources}

  # 可执行文件
  cp "$ROOT/target/x86_64-unknown-linux-musl/release/shield" "$CLI_PKG_DIR/bin/shield"
  chmod +x "$CLI_PKG_DIR/bin/shield"

  # 工具 JAR
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

  # resources.zip
  cp "$ROOT/shield-stub/build/outputs/resources/resources.zip" "$CLI_PKG_DIR/resources/resources.zip"

  # 启动脚本
  cat > "$CLI_PKG_DIR/shield.sh" << 'RUNEOF'
#!/usr/bin/env bash
# Mocika Shield CLI 启动脚本
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$DIR/bin/shield" "$@"
RUNEOF
  chmod +x "$CLI_PKG_DIR/shield.sh"

  # README
  cat > "$CLI_PKG_DIR/README.md" << READMEEOF
# Mocika Shield CLI v${VERSION}

Linux x86_64 静态链接版本，无需安装依赖。

## 使用方法

\`\`\`bash
./bin/shield protect -i input.apk -o protected.apk
\`\`\`

## 要求

- Linux x86_64
- Java 17+（需完整 JDK，`java` / `javac` / `keytool` 需可用）

## 目录结构

\`\`\`
mocika-shield-cli-${VERSION}-linux-x86_64/
├── bin/shield          # 可执行文件（静态链接）
├── lib/
│   ├── apktool.jar
│   └── apksigner.jar
├── resources/
│   └── resources.zip   # Android 壳资源（stub DEX + Native .so）
└── README.md
\`\`\`
READMEEOF

  # 打包
  cd "$DIST_DIR/cli"
  tar -czf "mocika-shield-cli-${VERSION}-linux-${ARCH}.tar.gz" \
      "mocika-shield-cli-${VERSION}-linux-${ARCH}"
  rm -rf "mocika-shield-cli-${VERSION}-linux-${ARCH}"

  success "CLI tar.gz 打包完成"
  ls -lh "$DIST_DIR/cli/"*.tar.gz
}

# ========== 收集 GUI 产物 ==========
collect_gui() {
  info "收集 GUI 产物..."

  local TAURI_TARGET="$ROOT/shield-gui/src-tauri/target/release/bundle"

  # AppImage（重命名为规范格式）
  local APPIMAGE
  APPIMAGE=$(find "$TAURI_TARGET/appimage" -name "*.AppImage" 2>/dev/null | head -n 1)
  if [[ -n "$APPIMAGE" ]]; then
    local APPIMAGE_OUT="MocikaShield_${VERSION}_linux_amd64.AppImage"
    cp "$APPIMAGE" "$DIST_DIR/gui-appimage/$APPIMAGE_OUT"
    success "AppImage → $APPIMAGE_OUT"
  else
    warn "AppImage 未找到，可能需要在 Ubuntu 22.04 上构建"
  fi

  # deb（重命名为规范格式）
  local DEB
  DEB=$(find "$TAURI_TARGET/deb" -name "*.deb" 2>/dev/null | head -n 1)
  if [[ -n "$DEB" ]]; then
    local DEB_OUT="MocikaShield_${VERSION}_linux_amd64.deb"
    cp "$DEB" "$DIST_DIR/gui-deb/$DEB_OUT"
    success "deb → $DEB_OUT"
  else
    warn "deb 安装包未找到"
  fi
}

# ========== 生成校验和 ==========
generate_checksums() {
  info "生成 SHA256 校验和..."
  cd "$DIST_DIR"
  find . -type f \( -name "*.tar.gz" -o -name "*.AppImage" -o -name "*.deb" \) \
    -exec sha256sum {} \; | sort > checksums-sha256.txt
  success "校验和已写入 checksums-sha256.txt"
  cat checksums-sha256.txt
}

# ========== 显示结果 ==========
show_results() {
  echo ""
  info "Linux 发布构建完成！v${VERSION}"
  echo ""
  echo "📁 $DIST_DIR/"
  find "$DIST_DIR" -type f | sort | while read -r f; do
    local size
    size=$(du -h "$f" | cut -f1)
    echo "  📄 ${f#"$DIST_DIR"/} ($size)"
  done
  echo ""
  echo "📦 CLI（静态链接 tar.gz，跨发行版无依赖）:"
  ls "$DIST_DIR/cli/"*.tar.gz 2>/dev/null || echo "   (未生成)"
  echo ""
  echo "📦 GUI AppImage（单文件，点击即用）:"
  ls "$DIST_DIR/gui-appimage/"*.AppImage 2>/dev/null || echo "   (未生成)"
  echo ""
  echo "📦 GUI deb 安装包（Ubuntu / Debian）:"
  ls "$DIST_DIR/gui-deb/"*.deb 2>/dev/null || echo "   (未生成)"
  echo ""
  success "全部完成！"
}

# ========== 主流程 ==========
main() {
  echo ""
  echo "  Mocika Shield — Linux 发布脚本"
  echo "  版本: $VERSION  架构: $ARCH"
  echo ""

  cd "$ROOT"

  check_deps
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
