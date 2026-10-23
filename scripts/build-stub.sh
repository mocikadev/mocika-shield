#!/bin/bash

# Mocika Shield - Runtime库构建脚本
# 用途：生成stub-classes.dex和Native库资源包

set -e

# 脚本位于 scripts/，PROJECT_ROOT 为上一级
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${GREEN}================================================${NC}"
echo -e "${GREEN}Mocika Shield - shield-stub 构建${NC}"
echo -e "${GREEN}================================================${NC}"
echo ""

# 切换到项目根目录
cd "$PROJECT_ROOT"

STANDARD_RUNTIME_PROTOCOL="$(python3 scripts/manifest_contract.py standard)"
MEMORY_RUNTIME_PROTOCOL="$(python3 scripts/manifest_contract.py memory)"

# 检查是否在项目根目录
if [ ! -f "Cargo.toml" ] || [ ! -d "shield-stub" ]; then
    echo -e "${RED}错误: 请在项目根目录运行此脚本${NC}"
    exit 1
fi

# 检查必要工具
echo -e "${BLUE}检查必要工具...${NC}"

if ! command -v java &> /dev/null; then
    echo -e "${RED}错误: 未找到Java，请安装JDK 17+${NC}"
    exit 1
fi

if [ ! -f "shield-stub/gradlew" ]; then
    echo -e "${RED}错误: 未找到 shield-stub/gradlew${NC}"
    exit 1
fi

# 检查 Android SDK
if [ -z "$ANDROID_HOME" ] && [ -z "$ANDROID_SDK_ROOT" ]; then
    echo -e "${RED}错误: 未设置ANDROID_HOME或ANDROID_SDK_ROOT环境变量${NC}"
    echo -e "${YELLOW}请安装Android SDK并设置环境变量${NC}"
    exit 1
fi

ANDROID_SDK="${ANDROID_HOME:-$ANDROID_SDK_ROOT}"
echo -e "${GREEN}✓ Android SDK: $ANDROID_SDK${NC}"

# 检查 Android NDK 29.0.14206865（build.gradle.kts 硬编码版本）
NDK_VERSION="29.0.14206865"
PINNED_NDK_PATH="$ANDROID_SDK/ndk/$NDK_VERSION"
if [ -d "$PINNED_NDK_PATH" ]; then
    ANDROID_NDK_PATH="$PINNED_NDK_PATH"
else
    ANDROID_NDK_PATH="${ANDROID_NDK_ROOT:-${NDK_HOME:-$PINNED_NDK_PATH}}"
fi
if [ ! -d "$ANDROID_NDK_PATH" ]; then
    echo -e "${RED}错误: 未找到 Android NDK $NDK_VERSION${NC}"
    echo -e "${YELLOW}  期望路径: $ANDROID_NDK_PATH${NC}"
    echo -e "${YELLOW}  请通过 Android Studio SDK Manager 安装 NDK $NDK_VERSION${NC}"
    echo -e "${YELLOW}  或设置环境变量: export ANDROID_NDK_ROOT=/path/to/ndk/$NDK_VERSION${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Android NDK: $ANDROID_NDK_PATH${NC}"

# 检查 Rust 工具链（rustup + cargo）
if ! command -v rustup &> /dev/null; then
    echo -e "${RED}错误: 未找到 rustup，请先安装 Rust 工具链${NC}"
    echo -e "${YELLOW}  下载: https://rustup.rs/${NC}"
    exit 1
fi
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}错误: 未找到 cargo，请先安装 Rust 工具链${NC}"
    echo -e "${YELLOW}  下载: https://rustup.rs/${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Rust 工具链: 已安装${NC}"

# 检查 cargo-ndk
if ! command -v cargo-ndk &> /dev/null; then
    echo -e "${RED}错误: 未安装 cargo-ndk${NC}"
    echo -e "${YELLOW}  请运行: cargo install cargo-ndk${NC}"
    exit 1
fi
echo -e "${GREEN}✓ cargo-ndk: $(cargo-ndk --version 2>/dev/null || echo '已安装')${NC}"

# 检查 Android Rust 目标架构
REQUIRED_TARGETS=(
    "aarch64-linux-android"
    "armv7-linux-androideabi"
    "i686-linux-android"
    "x86_64-linux-android"
)
MISSING_TARGETS=()
INSTALLED_TARGETS=$(rustup target list --installed 2>/dev/null)
for t in "${REQUIRED_TARGETS[@]}"; do
    if ! echo "$INSTALLED_TARGETS" | grep -q "^$t"; then
        MISSING_TARGETS+=("$t")
    fi
