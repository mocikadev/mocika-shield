VERSION    := $(shell grep '^version' apps/shield-cli/Cargo.toml | head -1 | sed 's/version = "\([^"]*\)"/\1/')
CLI_BIN    := target/release/shield
DIST_DIR   := dist

RESOURCES_ZIP  := shield-stub/build/outputs/resources/resources.zip

ifeq ($(OS),Windows_NT)
  BUILD_STUB_CMD := powershell -ExecutionPolicy Bypass -File scripts/build-stub.ps1
  CLEAN_CMD      := powershell -Command "Remove-Item -Recurse -Force -ErrorAction SilentlyContinue"
else
  BUILD_STUB_CMD := ./scripts/build-stub.sh
  CLEAN_CMD      := rm -rf
endif

.PHONY: build-cli build-stub build-gui build-all \
        release release-linux release-windows \
        release-macos release-macos-universal \
        bump-version test clean help

help:
	@echo "用法: make <目标>"
	@echo ""
	@echo "  build-cli              编译 shield-cli（当前平台 release）"
	@echo "  build-stub             构建 shield-stub（Android AAR + 资源包）"
	@echo "  build-gui              构建 shield-gui Tauri 桌面应用（需先 build-stub）"
	@echo "  build-all              build-stub + build-cli + build-gui（Tauri）"
	@echo "  release-linux          Linux 本地发布包（默认 GUI + CLI，可用 SKIP_CLI_RELEASE=1 跳过 CLI），在 Linux 上运行"
	@echo "  release-windows        Windows 本地发布包（默认 GUI + CLI，可用 SKIP_CLI_RELEASE=1 跳过 CLI），在 Windows 上运行"
	@echo "  release-macos          macOS 本地发布包（默认 GUI + CLI，可用 SKIP_CLI_RELEASE=1 跳过 CLI），在 macOS 上运行"
	@echo "  release-macos-universal  macOS Tauri universal binary 发布包（ARM64 + x86_64）"
	@echo "  release VERSION=x.y.z  CLI-only 发布包（维护者本地使用）"
	@echo "  bump-version V=x.y.z   同步版本号到所有配置文件"
	@echo "  test                   运行 shield-core + shield-cli 单元测试"
	@echo "  clean                  清理所有构建产物"

build-cli:
	@echo "🦀 编译 shield-cli..."
	cargo build --release -p shield-cli
	@echo "✅ 产物: $(CLI_BIN)"

build-stub:
	@echo "🤖 构建 shield-stub..."
	$(BUILD_STUB_CMD)

ifeq ($(OS),Windows_NT)
	set SKIP_STANDARD_STUB_BUILD=1&& bash scripts/build-android-api19-resources.sh
else
	SKIP_STANDARD_STUB_BUILD=1 ./scripts/build-android-api19-resources.sh
endif
	@echo "✅ 产物: shield-stub/build/outputs/resources/resources.zip"
	@echo "✅ 兼容产物: shield-stub/build/outputs/resources/resources-api19.zip"

build-gui:
	@echo "🖥️  构建 shield-gui（Tauri）..."
	cd apps/shield-gui && cargo tauri build --no-bundle
ifeq ($(OS),Windows_NT)
	@echo "✅ 产物: target/release/mocika-shield.exe"
else
	@echo "✅ 产物: target/release/mocika-shield"
endif

build-all: build-stub build-cli build-gui

release-linux:
	@echo "🐧 Linux 发布构建 v$(VERSION)..."
	VERSION=$(VERSION) ./scripts/release-linux.sh
	@echo "✅ 产物: dist/linux/"

release-windows:
	@echo "🪟 Windows 发布构建 v$(VERSION)（须在 Windows 上运行）..."
	powershell -ExecutionPolicy Bypass -File scripts/release-windows.ps1 -Version $(VERSION)
	@echo "✅ 产物: dist/windows/"

release-macos:
	@echo "🍎 macOS 发布构建 v$(VERSION)（须在 macOS 上运行）..."
	VERSION=$(VERSION) ./scripts/release-macos.sh
	@echo "✅ 产物: dist/macos/"

release-macos-universal:
	@echo "🍎 macOS Universal Binary 发布构建 v$(VERSION)..."
	VERSION=$(VERSION) ./scripts/release-macos.sh $(VERSION) universal
	@echo "✅ 产物: dist/macos/"

release: build-all
	@echo "📦 生成 CLI 发布包 v$(VERSION)..."
	./scripts/release-cli.sh $(VERSION)
	@echo "✅ 发布包: $(DIST_DIR)/mocika-shield-$(VERSION).tar.gz"

test:
	@echo "🧪 运行测试..."
	cargo test -p shield-core
	cargo test -p shield-cli

bump-version:
	@if [ -z "$(V)" ]; then echo "用法: make bump-version V=x.y.z"; exit 1; fi
	bash scripts/bump-version.sh $(V)

clean:
	@echo "🧹 清理构建产物..."
	$(CLEAN_CMD) \
		target \
		build \
		dist \
		release \
		apps/shield-cli/target \
		apps/shield-gui/dist \
		apps/shield-gui/src-tauri/gen/schemas/acl-manifests.json \
		shield-stub/build \
		shield-stub/.gradle \
		shield-stub/src/main/rust/target \
		apps/shield-gui/node_modules
	@echo "✅ 清理完成"
