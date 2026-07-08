#!/bin/bash
# Mocika Shield 发布打包脚本
# 自动构建并打包完整的发布版本

set -e

# 脚本位于 scripts/，PROJECT_ROOT 为上一级
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RELEASE_DIR="$PROJECT_ROOT/release"
DIST_DIR="$PROJECT_ROOT/dist"
VERSION="${1:-1.0.0}"
RELEASE_NAME="mocika-shield-$VERSION"

echo "========================================="
echo "Mocika Shield Release Builder"
echo "版本: $VERSION"
echo "========================================="

# 1. 清理旧的构建
echo "=> 清理旧的构建..."
rm -rf "$RELEASE_DIR"
mkdir -p "$RELEASE_DIR"/{bin,lib,resources}
mkdir -p "$DIST_DIR"

# 2. 构建 shield-stub
echo "=> 构建 shield-stub..."
SHIELD_VERSION="$VERSION" "$PROJECT_ROOT/scripts/build-stub.sh"
echo "✓ shield-stub 构建完成"

# 3. 编译 CLI
echo "=> 编译 CLI..."
cd "$PROJECT_ROOT"
cargo build --release -p shield-cli
echo "✓ CLI 编译完成"

# 4. 复制shield可执行文件
echo "=> 复制可执行文件..."
cp "$PROJECT_ROOT/target/release/shield" "$RELEASE_DIR/bin/"
chmod +x "$RELEASE_DIR/bin/shield"
echo "✓ shield -> release/bin/shield"

# 5. 复制工具JAR文件
echo "=> 复制工具..."

# apktool.jar
if [ -f "$PROJECT_ROOT/tools/apktool_3.0.1.jar" ]; then
    cp "$PROJECT_ROOT/tools/apktool_3.0.1.jar" "$RELEASE_DIR/lib/apktool.jar"
    echo "✓ apktool.jar -> release/lib/"
else
    echo "! 错误: apktool.jar 未找到"
    exit 1
fi

# apksigner.jar
if [ -f "$PROJECT_ROOT/tools/apksigner.jar" ]; then
    cp "$PROJECT_ROOT/tools/apksigner.jar" "$RELEASE_DIR/lib/"
    echo "✓ apksigner.jar -> release/lib/"
else
    echo "! 警告: apksigner.jar 未找到"
    echo "  请手动下载到 tools/ 目录"
fi

# 6. 复制 runtime resources
echo "=> 复制 runtime resources..."
RESOURCES_ZIP="$PROJECT_ROOT/shield-stub/build/outputs/resources/resources.zip"

if [ -f "$RESOURCES_ZIP" ]; then
    cp "$RESOURCES_ZIP" "$RELEASE_DIR/resources/"
    echo "✓ resources.zip -> release/resources/"
else
    echo "! 错误: resources.zip 未找到"
    echo "  构建失败，请检查 scripts/build-stub.sh 输出"
    exit 1
fi

# 7. 生成README
echo "=> 生成README..."
cat > "$RELEASE_DIR/README.md" << 'EOF'
# Mocika Shield - 发布版本

## 目录结构

```
mocika-shield-x.y.z/
├── bin/
│   └── shield          # 可执行文件
├── lib/
│   ├── apktool.jar     # APK反编译工具
│   └── apksigner.jar   # APK签名工具
├── resources/
│   └── resources.zip   # shield-stub 产物（壳DEX + Native库）
└── README.md
```

## 快速开始

### 环境要求

- Java 17+（需完整 JDK，`java` / `javac` / `keytool` 在 PATH）
- Linux / macOS

### 加固 APK

```bash
./bin/shield protect -i input.apk -o protected.apk
```

加固后需重新签名：

```bash
apksigner sign --ks keystore.jks protected.apk
```

### 查看帮助

```bash
./bin/shield --help
./bin/shield --version
```
EOF
echo "✓ README.md已生成"

# 8. 显示文件大小
echo ""
echo "========================================="
echo "发布包内容:"
echo "========================================="
ls -lh "$RELEASE_DIR/bin/"
ls -lh "$RELEASE_DIR/lib/" 2>/dev/null || echo "lib/目录为空"
ls -lh "$RELEASE_DIR/resources/"
echo ""

# 9. 创建压缩包
echo "=> 创建发布压缩包..."
cd "$RELEASE_DIR"
tar -czf "$DIST_DIR/$RELEASE_NAME.tar.gz" .
echo "✓ 压缩包已生成: dist/$RELEASE_NAME.tar.gz"

# 10. 计算SHA256
echo "=> 生成校验和..."
cd "$DIST_DIR"
sha256sum "$RELEASE_NAME.tar.gz" > "$RELEASE_NAME.tar.gz.sha256"
echo "✓ SHA256: $(cat "$RELEASE_NAME.tar.gz.sha256")"

# 11. 显示摘要
echo ""
echo "========================================="
echo "✓ 发布包构建完成！"
echo "========================================="
echo "构建目录: $RELEASE_DIR"
echo "发布目录: $DIST_DIR"
echo "压缩包: $DIST_DIR/$RELEASE_NAME.tar.gz"
echo "大小: $(du -h "$DIST_DIR/$RELEASE_NAME.tar.gz" | cut -f1)"
echo ""
echo "部署命令:"
echo "  tar -xzf $RELEASE_NAME.tar.gz -C /opt/mocika-shield"
echo "  export PATH=\$PATH:/opt/mocika-shield/bin"
echo ""
echo "验证校验和:"
echo "  sha256sum -c $RELEASE_NAME.tar.gz.sha256"
echo ""
