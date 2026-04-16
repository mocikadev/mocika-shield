# Mocika Shield

[![最新版本](https://img.shields.io/github/v/release/mocikadev/mocika-shield?style=flat-square&label=最新版本&color=6366f1)](https://github.com/mocikadev/mocika-shield/releases/latest)
[![平台](https://img.shields.io/badge/平台-Linux%20%7C%20macOS%20%7C%20Windows-blue?style=flat-square)](https://github.com/mocikadev/mocika-shield/releases/latest)

Android APK 离线加固工具，桌面 GUI，三平台支持。

---

## 功能

- **APK 加固**：对 DEX 文件加密保护，防止静态反编译
- **防重打包**：绑定原始签名，重打包后无法正常运行
- **多架构支持**：arm64-v8a / armeabi-v7a / x86 / x86_64
- **完全离线**：加固过程在本地完成，不上传任何文件
- **内置签名工具**：支持拖拽 APK + 密钥库，一键完成签名
- **版本更新提示**：有新版本时自动提示

---

## 下载

前往 [Releases](../../releases) 页面下载对应平台的安装包。

| 平台 | 安装包 |
|------|--------|
| Linux | `.AppImage`（免安装，点击即用）/ `.deb`（Debian / Ubuntu） |
| macOS | `.dmg` |
| Windows | `-setup.exe` |

---

## 使用说明

安装后直接打开，界面包含两个标签页：

**加固**

1. 拖入或选择一个**已签名的** APK
2. 点击「加固」，等待完成
3. 自动生成加固后的 APK（未签名）
4. 切换到签名标签页对产物重新签名，完成后即可安装

> ⚠️ **加固前的 APK 必须已签名。** 未签名的 APK 无法进行加固。

**签名**

拖入 APK + 配置密钥库 → 点击「签名」即可。

> 在设置页配置好密钥库后，可开启「加固后自动签名」，加固与签名一步完成。

---

## 截图

| 加固 | 签名 |
|------|------|
| ![加固界面](screenshots/home.png) | ![签名界面](screenshots/sign.png) |

| 设置 | 关于 |
|------|------|
| ![设置界面](screenshots/settings.png) | ![关于页](screenshots/about.png) |

---

## 系统要求

使用前请确保已安装 **Java 8 或以上版本**。
