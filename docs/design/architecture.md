# 架构详解

> 完整技术内参见 [internals.md](internals.md)，本文聚焦目录结构和工具路径逻辑。

## 目录结构

```
mocika-shield/
├── Cargo.toml                        # Rust workspace 根（shield-core + apps/shield-cli + apps/shield-gui/src-tauri + shield-stub/rust）
├── Cargo.lock
├── Makefile                          # 统一构建入口
│
├── crates/
│   └── shield-core/                  # Rust 共享核心库
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs                # 公共 API：protect_apk / sign_apk / ProgressEvent 等
│           ├── error.rs              # 错误类型（ShieldError）
│           ├── utils.rs              # 工具函数：exe_dir、strip_unc_prefix（dunce）、find_apktool 等
│           ├── protect_api.rs        # 加固流程编排、进度事件、取消
│           ├── signing.rs            # APK 签名（apksigner）
│           ├── protect/
│           │   ├── mod.rs
│           │   ├── manifest.rs       # AndroidManifest 修改与 Application 注入
│           │   ├── dex.rs            # DEX 收集、打包、header 修复
│           │   ├── runtime.rs        # runtime 资源注入、ABI 收集、metadata 读取
│           │   └── signature.rs      # 原 APK 签名提取
│           └── dex_packer/           # DEX 打包模块（Zstd 压缩 + ChaCha20-Poly1305 + HKDF）
│               ├── crypto.rs         # 加解密：derive_key / encrypt / decrypt
│               ├── packer.rs         # 打包入口：DexPacker，输出 DEXB v5 格式
│               └── mod.rs
│
├── apps/
│   ├── shield-cli/                   # Rust 命令行工具（单一二进制 shield）
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                # 兼容层：对外 re-export shield-core
│   │       └── main.rs               # CLI 入口（clap 参数解析，-i/-o/-v）
│
├── shield-stub/                      # Android 壳模块（独立 Gradle 项目）
│   ├── settings.gradle.kts
│   ├── build.gradle.kts              # AGP 8.x，NDK 硬编码 29.0.14206865
│   ├── gradle/                       # Gradle Wrapper + Version Catalog
│   ├── gradlew / gradlew.bat
│   ├── gradle.properties
│   └── src/main/
│       ├── java/dev/mocika/shield/loader/
│       │   ├── StubApp.java          # 壳 Application（attachBaseContext / onCreate）
│       │   ├── Ld.java               # DEX 提取、落地、缓存管理、JNI 接口声明
│       │   └── ARouterCompat.java    # ARouter 路由表补注册
│       └── rust/                     # Rust Native 层（libmocikashield.so）
│           ├── Cargo.toml
│           ├── .cargo/config.toml    # target-dir 指向 ../../../../build/rust-target
│           ├── build.sh              # cargo-ndk 交叉编译脚本（Gradle buildRustLibs 调用）
│           └── src/
│               ├── lib.rs            # JNI 入口：extractAndDecryptFromDex / nativeInjectDex
│               ├── bin_loader.rs     # MSHD 扫描、DEXB v5 解析、Zstd 解压
│               └── crypto.rs        # derive_key（HKDF-SHA256）/ decrypt（ChaCha20-Poly1305）
│
│   └── shield-gui/                   # 桌面 GUI（Tauri v2 + React，三平台）
│       ├── package.json              # React 前端依赖与 npm 脚本
│       ├── vite.config.ts            # Vite 构建配置
│       ├── tailwind.config.ts        # Tailwind 主题 token
│       ├── index.html                # React 入口 HTML
│       ├── src-tauri/                # Tauri 后端（Rust）
│       │   ├── Cargo.toml
│       │   ├── build.rs
│       │   ├── tauri.conf.json       # Tauri 配置（窗口、bundle、资源嵌入）
│       │   └── src/
│       │       ├── main.rs           # Tauri 启动入口、state 注入、command 注册
│       │       ├── app_config.rs     # config.toml 读写、应用级配置内存状态
│       │       ├── app_paths.rs      # apktool/resources/apksigner 路径查找
│       │       ├── apk_check.rs      # APK 预检、签名检测、证书指纹比对
│       │       ├── cert_store.rs     # shield.db 初始化、schema 与迁移
│       │       ├── cert_service.rs   # 证书增删改查、默认项、校验、创建/导入
│       │       ├── signing.rs        # APK 签名、keystore alias 解析
│       │       ├── protect_runner.rs # 加固任务桥接、取消、进度事件
│       │       ├── updates.rs        # GitHub Releases 更新检查与缓存
│       │       ├── file_ops.rs       # 打开目录、删除文件、URL 打开
│       │       └── build_info.rs     # 版本与构建工具信息
│       └── src/                      # React + TypeScript 前端
│           ├── main.tsx              # React 入口
│           ├── App.tsx               # 应用壳：侧边栏、页面切换、全局配置状态
│           ├── pages/
│           │   ├── protect-page.tsx      # 加固页
│           │   ├── sign-page.tsx         # 签名页
│           │   ├── certificates-page.tsx # 证书管理页
│           │   ├── settings-page.tsx     # 设置页
│           │   └── about-page.tsx        # 关于页
│           ├── components/app/
│           │   ├── branding.ts           # Logo、步骤文案
│           │   ├── common.tsx            # 应用级共享控件
│           │   ├── about-info-card.tsx   # 关于页信息卡
│           │   └── protect-progress-panel.tsx # 加固页进度侧栏
│           ├── styles.css                # Tailwind 与主题变量
│           ├── components/ui/            # Sidebar、Switch、Input 等基础组件
│           ├── hooks/
│           │   ├── use-app-config.ts     # 配置加载、更新检查
│           │   ├── use-applied-theme-mode.ts # 主题应用
│           │   ├── use-about-page.ts     # 关于页数据加载与更新检查
│           │   ├── use-clipboard.ts      # 复制反馈
│           │   ├── use-protect-workflow.ts # 加固页工作流状态与事件监听
│           │   ├── use-sign-workflow.ts  # 签名页工作流状态与拖拽处理
│           │   ├── use-certificates.ts   # 证书列表、保存、校验、默认项
│           │   └── use-mobile.tsx        # 响应式辅助 hook
│           └── lib/
│               ├── i18n.ts               # 中英双语
│               ├── path.ts               # 跨平台输出路径生成
│               ├── tauri.ts              # Tauri invoke 与事件封装
│               └── utils.ts              # className 合并工具
│
├── tools/                            # 外部工具 JAR（开发环境）
│   ├── apktool_3.0.1.jar
│   └── apksigner.jar
│
└── scripts/                          # 构建与发布脚本
    ├── build-stub.sh                 # Linux/macOS：shield-stub 构建脚本
    ├── build-stub.ps1                # Windows：shield-stub 构建脚本
    ├── setup-windows-dev.ps1         # Windows 开发环境一键配置（Scoop）
    ├── release-cli.sh                # CLI 发布打包（Linux/macOS）
    ├── release-linux.sh              # Linux 本地发布（默认 GUI + CLI；CI 只上传 GUI）
    ├── release-macos.sh              # macOS 本地发布（默认 GUI + CLI；CI 只上传 GUI）
    ├── release-windows.ps1           # Windows 本地发布（默认 GUI + CLI；CI 只上传 GUI）
    └── tools/                        # 下载的构建工具（不进版本库，见 .gitignore）
        └── appimagetool-x86_64.AppImage
```

