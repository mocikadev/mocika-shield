# 本地诊断与排障命令

本文档提供本地排障时常用的命令，用于快速判断问题位于 Java 环境、APK 签名、APK 对齐、壳资源构建还是设备运行时。

## Java / JDK 检查

签名、Alias 识别和加固流程需要完整 JDK 8+，不是只包含 `java` 的 JRE。

```bash
java -version
keytool -help
```

期望结果：

- `java -version` 显示 8 或更高版本
- `keytool -help` 可以正常输出帮助

如果 GUI 关于页提示缺少 `keytool`，通常说明当前 PATH 指向的是不完整运行时，或系统里安装了多个 Java 版本。

## APK 签名检查

输入 APK 必须已经签名。可使用仓库内置的 `apksigner.jar` 检查：

```bash
java -jar tools/apksigner.jar verify --print-certs app.apk
```

只看是否已签名时：

```bash
java -jar tools/apksigner.jar verify app.apk
```

退出码为 `0` 表示签名验证通过。V2/V3/V4 签名不一定会在 `META-INF/` 下留下证书文件，因此不要只依赖 `unzip -l app.apk | grep META-INF` 判断是否已签名。

加固绑定的是 `--print-certs` 输出中的当前 APK 内容签名证书 SHA-256。加固完成后必须使用同一证书对应的 keystore 重新签名，可在签名前后分别执行以下命令核对：

```bash
java -jar tools/apksigner.jar verify --print-certs input.apk
java -jar tools/apksigner.jar verify --print-certs protected_signed.apk
```

两次输出的 `certificate SHA-256 digest` 必须一致。当前 DEXB v5 仅支持单签名 APK；如果输出多个内容签名证书，需要先改为单证书签名后再加固。

## APK 16 KB 对齐检查

本工具的加固输出和 GUI 签名链路会自动执行 4 字节 / 16 KB ZIP 对齐。若需要用 Android SDK 官方工具复核：

```bash
zipalign -c -P 16 4 app.apk
```

如果本机找不到 `zipalign`，通常位于：

```bash
ls "$ANDROID_HOME/build-tools"
```

选择已安装 build-tools 版本下的 `zipalign` 即可，例如：

```bash
"$ANDROID_HOME/build-tools/35.0.0/zipalign" -c -P 16 4 app.apk
```

Google Play 16 KB 对齐问题反馈时，请同时提供 Google Play 的拒绝提示或 `zipalign` 检查输出。

`zipalign` 校验通过只代表 APK 内条目偏移正确，不代表 `.so` 一定未压缩，也不代表 ELF `LOAD` 段支持 16 KB 页面。原 APK 设置 `android:extractNativeLibs="false"` 时，还需要确认 Native 库使用不压缩存储：

```bash
unzip -lv app.apk | grep -E "lib/.+\\.so$"
```

输出中的 `.so` 应显示为 `Stored`，不能显示为 `Defl:N` 等压缩方式。完整设计和判断边界见 [Android Native 库打包与加载兼容设计](../design/native-library-packaging.md)。

## APK 结构检查

检查加固后是否注入壳 DEX 和 native 库：

```bash
unzip -l protected.apk | grep -E "classes|libmocikashield"
```

常见期望：

- `classes.dex` 存在
- `lib/<abi>/libmocikashield.so` 存在
- 不应出现旧方案的 `assets/app.bin`

检查 APK 包含哪些 ABI：

```bash
unzip -l app.apk | grep -E "lib/.+\\.so" | awk '{print $4}' | cut -d/ -f2 | sort -u
```

## 加固后安装失败

先保留完整安装输出：

```bash
adb install -r protected_signed.apk
```

常见排查方向：

- `INSTALL_FAILED_UPDATE_INCOMPATIBLE`：通常是签名证书与已安装版本不一致
- `INSTALL_PARSE_FAILED_NO_CERTIFICATES`：APK 未签名或签名损坏
- `INSTALL_FAILED_INVALID_APK` 且包含 `Failed to extract native libraries`：检查 `extractNativeLibs=false`、`.so` 压缩方式、ZIP 对齐和 ABI 是否一致
- ABI 相关错误：检查 APK 中的 `lib/<abi>/` 是否覆盖目标设备 ABI

## 加固后启动崩溃

先抓取关键日志：

```bash
adb logcat | grep -E "AndroidRuntime|ax|lx|rx|e[1-4]"
```

当前壳层日志 tag 已做弱特征化处理，常见 tag：

- `ax`：壳 Application
- `lx`：DEX 提取与加载
- `rx`：ARouter 兼容处理
- `e1` / `e2` / `e3` / `e4`：弱特征错误码

提交 issue 前请脱敏包名、业务类名、用户信息和内部路径。

## resources.zip 检查

CLI 和 GUI 运行时都依赖 `resources.zip`。源码构建或本地排障时，先构建 Android 壳资源：

```bash
make build-stub
```

期望产物：

```bash
ls -lh shield-stub/build/outputs/resources/resources.zip
```

如果缺少该文件，CLI / GUI 的加固流程会找不到壳 DEX 或 native 运行时资源。

## 发布前轻量检查

维护者发布前可运行：

```bash
bash scripts/check-release-ready.sh
```

该脚本会检查：

- 工作区是否干净
- 版本号是否同步
- `apktool`、`apksigner`、`resources.zip` 是否存在
- README 截图引用是否有效
- issue 模板、支持文档、CI/Release workflow 是否存在
- 是否误提交 APK、证书、数据库、配置文件等本地产物

## 反馈问题

如果仍无法定位问题，请阅读 [支持与问题反馈](../process/support.md)，并在 GUI 关于页点击“复制诊断信息”后粘贴到对应 issue 模板中。
