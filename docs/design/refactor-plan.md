# 目录与代码重构方案

> 2026-07-08 更新：阶段一“拆 Tauri 后端”、阶段二“拆前端 App.tsx”、阶段三“拆 CLI 加固主流程”已经落地，`shield-gui/src-tauri/src/main.rs`、`shield-gui/src/App.tsx`、`shield-cli/src/commands/protect.rs` 都已收缩为薄入口；CLI 与 GUI 也已经共用同一份 `no_window_command()`。

> 本文基于当前 `main` 分支实际代码结构整理，目标是降低维护成本、收敛职责边界，并保持现有 CLI / GUI / Android 壳行为稳定。

## 1. 审计结论

当前仓库已经完成开源前的一轮收口，技术路线也比较明确：

- `shield-cli/` 负责 APK 加固、对齐、签名等核心能力
- `shield-stub/` 负责 Android 运行时壳
- `shield-gui/` 是唯一正式 GUI（Tauri v2 + React）
- `docs/` 已基本按 `ops / design / process` 分层

目前最需要重构的不是目录数量，而是**单文件职责堆积**。

### 1.1 高风险文件

| 文件 | 规模 | 当前问题 |
|------|------|----------|
| `shield-gui/src-tauri/src/main.rs` | 1500 行 | Tauri commands、配置加载、路径查找、签名、证书比对、APK 检测、更新检查、文件操作、构建信息全堆在一个文件 |
| `shield-gui/src/App.tsx` | 1499 行 | 加固页、签名页、设置页、关于页、事件监听、状态管理、页面布局全部混在一个组件 |
| `shield-cli/src/commands/protect.rs` | 1003 行 | 主流程编排、Manifest 修改、DEX 处理、运行时注入、证书提取、DEX header 修复、临时 Java 编译、测试全部放在一处 |
| `shield-cli/src/main.rs` | 317 行 | 除 clap 入口外，还承载 APK/keystore 检查、指纹解析、JSON 输出拼装 |

### 1.2 结构层面的问题

#### `shield-gui/src-tauri`

- `main.rs` 是“后端总线”，职责边界不清晰
- 路径查找逻辑散落：`find_apktool_path` / `find_resources_path` / `find_apksigner_path` / `project_root_path`
- 配置读写、证书逻辑、更新逻辑、文件操作都与 Tauri command 注册耦合
- 后续任何一个功能点改动，都会持续放大 `main.rs`

#### `shield-gui/src`

- `App.tsx` 已经不只是根组件，而是完整应用实现
- 页面级逻辑没有拆分，导致：
  - 保护页小改动容易影响签名页/设置页
  - 状态和事件监听难以定位
  - UI 重做成本偏高

> 以上问题已通过拆分 `pages/*`、`components/app/*`、`hooks/*` 解决，保留这段是为了记录重构前风险来源。

#### `shield-cli/src`

- `commands/` 目录里既有“命令入口”，又承载了大量底层实现
- `protect.rs` 实际上已经是一个子系统，而不只是一个 command handler
- `sign.rs`、`main.rs`、GUI Tauri 后端里都存在证书/签名相关近似逻辑
- `no_window_command()` 在 CLI 和 GUI 各自重复定义

> 其中 `protect.rs` 的职责堆积已经通过 `protect/manifest.rs`、`protect/dex.rs`、`protect/runtime.rs`、`protect/signature.rs` 拆开。

### 1.3 当前目录里哪些不是重构重点

这些目录当前不是代码结构问题，不建议优先动：

- `shield-stub/`
  - 模块边界相对清楚，当前更需要稳定而不是重组
- `docs/ops` / `docs/process`
  - 分层已经合理，只需随着代码重构同步更新
- `tools/`
  - 仍被构建脚本和发布包依赖，暂不建议迁移位置
- `shield-gui/dist` / `shield-gui/node_modules` / `target` / `shield-stub/build`
  - 这些是构建产物或依赖目录，不属于版本化结构重构对象

## 2. 重构目标

### 2.1 总原则

- 先拆职责，再谈目录美观
- 先做“纯搬运、零行为变化”的重构
- 保持每一步都可编译、可测试、可打包
- 不同时大改 CLI 和 GUI 两个大面，避免回归叠加

### 2.2 本轮建议优先级

1. **优先拆 `shield-gui/src-tauri/src/main.rs`**
2. **再拆 `shield-gui/src/App.tsx`**
3. **最后拆 `shield-cli/src/commands/protect.rs`**