---

## 工具路径自动检测逻辑

`shield-core` 的 `find_apktool()` / `find_apksigner()` / `find_runtime_resources()` 按以下优先级查找：

1. **发布包路径**：`bin/../lib/apktool.jar`、`bin/../resources/resources.zip`
2. **用户数据目录**（`directories::ProjectDirs`）：`~/.local/share/mocika-shield/`（Linux）等
3. **系统数据目录**：`/usr/local/share/mocika-shield/`（Linux）、`/Library/Application Support/mocika-shield/`（macOS）等
4. **开发环境路径**：`tools/apktool_3.0.1.jar`、`shield-stub/build/outputs/resources/resources.zip`

`shield-gui`（Tauri 版）的查找优先级：

1. **AppImage 运行时**：`$APPDIR/usr/lib/mocika-shield/tools/`
2. **Tauri `resource_dir()`**：由 Tauri 打包时嵌入的资源目录
3. **开发环境路径**：同 CLI

所有路径在 Windows 下通过 `dunce::simplified()` 自动去除 UNC 前缀（`\\?\C:\...` → `C:\...`）。

项目根通过 `exe_dir()` / `dev_project_root()` 从 CWD 或可执行文件所在目录向上逐层查找，直到找到同时含有 `shield-stub/` 和 `apps/` 的目录。

