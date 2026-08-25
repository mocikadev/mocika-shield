# 1.4.0 apktool Manifest 兼容实施计划

> **供执行代理使用：** 必须逐任务使用 `superpowers:subagent-driven-development` 或 `superpowers:executing-plans` 执行；步骤使用复选框追踪。

**目标：** 将内置 apktool 升级至 3.0.3，并用含 `enableOnBackInvokedCallback` 的自建 APK 验证完整加固链路。

**架构：** 开发期仅保留 `tools/apktool_3.0.3.jar`；发布包继续映射为稳定路径 `tools/apktool.jar`。既有 Android smoke fixture 的 `<application>` 节点加入标准 Android 13 属性，端到端脚本验证原包解包、加固后 Manifest 保留与原有运行时回归。

**技术栈：** apktool、Android Gradle Plugin、Bash、Rust、Tauri v2。

**设计依据：** `docs/superpowers/specs/2026-08-25-1.4-compatibility-observability-design.md`

## 全局约束

- 正式路径只能使用 apktool `3.0.3`，不得混用旧版本。
- 发布包内资源路径固定为 `tools/apktool.jar`。
- 不提交用户 APK、uni-app 资源、证书或业务数据。
- 不改变 DEXB、签名、Native 库、16 KB 对齐和 Android 4.4 策略。
- 提交信息使用 `<英文类型>: <中文描述>`。

---

### 任务 1：替换资源并统一所有引用

**文件：**

- 删除：`tools/apktool_3.0.1.jar`
- 创建：`tools/apktool_3.0.3.jar`
- 修改：`apps/shield-gui/src-tauri/tauri.conf.json`、`apps/shield-gui/src-tauri/src/app_paths.rs`、`crates/shield-core/src/utils.rs`
- 修改：`scripts/release-cli.sh`、`scripts/release-linux.sh`、`scripts/release-macos.sh`、`scripts/release-windows.ps1`、`scripts/check-release-ready.sh`
- 修改：`docs/design/internals.md`、`docs/design/architecture.md`

- [ ] **步骤 1：写资源版本检查**

在发布资源检查中加入：

```bash
test -f "$ROOT/tools/apktool_3.0.3.jar"
! test -e "$ROOT/tools/apktool_3.0.1.jar"
java -jar "$ROOT/tools/apktool_3.0.3.jar" --version | grep -qx '3.0.3'
```

- [ ] **步骤 2：运行新检查确认其先失败**

运行 `test -f tools/apktool_3.0.3.jar`；预期失败，因为当前仓库只有 `apktool_3.0.1.jar`。

- [ ] **步骤 3：下载、校验并替换资源**

从 apktool `v3.0.3` 发布资产下载 JAR，确认 `java -jar tools/apktool_3.0.3.jar --version` 输出为 `3.0.3`。将所有开发期文件名替换为 `apktool_3.0.3.jar`；Tauri 映射保持：

```json
"../../../tools/apktool_3.0.3.jar": "tools/apktool.jar"
```

不得改变 `find_apktool()` 和 `find_apktool_path()` 的搜索优先级。

- [ ] **步骤 4：运行静态和发布资源检查**

```bash
! rg -n 'apktool_3\.0\.1' . -g '!**/target/**' -g '!**/node_modules/**'
scripts/check-release-ready.sh
```

预期：无旧文件名引用，发布检查通过开发资源检查。

- [ ] **步骤 5：提交完整资源替换**

```bash
git add tools/apktool_3.0.3.jar apps/shield-gui/src-tauri crates/shield-core/src/utils.rs scripts docs/design
git rm tools/apktool_3.0.1.jar
git commit -m 'build: 升级apktool兼容新版Manifest'
```

### 任务 2：扩展自建 Manifest 夹具与端到端回归

**文件：**

- 修改：`tests/fixtures/android-smoke-app/app/src/main/AndroidManifest.xml`
- 修改：`tests/scripts/run-protect-e2e.sh`、`tests/scripts/run-memory-loader-arouter-probe.sh`、`tests/scripts/prepare-memory-runtime-e2e-apks.sh`
- 修改：`docs/process/test-checklist.md`

**依赖：** 任务 1 的新 JAR。

- [ ] **步骤 1：写 Manifest 保留断言**

在 smoke fixture 的 `<application>` 节点加入：

```xml
android:enableOnBackInvokedCallback="true"
```

在 `run-protect-e2e.sh` 的最终 APK 解包后加入：

```bash
grep -q 'android:enableOnBackInvokedCallback="true"' "$DECODED/AndroidManifest.xml"
```

- [ ] **步骤 2：运行端到端脚本确认新增断言先失败**

```bash
make build-stub
tests/scripts/run-protect-e2e.sh
```

预期：在实现前，失败位置为 apktool 路径或新增 Manifest 断言，不以无关 DEX/签名失败替代。

- [ ] **步骤 3：完成脚本资源替换和夹具实现**

所有三个脚本统一改用 `apktool_3.0.3.jar`。保留 `compileSdk = 35`、`targetSdk = 35`、`extractNativeLibs=false`，不得删除或降级该属性。

- [ ] **步骤 4：执行无设备与设备回归**

```bash
make build-stub
tests/scripts/run-protect-e2e.sh
RUN_DEVICE_TEST=1 tests/scripts/run-protect-e2e.sh
tests/scripts/run-memory-loader-arouter-probe.sh
tests/scripts/prepare-memory-runtime-e2e-apks.sh
```

预期：无设备链路验证解包、重打包、MSHD、Native 别名、未压缩 `.so` 与对齐；有设备时另验证覆盖安装、首次解密与缓存命中。无设备时只记录无设备通过。

- [ ] **步骤 5：记录并提交回归结论**

```bash
git add tests/fixtures tests/scripts docs/process/test-checklist.md
git commit -m 'test: 覆盖新版Manifest加固回归'
```

### 任务 3：准备 Alpha.2 构建验证

**文件：** 由 `scripts/bump-version.sh` 同步的版本清单、`.github/release-notes/versions/1.4.0.md`、`docs/process/test-checklist.md`。

- [ ] **步骤 1：更新候选说明**

Release Notes 写明“升级 apktool 以兼容较新的 Android Manifest 属性”，同时保持“不是对 uni-app 前端资源的额外加密承诺”。

- [ ] **步骤 2：执行发布前验证**

```bash
make test
python3 -m unittest scripts.tests.test_project_stats
git diff --check
```

预期：通过。

- [ ] **步骤 3：同步版本并构建 `.app`**

```bash
scripts/bump-version.sh 1.4.0-alpha.2
make build-stub
make build-gui
```

预期：提供完整 macOS `.app` 验证包，不以裸二进制代替。

- [ ] **步骤 4：检查打包后的实际 JAR**

```bash
java -jar '<应用包>/Contents/Resources/tools/apktool.jar' --version
```

预期：输出 `3.0.3`。

- [ ] **步骤 5：提交候选版本准备**

```bash
git add crates shield-stub apps/shield-gui .github/release-notes docs/process
git commit -m 'build: 准备1.4候选版本'
```
