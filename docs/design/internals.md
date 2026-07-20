# INTERNALS.md — Mocika Shield 技术内参

> 本文档记录工程的业务细节、算法实现、数据格式与已知问题。
> 最后更新：2026-07-07

---

## 目录

1. [整体架构与数据流](#一整体架构与数据流)
2. [shield-cli 详解](#二shield-cli-详解)
3. [加密数据格式（DEXB v5）](#三加密数据格式dexb-v5)
4. [加解密算法](#四加解密算法)
5. [shield-stub 详解](#五shield-stub-详解)
6. [与 360 加固的差异](#六与-360-加固的差异)
7. [加固前后 APK 结构对比](#七加固前后-apk-结构对比)
8. [已知 Bug 与设计隐患](#八已知-bug-与设计隐患)
9. [后续迭代方向](#九后续迭代方向)

---

## 一、整体架构与数据流

```
┌─────────────────────────────────────────────────────────────────┐
│                      shield-cli（Rust）                          │
│                                                                 │
│  shield protect -i app.apk -o protected.apk                     │
│                                                                 │
│  ① apktool d          解包 APK（不反编译 Smali）                │
│  ② modify_manifest    注入 StubApp / meta-data                  │
│  ③ extract_signature  提取原始 APK 证书 SHA-256 指纹（3级降级） │
│  ④ process_dex        DEX → Zstd 压缩 → ChaCha20-Poly1305 加密 │
│                        → DEXB v5 payload（含签名指纹与随机 IKM）│
│  ⑤ inject_runtime     解压 resources.zip 注入 stub + .so；      │
│                        DEXB payload 以 MSHD 块追加到            │
│                        classes.dex 末尾（DEX file_size 之外）   │
│  ⑥ apktool b + 内置对齐  重打包并执行 4 KB / 16 KB ZIP 对齐      │
└──────────────────────────────┬──────────────────────────────────┘
                               │ protected.apk（未签名，需签名后安装）
                               ▼
              apksigner sign → 可安装 APK


┌─────────────────────────────────────────────────────────────────┐
│                 shield-stub（Android 设备上）                    │
│                                                                 │
│  App 启动                                                       │
│  ① StubApp.attachBaseContext()                                  │
│     ├─ exemptHiddenApi()                                        │
│     ├─ Ld.extractDexFiles(ctx) ──JNI──► Rust                   │
│     │      扫描 classes.dex 末尾 MSHD magic                     │
│     │      → 提取 DEXB v5 payload                              │
│     │      → 当前签名参与 HKDF 派生密钥 → ChaCha20-Poly1305 解密│
│     │      → timing-safe 签名指纹比对（v5）                    │
│     │      → Zstd 解压 → 落地到 app_dex/v{versionCode}/        │
│     ├─ Ld.p() ──JNI──► Rust                                    │
│     │      JNI 层调用 DexPathList.addDexPath()                  │
│     │      （不受 hidden API 限制，Java 反射作为降级路径）       │
│     └─ makeRealApp() → 真实 Application.attach()               │
│  ② StubApp.onCreate()                                          │
│     ├─ replaceAppReferences()                                   │
│     ├─ realApp.onCreate()                                       │
│     └─ ARouterCompat.injectARouterRouteMap()（按需）            │
└─────────────────────────────────────────────────────────────────┘
```

---

## 二、shield-cli 详解

### 2.1 命令行参数

```
shield protect [OPTIONS] --input <APK> --output <APK>

  -i, --input <APK>   输入 APK 路径
  -o, --output <APK>  输出 APK 路径
  -v, --verbose       输出详细日志
```

工具路径无需配置，按以下优先级自动检测：

1. 发布包路径：`bin/../lib/apktool.jar`、`bin/../resources/resources.zip`
2. 用户数据目录（`ProjectDirs`）
3. 系统数据目录（`/usr/local/share/mocika-shield/` 等）
4. 开发环境路径：`tools/apktool_3.0.1.jar`、`shield-stub/build/outputs/resources/resources.zip`

### 2.2 protect 命令核心流程

#### 步骤 ① 解包 APK

```bash
java -jar apktool.jar d <input.apk> -o <tmp/apk> -f --no-src
```

- `--no-src`：不反编译 Smali，只解压资源和 Manifest
- `-f`：强制覆盖目标目录

#### 步骤 ② Manifest 修改

使用 `xmltree 0.10` 进行结构化修改（非正则字符串操作）：

```xml
<!-- 修改前 -->
<application android:name="com.example.MyApp" ...>

<!-- 修改后 -->
<application android:name="dev.mocika.shield.loader.StubApp" ...>
    <meta-data android:name="ORIGINAL_APPLICATION"
               android:value="com.example.MyApp" />
```

- 删除 `android:appComponentFactory`（与壳 Application 冲突）
- 检查是否已存在 `ORIGINAL_APPLICATION`，避免重复插入（幂等）
- 签名指纹不写入 Manifest，改为写入 DEXB v5 明文头部并参与密钥派生

#### 步骤 ③ 签名提取

```
java -jar apksigner.jar verify --print-certs <apk>
    ↓ 验证 APK 的实际有效签名
解析当前 APK 内容签名证书的 "certificate SHA-256 digest"
    ↓
规范化为大写 64 位十六进制
```

> 取 **当前 X.509 内容签名证书 DER 的 SHA-256**。`apksigner` 的 `certificate SHA-256 digest` 与 Runtime 侧对 `PackageManager` 返回的 `Signature.toByteArray()` 计算 SHA-256 口径一致。

安全约束：

- 不使用 `keytool -jarfile` 提取 APK 证书，避免严格 V2/V3-only APK 无法读取，也避免命中已经失效的 V1 残留证书
- 只接受 `apksigner` 验证成功的 APK
- 忽略 public key digest 和 Source Stamp 证书，只读取 APK 内容签名证书摘要
- DEXB v5 只支持一个签名指纹；检测到多签名 APK 时直接拒绝加固
- 加固输出重新签名时必须继续使用该当前证书对应的 keystore，否则设备运行时取得的实际指纹不同，AEAD 解密和指纹比较都会失败
- GUI 配置自动签名时，所选证书指纹必须在解包前与输入 APK 指纹一致；不一致或读取失败直接终止，不生成加固产物
- 核心加固入口会独立检查 MSHD 追加块并拒绝已加固 APK，不能依赖 GUI 预检作为唯一防线

#### 步骤 ④ DEX 打包

```
原始 DEX 文件（classes.dex、classes2.dex ...）
    ↓ 逐个 Zstd 压缩（level 19）
    ↓ 构建 DEXB v5 buffer（含签名指纹与随机 IKM）
    ↓ 随机 nonce + HKDF-SHA256 派生密钥
    ↓ ChaCha20-Poly1305 加密
→ DEXB v5 格式完整内容
```

DEX 排序规则：`classes.dex` 永远最前，其余按文件名字典序。

#### 步骤 ⑤ Runtime 注入

`resources.zip` 内部结构：

```
stub-classes.dex               ← 壳 Java 层编译产物
lib/
├─ arm64-v8a/libmocikashield.so
├─ armeabi-v7a/libmocikashield.so
├─ x86/libmocikashield.so
└─ x86_64/libmocikashield.so
metadata.json
```

注入逻辑：
1. 解压 `resources.zip` 到 APK 目录
2. `stub-classes.dex` → 重命名为 `classes.dex`（占据主 dex 位置）
3. **将 DEXB v5 加密数据以 MSHD 块格式追加到 `classes.dex` 末尾**（DEX `file_size` 之外，工具不可见）
4. 跳过所有含 `libzstd-jni` 的文件（Rust 静态链接了 zstd）

#### 步骤 ⑥ 重打包

```bash
java -jar apktool.jar b <tmp/apk> -o <output.apk> -f
```

完成后打印输入/输出文件大小与压缩比。

---

## 三、加密数据格式（DEXB v5）

加密 DEX 数据以 **MSHD 追加块**格式写在 `classes.dex` 文件末尾（DEX `file_size` 边界之外）：

```
[classes.dex 标准内容，工具解析至 file_size 边界后停止]
...
MSHD            (4 bytes ASCII magic，用于 runtime 侧定位追加块起点)
payload_len     (4 bytes u32 LE，不含 magic 和 payload_len 字段自身)
<DEXB 加密数据> (payload_len bytes，DEXB v5 格式完整内容)
```

追加块内的 **DEXB payload** 采用 **Version 5** 格式：

```
Offset     Size      字段             说明
──────────────────────────────────────────────────────────────
0          4         magic            固定 ASCII "DEXB"
4          4         version          u32 LE = 5
8          4         dex_count        u32 LE，DEX 文件数量 N（上限 256）
12         1         sig_len          签名指纹字节长度（0 表示无签名）
13         sig_len   signature        原始 APK 证书 SHA-256 指纹
                                      （大写 hex ASCII，64 字节；或空）
13+sig_len 1         ikm_len          IKM 字节长度，当前为 32
14+sig_len ikm_len   ikm              每次加固随机生成的密钥材料（明文）
14+sig_len+ikm_len
           12        nonce            每次加固随机生成（明文）
26+sig_len+ikm_len
           *         ciphertext       ChaCha20-Poly1305 密文
                                      （含 16 字节 Poly1305 AEAD tag）
                                      解密后明文格式见下表
```

> **magic / version / dex_count / sig_len / signature / ikm_len / ikm / nonce 为明文头部**，runtime 侧无需密钥即可读取。
> 当前 stub 仅支持 v5；v5 与 v4 不兼容，旧加固 APK 需重新加固。

解密后的明文（payload）格式：

```
            ─── 循环 N 次（元数据区）───
字段             Size      说明
name_len         1         文件名字节长度
name             name_len  原始文件名（如 "classes.dex"）
compressed_size  4         u32 LE，Zstd 压缩后大小
original_size    4         u32 LE，原始 DEX 大小
            ─── 循环 N 次（数据区）───
compressed_data  comp_sz   Zstd 压缩块（level 19）
```

**格式要点：**
- `MSHD` magic：runtime 侧全文反向扫描，每个候选位置均做严格一致性校验（magic + payload_len + 文件末尾三者完全吻合），消除误命中风险
- 桌面端与 CLI 的加固状态预检会流式扫描完整 `classes.dex`，同样要求 `magic + payload_len` 恰好指向文件末尾；不依赖固定大小的尾部窗口，也不把完整 DEX 载入内存
- AEAD tag 校验失败立即报错，不返回任何明文
- 每次加固追加前，先读取 DEX header 的 `file_size` 并裁剪文件至原始边界，确保不会产生多份 MSHD
- Zstd 参数：`level = 19`（最高压缩比）

---

## 四、加解密算法

### 4.1 密钥派生（HKDF-SHA256）

```rust
pub fn derive_key(ikm: &[u8], nonce: &[u8; 12], cert_fingerprint: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(nonce), ikm);
    let mut okm = [0u8; 32];
    hk.expand(cert_fingerprint, &mut okm).unwrap();
    okm
}
```

- **IKM**：CLI 每次加固随机生成 32 字节，写入 DEXB v5 明文头部
- **Salt**：nonce（12 字节，每次加固随机生成，写入 DEXB 头部明文区）
- **Info**：原始 APK 证书 SHA-256 指纹字节，使派生密钥绑定签名证书
- **OKM**：32 字节，直接作为 ChaCha20-Poly1305 密钥

### 4.2 ChaCha20-Poly1305 加密（CLI 侧）

```rust
pub fn encrypt(plaintext: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> Vec<u8> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let nonce = Nonce::<ChaCha20Poly1305>::from_slice(nonce);
    cipher.encrypt(nonce, plaintext).expect("加密不应失败")
}
```

- 加密范围：`meta[] + data[]` 整体加密
- 输出：密文字节（末尾自带 16 字节 Poly1305 AEAD tag）
- nonce 由 `rand::thread_rng()` 每次加固随机生成，一次性使用

### 4.3 ChaCha20-Poly1305 解密（stub 侧）

```rust
pub fn decrypt(ciphertext: &[u8], key: &[u8; 32], nonce: &[u8; 12]) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let nonce = Nonce::<ChaCha20Poly1305>::from_slice(nonce);
    cipher.decrypt(nonce, ciphertext)
        .map_err(|_| anyhow!("AEAD 解密失败：密文被篡改或密钥错误"))
}
```

### 4.4 密钥材料与签名绑定

v5 不再通过 Manifest 写入 `ENCRYPTION_KEY`。随机 IKM 明文存放在 DEXB 头部，实际 AEAD 密钥由 `HKDF(ikm, nonce, cert_fingerprint)` 派生。安全边界在于：
- ChaCha20-Poly1305 AEAD 防密文篡改
- HKDF + 随机 IKM + 随机 nonce 使每次加固产生不同密文
- 证书指纹参与密钥派生，重签名后派生密钥不同，AEAD 解密直接失败
- 运行时仍会读取设备实际签名指纹并执行常数时间校验

---

## 五、shield-stub 详解

### 5.1 Java 壳层启动时序

```
StubApp.attachBaseContext(base)
    │
    ├─[1] super.attachBaseContext(base)
    │
    ├─[2] exemptHiddenApi()
    │         VMRuntime.setHiddenApiExemptions(["L"])
    │         豁免所有 Android 9+ 隐藏 API 限制
    │
    ├─[3] Ld.extractDexFiles(ctx)
│         检查 app_dex/v{versionCode}/ 缓存目录
│         ├─ 命中缓存：直接返回已落地的 DEX 列表
│         └─ 未命中：
│               ZipFile 读取 APK 中的 classes.dex → byte[]
│               JNI → Rust: q(ctx, dexData)
│                   ├─ 全文反向扫描 MSHD magic（严格一致性校验）
│                   ├─ 读取 DEXB v5 头（明文区）
│                   ├─ 当前签名指纹参与 HKDF-SHA256 派生 ChaCha20 密钥
│                   ├─ ChaCha20-Poly1305 解密（含 AEAD 校验）
│                   ├─ v5：回调 Ld.getSignatureSha256(ctx)
│                   │        → timing_safe_eq 比对签名指纹
│                   │        → 不匹配抛出 SecurityException
│                   └─ 逐个 Zstd 解压 → 落地到 app_dex/v{versionCode}/
│
    ├─[4] Ld.p(classLoader, dexPaths, optDirPath)
    │         JNI 层调用 DexPathList.addDexPath()（不受 hidden API 限制）
    │         返回 false 时降级到 Java 反射 addDexPath
    │         新增 elements 移到数组前端（app 类优先）
    │
    └─[5] makeRealApp(base.getClassLoader(), base)
              用 PathClassLoader（已含 app DEX）加载真实 Application
              反射调用 Application.attach(base)

StubApp.onCreate()
    ├─[6] replaceAppReferences(realApp)
    │         替换 ActivityThread.mInitialApplication
    │         替换 ActivityThread.mAllApplications 列表中的引用
    │         替换 LoadedApk.mApplication
    │
    ├─[7] realApp.onCreate()
    │
    └─[8] ARouterCompat.injectARouterRouteMap(this)（按需）
```

### 5.2 JNI 接口

native 方法通过 `JNI_OnLoad` 中的 `RegisterNatives` 动态绑定，Rust 函数符号不出现在 `.dynsym` 动态符号表，切断可读性。
绑定的类名和方法名在编译期由环境变量常量注入，构建时由 `build.rs` 解析 R8 `mapping.txt` 后生成：

| 编译期常量 | 说明 |
|---|---|
| `env!("STUB_BINLOADER_CLASS")` | R8 混淆后的壳类内部路径（如 `msk/b`） |
| `env!("STUB_METHOD_INJECT_DEX")` | R8 混淆后的 DEX 注入方法名（对应 Java `p`） |
| `env!("STUB_METHOD_EXTRACT_DECRYPT")` | R8 混淆后的 DEX 解密提取方法名（对应 Java `q`） |
| `env!("STUB_METHOD_GET_SIG")` | R8 混淆后的签名获取方法名（对应 Java `getSignatureSha256`，此方法被 keep 故不变） |

绑定的两个函数：

```
f1  ←→  Ld.p(ClassLoader classLoader, String[] dexPaths, String optDirPath) → boolean
        DEX 注入：通过 JNI 将解密后的 DEX 插入 PathClassLoader。
        成功返回 JNI_TRUE，失败返回 JNI_FALSE（Java 层降级到反射方案）。

f2  ←→  Ld.q(Context ctx, byte[] dexData) → byte[][]
        DEX 解密：从 classes.dex 末尾提取 MSHD payload，解密解压后返回各 DEX 字节数组。
        ctx 由调用方（attachBaseContext 阶段）显式传入，规避 ActivityThread.currentApplication()
        在 Application 初始化阶段返回 null 的问题。
```

**签名校验流程（v5 格式）：**
1. 解析 DEXB v5 头，读取 `expected_signature`
2. 调用 `Ld.getSignatureSha256(ctx)` 获取设备当前 APK 实际签名指纹
   - 使用传入的 `ctx`，不依赖 `ActivityThread.currentApplication()`（该阶段返回 null）
   - Android 9 及以上读取 `SigningInfo.getApkContentsSigners()`，旧系统读取 `PackageInfo.signatures`
   - 两条路径都要求当前内容签名证书恰好一个，并对 `Signature.toByteArray()` 的证书 DER 计算 SHA-256
3. `timing_safe_eq(expected, actual)` 常数时间比对，防时序攻击
4. 不匹配时抛出 `java.lang.RuntimeException`，中断加载

### 5.3 DEX 注入机制

**核心原则：不创建任何中间 ClassLoader，所有 app 类的 defining loader 始终是原始 PathClassLoader。**

优先路径（JNI，不受 hidden API 限制）：

```rust
// f1（Ld.p JNI 实现）
FindClass("dalvik/system/BaseDexClassLoader")
→ GetFieldID → pathList 字段
→ GetObjectField → pathList 对象
→ FindClass("dalvik/system/DexPathList")
→ GetMethodID → addDexPath(String, File)
→ CallVoidMethod 调用
```

降级路径（Java 反射，`p` 返回 false 时）：

```java
Method addDexPath = pathList.getClass().getDeclaredMethod("addDexPath", String.class, File.class);
addDexPath.setAccessible(true);
addDexPath.invoke(pathList, dexFile.getAbsolutePath(), optDir);
```

`addDexPath` 内部将 DEX 直接注册到 PathClassLoader（`definingContext`），天然避免 `multiple class loaders` 问题。

**兼容性：**
- `addDexPath(String, File)` 在 Android 7.0（API 24）~ 15（API 35）均存在
- API 26+ `optimizedDirectory` 参数被忽略（传 null）

### 5.4 ARouter 路由表补注册

| 方式 | 检测标志 | 处理 |
|------|---------|------|
| arouter-register Gradle plugin | `ARouter$$Root$$xxx` 在壳 DEX 中 | 已通过 plugin 静态注入，跳过补注册 |
| 运行时扫描 | 无上述类 | 静态扫描 DEX，找到所有路由表类，逐一反射调用 `loadInto()` |

---

## 六、与 360 加固的差异

> 分析样本：某公开可分析的第三方加固 APK。

| 维度 | 当前方案（Mocika Shield） | 360 加固（参考） |
|------|--------------------------|----------------|
| **加密 DEX 存储** | `classes.dex` 末尾追加 MSHD 块（工具不可见） | `classes.dex` 末尾追加（magic `71 68 00 01`） |
| **壳 SO 位置** | `lib/` 目录（系统自动加载） | `assets/`（手动 extract + dlopen） |
| **DEX 注入方式** | JNI 优先 + Java 反射降级 | Native 层直接操作 `dexElements` |
| **加密算法** | Zstd + ChaCha20-Poly1305 + HKDF-SHA256（AEAD） | 私有算法（native 混淆，不可见） |
| **隐藏 API 绕过** | JNI（不受限）+ Java `VMRuntime.setHiddenApiExemptions` 降级 | Native 层（JNI 不受限制） |
| **签名校验** | DEXB v5 头部记录指纹，参与密钥派生并 timing-safe 比对 | 有 |
| **安全检测** | Rust native 层反调试检测（TracerPid + Frida maps/线程名），检测到立即中止 | 内置反调试、Root、模拟器检测 |

---

## 七、加固前后 APK 结构对比

```
原始 APK：                          加固后 APK：
├─ classes.dex                     ├─ classes.dex  ← 壳 DEX + 末尾追加 MSHD 加密块
├─ classes2.dex                    │               （工具看不到追加数据，无 assets/app.bin）
├─ classes3.dex                    ├─ lib/
├─ assets/                         │   ├─ arm64-v8a/libmocikashield.so   ← 新增
│   └─ ...（原有资源）              │   ├─ armeabi-v7a/libmocikashield.so ← 新增
├─ lib/                            │   ├─ x86/libmocikashield.so         ← 新增
│   └─ arm64-v8a/...              │   └─ x86_64/libmocikashield.so      ← 新增
├─ AndroidManifest.xml             ├─ AndroidManifest.xml
│   Application=原始类             │   ├─ Application=msk.b（R8 混淆后的 StubApp，由 metadata.json 读取）
└─ res/                            │   └─ meta-data（ORIGINAL_APPLICATION）
                                   └─ res/
```

---

## 八、已知 Bug 与设计隐患

### Bug 1：Manifest 修改用字符串操作 ✅ 已修复

- **问题**：使用正则 + 字符串操作 XML，部分混淆 Manifest 可能修改失败
- **修复**：改用 `xmltree 0.10` 结构化修改

---

### 隐患 2：DEX 注入依赖 Android 内部 API ✅ 已缓解

- **位置**：`shield-stub/src/main/java/.../StubApp.java`
- **问题**：反射调用 `DexPathList.addDexPath()`，属于 `@UnsupportedAppUsage`
- **当前状态**：已通过 JNI 优先路径缓解（JNI 不受 hidden API 限制），Java 反射仅作为降级路径

---

### 隐患 3：XOR 无 KDF ✅ 已修复

- **问题**：XOR 直接使用原始 key，无派生、无随机性，存在已知明文攻击面
- **修复**：升级为 ChaCha20-Poly1305 + HKDF-SHA256，DEXB 格式升级至 v5

---

### 隐患 4：无签名校验 ✅ 已修复

- **问题**：无任何运行时安全检测，可被重打包攻击
- **修复**：DEXB v5 将签名指纹与随机 IKM 写入头部并绑定密钥派生，Rust Native 层执行 timing-safe 比对

---

### 隐患 5：签名降级返回固定字符串不报错 ✅ 已修复

- **问题**：所有提取路径失败后返回 `"UNSIGNED_OR_UNSUPPORTED"`，无任何警告
- **修复**：降级路径打印醒目 WARNING，提示用户先签名再加固

---

### Bug 6：ARouterCompat 缺少 ProGuard keep 规则 ✅ 已修复

- **问题**：R8 将 `ARouterCompat` 混淆为短类名（如 `a.a`），与被加固 app 的混淆产物冲突，ART 抛出 `InstantiationError`
- **修复**：`proguard-rules.pro` 中为 `dev.mocika.shield.loader` 包下所有类补充 `-keep class ... { *; }`
- **教训**：壳 DEX 中所有类均不应被 R8 重命名

---

### Bug 7：`ActivityThread.currentApplication()` 在 attachBaseContext 阶段返回 null ✅ 已修复

- **位置**：`shield-stub/src/main/rust/src/lib.rs`
- **问题**：签名校验 JNI 函数内部通过静态方法获取 Context，该阶段返回 null，导致 NPE 崩溃
- **修复**：`extractAndDecryptFromDex` 改为接受 `Context ctx` 参数，由 Java 层 `attachBaseContext` 显式传入
- **教训**：`attachBaseContext` 阶段凡需要 Context 的 JNI 函数，必须由 Java 层显式传入

---

### Bug 8：ProGuard 逐条列举 JNI 回调方法导致 R8 删除方法 ✅ 已修复

- **位置**：`shield-stub/proguard-rules.pro`
- **问题**：`getSignatureSha256` 仅被 JNI Native 调用，R8 无法感知，判定为死代码删除
- **修复**：`Ld` 改为 `-keep class ... { *; }` 全保留
- **教训**：任何被 JNI 调用的类，必须用 `{ *; }` 全保留，不能逐条列举

---

### 代码健壮性修复 ✅ 已修复

- `payload.len() as u32` 改为 `u32::try_from()`，超 4GiB 时报错而非截断
- MSHD 扫描改为严格一致性校验（magic + payload_len + 文件末尾三者吻合），消除误命中
- 重复加固前裁剪 DEX 至 `file_size` 边界，确保只有一份 MSHD
- `dex_count` 上限 256，`payload_len` 上限 512 MiB
- DEX 文件数量 / 字节数组长度溢出防护（`i32::try_from()`）
- 缓存写入后创建 `.done` 标记，校验时检查标记，写入成功后再删旧缓存

---

## 九、后续迭代方向

待实现功能及进度见 [docs/process/roadmap.md](../process/roadmap.md)。

以下为纯 Android 端的低优先级技术方向，暂无计划：

| 项目 | 说明 |
|------|------|
| SO 移到 assets + 手动 dlopen | 加载前做完整性校验，防 SO 替换攻击 |
| 加入 baseline.prof | 加速 ART 首次编译 |

---

*最后更新：2026-04-17*
