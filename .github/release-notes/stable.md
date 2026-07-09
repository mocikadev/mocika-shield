# Mocika Shield {{tag}}

这是当前的 **{{release_kind}}**。

## 本次说明

- 版本号：`{{version}}`
- 发布类型：{{release_kind}}
- 桌面 GUI：Tauri v2 + React + TypeScript
- 核心能力：APK 加固、签名、证书管理、版本更新提示
- 证书资料：本地 SQLite 管理，密码字段加密落盘，不上传 APK、证书或密码
- APK 对齐：加固与签名链路内置 4 KB / 16 KB ZIP 对齐
- 运行环境：签名、Alias 识别和加固流程需要 Java 17+
- 发布产物：GitHub Release 面向普通用户只提供桌面 GUI 安装包

## 使用建议

- 首次使用前，请先在证书页导入或创建签名证书，并按需设为默认
- 加固产物默认输出到原 APK 同目录；开启自动签名时会使用默认证书
- 如需稳定使用，优先下载与你当前平台匹配的 GUI 安装包
- macOS 未签名安装包首次打开时，如系统提示无法验证开发者，请按 README 中的说明移除隔离标记

## 已知限制

- 当前只维护 Tauri 桌面 GUI；CLI 仍可从源码构建，但 GitHub Release 不上传 CLI 包
- Windows、Linux、macOS 的安装包尚未做商业代码签名或公证
- 证书数据库的旧明文测试记录不再兼容，如遇到旧数据请重新导入或创建证书

## 自动生成的变更列表

以下内容由 GitHub 根据本次版本与上一个版本之间的提交自动生成。
