# Mocika Shield — Android APK 加固工具

简体中文 | [English](README.en.md)

[![最新版本](https://img.shields.io/github/v/release/mocikadev/mocika-shield?style=flat-square&label=最新版本&color=6366f1)](https://github.com/mocikadev/mocika-shield/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/mocikadev/mocika-shield/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/mocikadev/mocika-shield/actions/workflows/ci.yml)
[![许可证](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-green?style=flat-square)](#许可证)

在本机完成 Android APK 加固、签名和证书管理。通过 DEX 加密、签名绑定与基础运行时保护，提高静态分析、篡改和非法重签成本，不承诺无法脱壳或逆向。

推荐使用 Windows、macOS、Linux 桌面 GUI，支持中英文界面；CLI 可从源码构建，用于自动化和本地开发。

> 仅用于保护你拥有合法权利的 Android 应用，请勿用于绕过第三方保护或其他未授权场景。

## 核心能力

- **DEX 加固与运行时保护**：加密业务 DEX，绑定原签名证书；提供基础反调试和可选的严格保护。
- **加固前风险检查**：检查签名、已有加固、系统要求、ABI 等兼容信号，提示风险及可处理方式。
- **加固签名一体化**：证书导入、新建与管理，加固后自动签名，也支持独立签名。
- **省去重复配置**：记住常用加固选项和加固页证书选择，可调整输出目录与文件名，任务开始后固定本次配置。
- **安装与系统兼容**：处理 APK ZIP 对齐及常见 Native 库兼容问题；标准模式与 Android 4.4 工控模式分开选择。
- **诊断与反馈**：提供脱敏诊断摘要，失败时可预览并确认发送安全错误报告；APK 和签名材料始终在本机处理。

## 下载与快速开始

从 [GitHub Releases](https://github.com/mocikadev/mocika-shield/releases/latest) 下载正式桌面安装包：

| 平台 | 安装包 |
|---|---|
| Windows | `MocikaShield_x.y.z_windows_x64_setup.exe` |
| macOS | `MocikaShield_x.y.z_macos_universal.dmg` |
| Linux | `MocikaShield_x.y.z_linux_amd64.AppImage` 或 `.deb` |

使用前安装 **完整 JDK 8 或更高版本**，确保 `java`、`keytool` 可用；使用桌面发布包无需自行安装 Android SDK。安装包尚未进行商业代码签名或 macOS 公证，仅从可信发布地址下载。

1. 在 **证书** 页面导入原 APK 使用的签名证书。新项目可创建证书，但输入 APK 也必须先用该证书签名。
2. 在 **加固** 页面选择已签名 APK，查看预检结果。
3. 选择目标系统和保护策略。一般使用“Android 5.0 及以上”与“标准保护（推荐）”；目标确有 Android 4.4 时再选择工控模式。
4. 确认输出目录、文件名和自动签名证书，按需调整当前应用的分享选择，再开始加固。
5. 使用签名后的产物，在目标设备验证安装、启动、主要业务功能和覆盖升级。

输出默认位于原 APK 同目录，并自动建议文件名；执行前可修改。不启用自动签名时，产物仍需使用原证书签名才能安装运行。**加固产物与原证书绑定，换证书重签会导致应用无法启动。**

<details>
<summary>macOS 首次打开提示无法验证开发者</summary>

确认安装包来源可信，并将应用移到“应用程序”目录后，可执行：

```bash
xattr -rd com.apple.quarantine /Applications/MocikaShield.app
```

该命令移除隔离标记，不代表应用已获 Apple 公证。随后重新打开；其他问题见[使用指南](docs/usage.md)。

</details>

## 界面预览

![Mocika Shield 加固页示意](docs/assets/screenshots/readme-protect-main.png)

截图用于展示页面布局，具体选项以当前版本为准。更多页面与操作说明见[使用指南](docs/usage.md)。

## 兼容性与限制

| 模式 | 适用范围 |
|---|---|
| Android 5.0 及以上 | API 21+，支持 `armeabi-v7a`、`arm64-v8a`、`x86`、`x86_64`；不要求每个 APK 都包含四种架构 |
| Android 4.4 工控兼容 | API 19+；只接受无 Native 库或 Native 库仅含 `armeabi-v7a` 的 APK；真机验证范围为 Android 4.4.2、`armeabi-v7a`/NEON 工控设备 |

- 兼容模式不会降低原应用的 `minSdkVersion`，不同厂商系统和硬件仍需实测。
- 只支持 APK，不支持直接加固 AAB、APKS 或已加固 APK；输入 APK 必须已经签名。
- GUI 一次处理一个 APK，暂不提供批量队列。
- 混合旧 ABI 可逐次确认排除，但优先建议从原工程过滤业务不需要的架构；不会为缺失架构生成业务库。
- 16 KB ZIP 对齐不等于所有第三方 `.so` 都满足 ELF 页大小要求，也不等于通过 Google Play 审核。
- 部分 Fedora 环境下 AppImage 出现空白窗口的问题仍在调查，见 [#121](https://github.com/mocikadev/mocika-shield/issues/121)。

详细边界见[使用指南](docs/usage.md)；遇到问题请提供版本、环境及脱敏诊断信息。

## 工作原理与安全边界

加固时读取原签名证书，压缩并加密 DEX，注入壳资源，再重新打包、对齐并按需签名。运行时由壳执行安全检查、校验或解密 DEX 缓存，加载业务代码并启动原应用。

当前正式方案会在应用私有目录使用解密后的 DEX 缓存，**不是完整内存 DEX 或方法代码抽取方案**。Root、进程控制或其他高权限环境下，攻击者仍可能提取运行时代码。标准保护不因 Root 信号拒绝启动；严格保护可阻断部分风险环境，但无法保证识别隐藏 Root 或抵御绕过。

加固不能替代服务端鉴权、密钥管理和应用自身的安全设计。技术细节见[运行时安全](docs/design/runtime-security.md)与[技术内参](docs/design/internals.md)。

## 隐私说明

APK、证书、密钥库和签名密码只在本机处理，不上传业务文件。桌面工具有三个独立数据通道：

| 通道 | 数据与控制方式 |
|---|---|
| 匿名使用统计 | 默认启用，记录随机安装标识、工具版本、启动和任务计数及固定失败类别；可在设置中关闭，不包含包名或原始日志 |
| 安全错误报告 | 每份报告先预览、再确认发送；不上传原始日志、APK、路径、包名、证书或密码，不受匿名统计开关控制 |
| 应用使用分享 | 加固/签名页控制，新包名默认选中，同包名取消后跨页及重启保持；成功时发送应用名称、包名、版本码、工具版本、操作、流程、成功日期及协议/去重信息，不带设备标识 |

应用分享独立于匿名统计，仅维护者可见，明细保留 180 天；取消停止同包名后续分享，不删除已接收记录。**加固后的 APK 不包含这些统计或分享上报。** 数据范围、保留与删除说明见[数据与隐私说明](docs/ops/telemetry.md)。

## 文档与开发者入口

| 需要了解 | 文档 |
|---|---|
| GUI 操作、签名证书、配置位置、CLI 用法 | [使用指南](docs/usage.md) |
| 安装与运行问题 | [本地排障](docs/ops/troubleshooting.md) |
| 从源码编译 | [构建指南](docs/ops/build.md)、[环境要求](docs/ops/environment.md) |
| 模块结构、原理与设计 | [文档导航](docs/README.md) |
| 后续规划 | [路线图](docs/process/roadmap.md) |

CLI 仅供源码构建和自动化使用，Release 不单独提供 CLI 包。构建顺序为先 `make build-stub`，再 `make build-cli` 或 `make build-gui`；完整依赖与平台步骤以构建指南为准。

## 反馈与交流

- 使用问题请先阅读[反馈指南](docs/process/support.md)，再提交 [GitHub Issue](https://github.com/mocikadev/mocika-shield/issues)；“关于”页可复制诊断信息。
- 功能建议请使用[需求表单](https://github.com/mocikadev/mocika-shield/issues/new?template=feature_request.yml)，已有相同需求可在原 issue 点赞。
- 安全漏洞请按 [SECURITY.md](SECURITY.md) 私下报告，不公开可利用细节、业务 APK、证书或密码。
- QQ 用户交流群：`1090352773`。群内便于沟通，正式问题仍建议保留 issue 记录。

<details>
<summary>QQ群二维码</summary>

<img src="docs/assets/community/qq-group.png" alt="Mocika Shield QQ 用户交流群：1090352773" width="300">

</details>

## 许可证

采用 **MIT OR Apache-2.0** 双协议，可选择其中任意一种：[MIT](LICENSE-MIT) · [Apache-2.0](LICENSE-APACHE)。