---

## GUI 本地数据目录

GUI 本地数据建议拆分为三类：

| 文件/目录 | 作用 |
|-----------|------|
| `config.toml` | 主题、语言、更新检查等应用级配置 |
| `shield.db` | 证书列表、默认证书、加密后的签名密码、校验状态 |
| `keystores/` | 应用内新建并托管的 keystore 文件 |

证书存储约束：

- `managed`：应用内创建或用户选择“导入并托管”的 keystore，文件落在 `keystores/`
- `external`：仅记录用户原始路径，不复制文件
- 创建证书时 Keystore 密码至少 6 位；Key 密码可留空，填写时同样至少 6 位
- PKCS12 Alias 可能被 `keytool` 规范为小写，后端校验按大小写不敏感匹配，并保存 keystore 实际返回的 Alias
- 密码字段使用本机派生密钥加密落盘，格式为 `enc:v1:<nonce>:<ciphertext>`；不兼容旧明文记录
- Tauri 前端只持有证书元数据，签名、自动签名、证书指纹比对通过证书 ID 交给后端完成

## 构建产物路径

| 产物 | 路径 |
|------|------|
| `shield` 二进制（Linux/macOS） | `target/release/shield` |
| `shield.exe`（Windows） | `target\release\shield.exe` |
| 版本化壳资源包 | `shield-stub/build/outputs/resources/mocika-runtime-resources-x.y.z.zip` |
| `resources.zip` 兼容入口 | `shield-stub/build/outputs/resources/resources.zip` |
| Tauri GUI AppImage | `target/release/bundle/appimage/` |
| Tauri GUI deb | `target/release/bundle/deb/` |
| Tauri GUI dmg | `target/release/bundle/dmg/` |
| Tauri GUI NSIS 安装包 | `target/release/bundle/nsis/` |
| CLI 本地发布压缩包 | `dist/*/cli/mocika-shield-cli-x.y.z-*` |

---

## release profile 配置

两个 Rust 子工程均使用相同的 release 优化配置（体积优先）：

```toml
[profile.release]
opt-level = "z"
lto = true
strip = true
panic = "abort"
codegen-units = 1
```

---

## shield-gui（Tauri 版）与 shield-core 的集成方式

Tauri GUI **不**通过子进程调用 `shield` 二进制，直接链接 `shield-core` 库：

```
shield-core 暴露：
  protect_apk(opts, on_progress: impl Fn(ProgressEvent), cancel: Arc<AtomicBool>)
  sign_apk(opts) -> Result<(), ShieldError>

Tauri 后端（main.rs + 模块）：
  #[tauri::command] protect_apk
    → tokio::task::spawn_blocking
    → protect_runner::execute_protect_apk()
    → window.emit("protect-progress", payload) 推送进度到前端

  #[tauri::command] sign_apk / check_apk / check_update
    → main.rs 只做参数接线
    → 具体实现分别委托给 signing.rs / apk_check.rs / updates.rs

React 前端：
  listen("protect-progress") → 更新分步进度条
  listen("protect-done" / "protect-error") → 完成/失败状态
```

子进程调用（`java`、`keytool` 等）在 Windows 上统一复用 `shield_core::utils::no_window_command()`，设置 `CREATE_NO_WINDOW` flag 避免弹出控制台窗口。