这个顺序的原因：

- Tauri 后端现在是最明显的维护瓶颈
- GUI 前端拆页风险低于 CLI 核心加固流程
- CLI `protect.rs` 牵涉加固主链路，应该最后做

## 3. 目标目录树

### 3.1 `shield-gui/src-tauri` 目标结构

保持 **binary crate**，继续以 `main.rs` 为入口，不引入 `lib.rs`。

当前已落地结构：

```text
shield-gui/src-tauri/src/
├── main.rs                 # 仅保留启动、state 注入、command 注册
├── app_config.rs           # config.toml 读写、迁移、状态结构
├── app_paths.rs            # resource_dir、project_root、apktool/resources/apksigner 查找
├── protect_runner.rs       # execute_protect_apk、进度桥接、取消逻辑
├── signing.rs              # execute_sign_apk、keystore alias 查询
├── apk_check.rs            # check_apk、签名检测、指纹比对
├── updates.rs              # 更新检查、忽略版本、缓存
├── file_ops.rs             # show_in_folder、delete_file、check_file_exists、open_url
└── build_info.rs           # get_app_info、get_build_info
```

### 3.2 `shield-gui/src` 目标结构

当前已落地结构：

```text
shield-gui/src/
├── App.tsx
├── pages/
│   ├── protect-page.tsx
│   ├── sign-page.tsx
│   ├── settings-page.tsx
│   └── about-page.tsx
├── components/
│   ├── app/
│   │   ├── branding.ts
│   │   └── common.tsx
│   └── ui/
├── hooks/
│   ├── use-mobile.tsx
│   ├── use-app-config.ts
│   ├── use-applied-theme-mode.ts
│   └── use-clipboard.ts
└── lib/
    ├── i18n.ts
    ├── path.ts
    ├── tauri.ts
    └── utils.ts
```

### 3.3 `shield-cli/src` 目标结构

当前已落地结构：

```text
shield-cli/src/
├── lib.rs
├── main.rs
├── error.rs
├── utils.rs
├── zipalign.rs
├── commands/
│   ├── mod.rs
│   ├── protect.rs          # 保留薄编排层
│   └── sign.rs
├── protect/
│   ├── mod.rs
│   ├── manifest.rs         # AndroidManifest 修改
│   ├── dex.rs              # DEX 处理、header 修复
│   ├── runtime.rs          # runtime 注入
│   └── signature.rs        # 原 APK 签名提取
└── dex_packer/
```

> `zipalign.rs` 当前已经相对独立，短期不建议再动。

## 4. 分阶段执行方案

## 阶段一：拆 Tauri 后端

状态：已完成

### 目标

- `shield-gui/src-tauri/src/main.rs` 收缩为薄入口
- 所有 command 仍保持原名字、原参数、原返回值
- GUI 前端不需要同时改协议

### 具体步骤

1. 新建 `app_config.rs`
   - 搬走：
     - `SignConfig`
     - `StoredSigningConfig`
     - `UpdateCache`
     - `AppConfig`
     - `AppConfigPayload`
     - `AppConfigState`
     - 配置加载/迁移/保存

2. 新建 `app_paths.rs`
   - 搬走：
     - `resource_dir`
     - `appimage_resource_dir`
     - `find_apktool_path`
     - `find_resources_path`
     - `find_apksigner_path`
     - `project_root_path`
     - `strip_unc_prefix`

3. 新建 `signing.rs`
   - 搬走：
     - `execute_sign_apk`
     - `query_keystore_aliases`
     - `parse_keytool_aliases`

4. 新建 `apk_check.rs`
   - 搬走：
     - `ApkCheckResult`
     - `CertCompareResult`
     - `do_compare_cert_fingerprints`
     - `extract_apk_fingerprint`
     - `extract_keystore_fingerprint`
     - `parse_sha256_fingerprint`
     - `parse_sha256_from_apksigner`
     - `normalize_fingerprint`
     - `do_check_apk`
     - `check_apk_signed`

5. 新建 `updates.rs`
   - 搬走：
     - `UpdateCheckResult`
     - `compare_semver`
     - `get_cached_update`
     - `save_update_to_cache`
     - 更新命令实现

6. `main.rs`
   - 最终只保留：
     - 应用启动
     - state 注入
     - command 注册
     - 对各模块的薄包装
     - 少量共享 state 定义
     - `#[tauri::command]` 入口
     - `Builder` / `generate_handler!`

