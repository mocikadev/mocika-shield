# 桌面 Java 工具适配资源

正式源码在 `src/`，生成物 `build/java-adapter.jar` 不入库。使用 Java 8 标准 API，当前私有协议为 MSJA 4；提供证书查询／新建及固定 apksigner／apktool 入口，不提供任意 Java 命令。

```bash
make build-java-adapter
python3 scripts/build_java_adapter.py --check
```

构建要求 JDK 17，用户运行支持 Java 8。默认编译器不是 17 时可单独指定，不改全局环境：

```bash
python3 scripts/build_java_adapter.py --javac /实际的JDK17/bin/javac
```

普通 Cargo 编译不运行 javac；GUI 构建会准备并共用该资源。它不包含 Android 壳、不放进 Android 资源 ZIP，也不要求 Maven、额外 jar／zip 工具或运行时联网。

Windows 默认业务已接入适配，macOS／Linux 默认保留原工具路径。密码不进入请求／日志，工作根、停止和保存权留在原调用方；详细契约统一见[设计](../../docs/design/java-tool-adapter.md)，实际覆盖见[验收表](../../docs/process/java-tool-adapter-plan.md)。