done
if [ ${#MISSING_TARGETS[@]} -gt 0 ]; then
    echo -e "${YELLOW}⚠ 缺少 Android Rust 目标架构，正在安装...${NC}"
    for t in "${MISSING_TARGETS[@]}"; do
        echo -e "  安装: $t"
        rustup target add "$t"
    done
fi
echo -e "${GREEN}✓ Android Rust targets: 全部就绪${NC}"

# 创建输出目录
OUTPUT_DIR="shield-stub/build/outputs/resources"
mkdir -p "$OUTPUT_DIR"
echo -e "${GREEN}✓ 创建输出目录: $OUTPUT_DIR${NC}"
echo ""

# ==========================================
# 步骤1: 构建Native库（Rust）
# ==========================================
echo -e "${BLUE}步骤1: 构建Rust Native库...${NC}"
echo -e "${YELLOW}架构: arm64-v8a, armeabi-v7a, x86, x86_64${NC}"

# Rust构建通过buildRustLibs任务自动执行，无需单独调用
echo -e "${GREEN}✓ Rust构建将在AAR构建时自动执行${NC}"
echo ""

# ==========================================
# 步骤2: 构建 shield-stub AAR（含 R8 混淆）
# ==========================================
echo -e "${BLUE}步骤2: 构建 shield-stub AAR（含 R8 混淆）...${NC}"

./shield-stub/gradlew --no-daemon -p shield-stub assembleRelease

if [ $? -ne 0 ]; then
    echo -e "${RED}错误: shield-stub AAR构建失败${NC}"
    exit 1
fi

AAR_FILE="shield-stub/build/outputs/aar/shield-stub-release.aar"
if [ ! -f "$AAR_FILE" ]; then
    echo -e "${RED}错误: 未找到AAR文件: $AAR_FILE${NC}"
    exit 1
fi

# R8 mapping 文件路径（Gradle 固定输出位置）
MAPPING_FILE="shield-stub/build/outputs/mapping/release/mapping.txt"
if [ ! -f "$MAPPING_FILE" ]; then
    echo -e "${RED}错误: 未找到 R8 mapping 文件: $MAPPING_FILE${NC}"
    echo -e "${YELLOW}  请确认 proguard-rules.pro 已启用混淆且 R8 正常运行${NC}"
    exit 1
fi

echo -e "${GREEN}✓ AAR构建完成: $AAR_FILE${NC}"
echo -e "${GREEN}✓ R8 mapping: $MAPPING_FILE${NC}"
echo ""

# ==========================================
# 步骤3: 解析 mapping，导出混淆后的类名/方法名
# ==========================================
echo -e "${BLUE}步骤3: 解析 R8 mapping，提取混淆后的类名/方法名...${NC}"

PYTHON_CMD="${PYTHON:-python3}"
BINDINGS_FILE="shield-stub/build/outputs/mapping/release/jni-bindings.env"
"$PYTHON_CMD" scripts/stub_bindings.py env "$MAPPING_FILE" > "$BINDINGS_FILE"
while IFS='=' read -r binding_key binding_value; do
    binding_value="${binding_value%$'\r'}"
    export "$binding_key=$binding_value"
done < "$BINDINGS_FILE"
OBF_STUBAPP_DOTTED="$STUB_APPLICATION"
OBF_STUB_FACTORY="$STUB_COMPONENT_FACTORY"
echo -e "${GREEN}  JNI 绑定已完整解析：$STUB_BINLOADER_CLASS${NC}"
echo ""

# ==========================================
# 步骤4: 使用混淆名重新编译 Rust（生成最终 .so）
# ==========================================
echo -e "${BLUE}步骤4: 使用混淆后的类名/方法名重新编译 Rust Native 库...${NC}"

RUST_SRC="shield-stub/src/main/rust"

export ANDROID_NDK_ROOT="$ANDROID_NDK_PATH"
# 用项目根路径替换编译期嵌入的源码绝对路径，消除 .so 字符串段中的本机路径信息
source "$SCRIPT_DIR/native-build-flags.sh"
configure_native_source_paths "$PROJECT_ROOT"

(
    cd "$RUST_SRC"
    cargo ndk \
        -t arm64-v8a \
        -t armeabi-v7a \
        -t x86 \
        -t x86_64 \
        -o ../../../build/jniLibs \
        build --release 2>&1
)

echo -e "${GREEN}✓ Rust 二次编译完成（.so 字符串已使用混淆名）${NC}"
echo ""

# ==========================================
# 步骤5: 从AAR提取资源，替换为混淆版 .so
# ==========================================
echo -e "${BLUE}步骤5: 提取资源文件...${NC}"

# 创建临时目录
TEMP_DIR=$(mktemp -d)
echo -e "${YELLOW}临时目录: $TEMP_DIR${NC}"

# 解压AAR（classes.jar 已被 R8 混淆）
cd "$TEMP_DIR"
unzip -q "$OLDPWD/$AAR_FILE"
cd "$OLDPWD"

# 提取classes.jar并转换为DEX
echo -e "${YELLOW}提取并转换classes.jar -> stub-classes.dex${NC}"

if [ ! -f "$TEMP_DIR/classes.jar" ]; then
    echo -e "${RED}错误: AAR中未找到classes.jar${NC}"
    rm -rf "$TEMP_DIR"
    exit 1
fi

# 使用d8工具将JAR转换为DEX
D8_TOOL="$ANDROID_SDK/build-tools"
BUILD_TOOLS_VERSION=$(ls "$D8_TOOL" | tr ' ' '\n' | sort -t. -k1,1n -k2,2n -k3,3n | tail -n 1)
D8_CMD="$D8_TOOL/$BUILD_TOOLS_VERSION/d8"

if [ ! -f "$D8_CMD" ]; then
    echo -e "${RED}错误: 未找到d8工具: $D8_CMD${NC}"
    rm -rf "$TEMP_DIR"
    exit 1
fi

echo -e "${YELLOW}使用d8工具: $D8_CMD${NC}"

# 查找 android.jar（取已安装的最高版本 platform）
ANDROID_JAR=""
PLATFORMS_DIR="$ANDROID_SDK/platforms"
if [ -d "$PLATFORMS_DIR" ]; then
    LATEST_PLATFORM=$(ls "$PLATFORMS_DIR" | tr ' ' '\n' | sort -t- -k2,2n | tail -n 1)
    CANDIDATE="$PLATFORMS_DIR/$LATEST_PLATFORM/android.jar"
    if [ -f "$CANDIDATE" ]; then
        ANDROID_JAR="$CANDIDATE"
    fi
fi

if [ -n "$ANDROID_JAR" ]; then
    echo -e "${YELLOW}使用 android.jar: $ANDROID_JAR${NC}"
    "$D8_CMD" --output "$OUTPUT_DIR" "$TEMP_DIR/classes.jar" --min-api 19 --lib "$ANDROID_JAR"
else
    echo -e "${YELLOW}警告: 未找到 android.jar，desugaring 可能产生警告${NC}"
    "$D8_CMD" --output "$OUTPUT_DIR" "$TEMP_DIR/classes.jar" --min-api 19
fi

# 重命名为stub-classes.dex
if [ -f "$OUTPUT_DIR/classes.dex" ]; then
    mv "$OUTPUT_DIR/classes.dex" "$OUTPUT_DIR/stub-classes.dex"
    echo -e "${GREEN}✓ stub-classes.dex生成成功${NC}"
else
    echo -e "${RED}错误: DEX文件生成失败${NC}"
    rm -rf "$TEMP_DIR"
    exit 1
fi

# 使用步骤4重新编译的 .so（字符串已混淆），不用 AAR 里的旧版本
echo -e "${YELLOW}复制混淆版 Native 库...${NC}"

LIB_OUTPUT="$OUTPUT_DIR/lib"
mkdir -p "$LIB_OUTPUT"

JNI_LIBS_DIR="shield-stub/build/jniLibs"
for arch in arm64-v8a armeabi-v7a x86 x86_64; do
    if [ -f "$JNI_LIBS_DIR/$arch/libmocikashield.so" ]; then
        mkdir -p "$LIB_OUTPUT/$arch"
        cp "$JNI_LIBS_DIR/$arch/libmocikashield.so" "$LIB_OUTPUT/$arch/"
        echo -e "${GREEN}  ✓ $arch/libmocikashield.so${NC}"
    else
        echo -e "${YELLOW}  ⚠ $arch: libmocikashield.so未找到${NC}"
    fi
done

# 清理临时目录
rm -rf "$TEMP_DIR"
echo -e "${GREEN}✓ 资源提取完成${NC}"
echo ""

# ==========================================
# 步骤6: 生成metadata.json（含混淆后的类名）
# ==========================================
echo -e "${BLUE}步骤6: 生成metadata.json...${NC}"

BUILD_DATE=$(date +"%Y-%m-%d %H:%M:%S")
BUILD_VERSION="${SHIELD_VERSION:-1.0.0}"

cat > "$OUTPUT_DIR/metadata.json" << EOF
{
  "version": "$BUILD_VERSION",
  "build_date": "$BUILD_DATE",
  "stub_dex": "stub-classes.dex",
  "stub_application": "$OBF_STUBAPP_DOTTED",
  "compression": "Zstd (level 19)",
  "supported_modes": [
    "traditional",
    "bin"
  ],
  "supported_architectures": [
    "arm64-v8a",
    "armeabi-v7a",
    "x86",
    "x86_64"
  ],
  "min_android_api": 21,
  "target_android_api": 35,
  "native_library": "libmocikashield.so",
  "native_name_placeholder": "mocikanativeslot",
  "native_name_length": 16,
  "native_name_scheme": 1,
  "runtime_protocol": $STANDARD_RUNTIME_PROTOCOL,
  "cache_schema": 1,
  "environment_policy": true,
  "memory_dex": false
}
EOF

echo -e "${GREEN}✓ metadata.json生成完成（stub_application: ${OBF_STUBAPP_DOTTED}）${NC}"
echo ""

# ==========================================
# 步骤7: 创建资源包ZIP
# ==========================================
echo -e "${BLUE}步骤7: 创建资源包...${NC}"

cd "$OUTPUT_DIR"
RESOURCES_ZIP="mocika-runtime-resources-${BUILD_VERSION}.zip"

rm -f "$RESOURCES_ZIP" resources.zip
zip -r "$RESOURCES_ZIP" stub-classes.dex lib/ metadata.json \
    -x '*.DS_Store' '__MACOSX/*' > /dev/null

if [ $? -eq 0 ]; then
    RESOURCES_SIZE=$(du -h "$RESOURCES_ZIP" | cut -f1)
    echo -e "${GREEN}✓ 资源包创建成功${NC}"
    echo -e "${GREEN}  文件: $OUTPUT_DIR/$RESOURCES_ZIP${NC}"
    echo -e "${GREEN}  大小: $RESOURCES_SIZE${NC}"
else
    echo -e "${RED}错误: 资源包创建失败${NC}"
    cd "$OLDPWD"
    exit 1
fi

cd "$OLDPWD"

# 候选资源复用同一份已混淆 Stub，只改变受核心严格校验的运行时能力声明。
# 自动资源发现仍只返回 resources.zip；resources-memory.zip 仅供显式内部回归。
cp "$OUTPUT_DIR/metadata.json" "$OUTPUT_DIR/metadata-standard.json"
cat > "$OUTPUT_DIR/metadata.json" << EOF
{
  "version": "$BUILD_VERSION",
  "build_date": "$BUILD_DATE",
  "stub_dex": "stub-classes.dex",
  "stub_application": "$OBF_STUBAPP_DOTTED",
  "stub_component_factory": "$OBF_STUB_FACTORY",
  "supported_architectures": ["arm64-v8a", "armeabi-v7a", "x86", "x86_64"],
  "min_android_api": 28,
  "target_android_api": 35,
  "native_library": "libmocikashield.so",
  "native_name_placeholder": "mocikanativeslot",
  "native_name_length": 16,
  "native_name_scheme": 1,
  "runtime_protocol": $MEMORY_RUNTIME_PROTOCOL,
  "cache_schema": 1,
  "environment_policy": true,
  "memory_dex": true,
  "memory_dex_min_api": 31
}
EOF
cd "$OUTPUT_DIR"
rm -f "resources-memory.zip"
zip -r "resources-memory.zip" stub-classes.dex lib/ metadata.json \
    -x '*.DS_Store' '__MACOSX/*' > /dev/null
cd "$OLDPWD"
mv "$OUTPUT_DIR/metadata-standard.json" "$OUTPUT_DIR/metadata.json"
echo -e "${GREEN}✓ 内存 DEX 候选资源: $OUTPUT_DIR/resources-memory.zip${NC}"

# 在交付资源前校验实际 DEX 方法定义与全部 ELF 的同批绑定摘要。
"$PYTHON_CMD" scripts/stub_bindings.py audit "$MAPPING_FILE" "$OUTPUT_DIR/$RESOURCES_ZIP"
"$PYTHON_CMD" scripts/stub_bindings.py audit "$MAPPING_FILE" "$OUTPUT_DIR/resources-memory.zip"
# 只有全部审计成功后才开放自动发现入口。
ln -sf "mocika-runtime-resources-${BUILD_VERSION}.zip" "$OUTPUT_DIR/resources.zip"

echo ""
echo -e "${GREEN}================================================${NC}"
echo -e "${GREEN}构建完成！${NC}"
echo -e "${GREEN}================================================${NC}"
echo ""
echo -e "${BLUE}资源包位置:${NC}"
echo -e "  $OUTPUT_DIR/$RESOURCES_ZIP"
echo -e "  $OUTPUT_DIR/resources.zip (符号链接)"
echo ""
echo -e "${BLUE}包含内容:${NC}"
echo -e "  ✓ stub-classes.dex (运行时Java类，R8已混淆)"
echo -e "  ✓ lib/*/libmocikashield.so (Native库，字符串已使用混淆名)"
echo -e "  ✓ metadata.json (元数据，含混淆后的壳类名)"
echo ""
echo -e "${YELLOW}下一步:${NC}"
echo -e "  使用 ${GREEN}shield-cli${NC} 加固APK"
  echo -e "  示例: ${GREEN}./target/release/shield protect -i input.apk -o output.apk${NC}"
echo ""
