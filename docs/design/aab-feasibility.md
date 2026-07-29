# AAB 加固可行性结论

本文档沉淀 AAB 单模块加固实验的验证结论、适用边界和后续正式支持计划。当前正式版本仍只支持 APK；实验通过不代表已经支持直接加固 AAB。

## 实验目标

实验优先验证现有 DEXB v5 是否可以穿过 AAB 到设备 APK 的转换链路，并确认现有壳加载方案在拆分安装场景中的基础可行性：

1. `base/dex/classes.dex` 的 DEX 头部 `file_size` 之外追加载荷，经过 bundletool 生成拆分 APK 和通用 APK 后是否保留。
2. 单 base 模块能否复用现有 DEXB v5、混淆 Stub DEX、Native 解密和 `PathClassLoader` 注入链路。
3. ABI 配置 APK、多 DEX、覆盖更新、清除数据和错误签名拒绝是否符合预期。
4. AAB 上传证书、Play 应用签名证书与本地测试证书之间需要怎样的签名绑定边界。

## 已验证结论

实验环境使用 bundletool 1.18.3，并在 Android 15 arm64 真机完成运行验证。

### DEX 尾部载荷保真

- AAB 中 `base/dex/classes.dex` 的 DEX 头部 `file_size` 保持原值，固定实验标记追加在物理文件末尾。
- bundletool 默认拆分 APK 集、通用 APK、重新 JAR 签名后的 AAB 和设备实际安装的 base APK 均保留尾部标记。
- bundletool 的设备选择、安装和 Android 包管理器安装过程没有清理 DEX 头部声明范围之外的载荷。

该结果证明现有尾部容器可以通过本地 bundletool 链路，但不能据此推断 Google Play 服务端一定采用相同行为。

### DEXB v5 运行链路

- 正式 `shield-core` packer 生成的 DEXB v5 可以追加到单 base 模块的 Stub DEX。
- bundletool 生成并安装设备 APK 集后，Native 库能够从对应 ABI 配置 APK 加载。
- 壳 DEX 不包含原始业务 Activity；运行时完成解密和 DEX 注入后，原始 Activity 可以正常启动。
- 强制停止后再次启动、清除应用数据后重建缓存均正常。
- 最终 APK 使用错误证书签名时，运行时按预期拒绝解密；恢复绑定证书后可以正常启动。

### ABI、多 DEX 与覆盖更新

- 四种 ABI Native 库由 AAB 模块的 `jniLibs` 在构建阶段提供，bundletool 会生成独立 ABI 配置 APK；arm64 设备只安装对应的 arm64 配置。
- 业务 `classes.dex` 与 `classes2.dex` 均可进入同一 DEXB v5 载荷并在运行时成功加载。
- 使用同一证书从较低 `versionCode` 覆盖到较高版本后，版本隔离缓存能够重建，应用继续正常启动。
- Native 库必须在 AAB 模块构建阶段纳入，不能沿用“生成 AAB 后再注入 Native 库”的做法。

## 尚未验证的门禁

以下项目未完成，因此当前不能宣称支持 AAB 加固：

- Google Play 内部测试轨道是否保留 DEX 尾部载荷。
- Play 应用签名证书绑定，以及上传证书与设备最终证书的配置流程。
- 动态功能模块、安装时模块、条件模块和按需模块。
- Asset Pack、Instant App 和第三方应用商店的 AAB 转换链路。
- 生产级 AAB 解析、Manifest 修改、资源处理、签名校验、错误恢复和 GUI/CLI 交互。

## 正式支持边界

AAB 是发布格式，设备实际安装的是由 Google Play 或 bundletool 生成的 base APK、配置 APK和功能模块 APK。AAB 通常使用上传密钥签名，而设备 APK 使用 Play 应用签名密钥。因此，APK 流程中“从输入文件提取当前证书并绑定 DEXB 密钥”的策略不能原样复用。

正式方案至少需要：

1. 允许用户配置并校验 Play 应用签名证书 SHA-256 指纹。
2. 为本地 bundletool 测试和 Play 分发建立明确、互不混淆的证书模式。
3. 在模块构建阶段生成 Stub DEX、Manifest 和 ABI Native 库，而不是事后修改已生成 AAB 的模块结构。
4. 明确只支持 base 模块还是同时处理动态功能模块，并为每种交付模式建立独立测试矩阵。
5. 验证 Google Play 服务端产物后，才能在 GUI、CLI 和用户文档中声明正式支持。

## 后续版本规划

AAB 正式支持规划为 `1.5.0` 独立主题版本，不并入 `1.3.0` 的运行时安全收尾，也不与 `1.4.0` 的内存 DEX 生产化混合。

建议阶段：

| 阶段 | 目标 |
|------|------|
| `1.5.0-alpha.1` | 建立单 base 模块正式加固流程、Play 签名指纹配置和本地 bundletool 端到端测试 |
| `1.5.0-beta.1` | 完成 Google Play 内部测试轨道、四 ABI、单/多 DEX及覆盖更新验证 |
| `1.5.0-rc.1` | 冻结输入输出协议和 GUI/CLI 行为，只修复阻塞缺陷 |
| `1.5.0` | 在 Play 产物和发布矩阵全部通过后提供稳定支持 |

动态功能模块和 Asset Pack 是否进入 `1.5.0`，应在 Alpha 阶段根据真实需求与测试结果确定；不得仅凭单 base 模块实验结论默认承诺支持。
