# 中文路径与 Java 工具兼容

中文问题有两个独立层面：工具**输出编码**影响显示和错误分类；原生 argv **输入转换**可能真正改变路径、Alias 或主题。修复前者不能证明后者正确。

当前实现契约见[Java 工具适配](java-tool-adapter.md)，平台和 Java 覆盖见[验收表](../process/java-tool-adapter-plan.md)。

## 已确认的问题

- 原工具输出可能为 GBK／ASCII，Rust 直接按 UTF-8 读取会损坏中文错误文本。共享 `java_process` 统一本次子进程的输出编码和语言，不修改宿主环境。
- Windows Java 8／17 的独立探针证明，中文 argv 进入 Java main 时可变成问号；中文 Alias／主题创建退出 0，也可能已经写入错误内容。移除原编码／语言属性并不能修复。
- 从 UTF-8 文件读取的独立期望值与实际证书内容可区分“显示乱码”和“材料损坏”；不能只凭退出码或控制台显示判断。
- 历史 QQ 群中文 APK 个例仍缺本机完整信息，不把所有用户失败归为同一根因。

## 当前处理

| 链路 | 处理 |
|------|------|
| Java 文本输出 | 统一子进程编码和语言，有限错误分类；不把猜编码当作协议 |
| Windows 工具输入 | 私有 UTF-8 请求交给 Java 8 适配 JAR，集中在 core，不在 GUI／CLI／MCP 分别改名 |
| 证书 | 标准接口读取实际 Alias／指纹；新建经原 keytool 和材料复核，导入原件不改写 |
| 引导与 Native 路径 | 使用系统已有、身份一致的 ASCII 目录别名；缺失则拒绝，不改卷设置或搬迁用户材料 |
| 前端、配置及数据库 | 保留 Unicode／UTF-8 及既有路径辅助函数，不做全局字符串替换或密码迁移 |
| 独立 SDK 工具验收 | 校验同字节 ASCII 副本，避免外部工具的路径限制污染产物有效性结论 |

`java_command`／`keytool_command` 的既有接口和版本探测保留；业务参数适配使用语义入口，不通过更改 `Command` 返回类型拦截任意后续参数。

## 仍有边界

- 中文业务文件名不等同任意 Unicode 工作根；补充字符引导目录仍提前拒绝。
- JKS 与 PKCS12 的密码／提供程序限制各自验证，不承诺任意中文密码组合。
- 非 UTF-8 Unix 文件名、长路径、全部 JDK 安装位置及外部 JVM 选项未穷尽；不能从一个平台外推全部支持。
- `open`、`explorer`、SDK 工具和 aapt2 有各自的参数规则，不能套用 Java 输出属性或仅凭 JVM 收到正确字符串就判为可用。

## 回归入口

输出编码对照为 `cargo test -p shield-core --test chinese_java_output -- --ignored --nocapture`，设置 `SHIELD_TEST_JDK_HOME` 选择 JDK；证书与探测对照为 `--test java_toolchain`。Windows 参数、实际证书及工具原生验证使用手动 `windows_paths`，默认业务使用 `windows_defaults`。

故障排查只收集脱敏诊断及系统／Java 版本，不要求私钥、密码或未经授权的业务 APK。历史逐轮探针、提交和耗时留在 Git／CI，不再作为当前操作指南。