### 风险

- `tauri::State` 类型路径一旦改错，编译会报错
- 模块移动后需注意私有函数可见性
- Windows 相关无窗口子进程逻辑不能丢

## 阶段二：拆 React 前端

状态：已完成

### 目标

- `App.tsx` 降到 300 行以内
- 每个页面单文件职责清晰
- 不引入新的状态库

### 具体步骤

1. 先抽纯展示组件
   - `DropZone`
   - `SelectedApkCard`
   - `StatusMessage`
   - `AppButton`

2. 再拆页面
   - `ProtectPage`
   - `SignPage`
   - `SettingsPage`
   - `AboutPage`

3. 最后再抽 Hook
   - `useAppConfig`
   - `useProtectProgress`

> 实际落地时保留了 `use-app-config.ts`、`use-applied-theme-mode.ts`、`use-clipboard.ts` 三个 hook；`useProtectProgress` 暂未单独抽离，因为当前只被加固页消费，继续内聚在页面内更直接。

### 风险

- 当前页面间共享 `signConfig` / 密码状态，拆页时要防止重复 state
- 事件监听解绑逻辑必须保留在正确层级

## 阶段三：拆 CLI 加固主流程

状态：已完成

### 目标

- `shield-cli/src/commands/protect.rs` 降到 250 行以内
- 加固主流程保持不变

### 具体步骤

1. 新建 `protect/signature.rs`
   - 签名提取、`keytool` / 临时 Java 提取逻辑搬走

2. 新建 `protect/manifest.rs`
   - `modify_manifest`
   - XML 属性处理函数

3. 新建 `protect/dex.rs`
   - `process_dex`
   - `patch_dex_header`
   - `adler32_checksum`

4. 新建 `protect/runtime.rs`
   - `inject_runtime`
   - ABI 收集
   - runtime 资源读取

5. 保留 `commands/protect.rs`
   - 只做流程编排、进度回调、取消控制

> 实际落地结果与方案一致，原有 manifest / dex / runtime / signature 测试也分别迁到了对应模块。

### 风险

- `protect.rs` 现有测试很多，拆模块时测试也要跟着迁移
- 这个阶段最容易引出行为回归，必须最后做

## 5. 暂时不要做的重构

这些现在不建议动：

- 不把 `shield-gui/src-tauri` 改成 `lib.rs + main.rs` 双入口
- 不重组 `shield-stub` 的 Gradle / Rust 目录
- 不把 `tools/` 改成更深层的目录
- 不一次性引入前端路由器、状态库或表单库
- 不把 CLI 和 GUI 的所有重复逻辑一次性抽成共享大模块

## 6. 建议的执行顺序

建议严格按下面顺序推进：

1. 拆 `shield-gui/src-tauri/src/main.rs`
2. 拆 `shield-gui/src/App.tsx`
3. 更新文档（`architecture.md` / `docs/README.md`）
4. 拆 `shield-cli/src/commands/protect.rs`

上述四步已全部完成。

## 7. 验收标准

每个阶段完成后至少满足：

- `cargo test --manifest-path shield-cli/Cargo.toml`
- `cargo check --manifest-path shield-gui/src-tauri/Cargo.toml`
- `npm run build`（在 `shield-gui/` 下）
- `git diff --check`

GUI 相关阶段完成后，建议再实际验证：

- 加固
- 自动签名
- 中间产物清理
- macOS `.app` 打包

## 8. 当前推荐动作

当前这一轮目录与代码重构已经完成。下一步更适合转入两类工作：

1. 针对 GUI 继续做页面级重构，把加固页和设置页里的局部状态继续按功能抽成更细的 hooks / panels
2. 再评估其余重复辅助逻辑是否值得继续上收，避免为了“共享”而制造新的耦合

其中第 1 项已经继续推进到：

- `ProtectPage` 的事件监听、自动签名收尾、拖拽状态已收口到 `use-protect-workflow.ts`
- `SignPage` 的签名流程、拖拽状态已收口到 `use-sign-workflow.ts`
- `AboutPage` 的数据加载与手动检查更新已收口到 `use-about-page.ts`
- `SettingsPage` 的表单状态与保存逻辑已收口到 `use-settings-form.ts`
- 设置页签名配置区已拆为 `settings-signing-panel.tsx`
- 加固页进度侧栏、签名页摘要卡、关于页信息卡已拆到 `components/app/*` 下独立组件
