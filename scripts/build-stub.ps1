# Mocika Shield - Runtime 库构建脚本（PowerShell）
# 用途：生成 stub-classes.dex 和 Native 库资源包
#
# 用法:
#   .\scripts\build-stub.ps1
#   .\scripts\build-stub.ps1 -ShieldVersion 1.2.3

param(
    [string]$ShieldVersion = $env:SHIELD_VERSION
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$OutputDir   = Join-Path $ProjectRoot "shield-stub\build\outputs\resources"

if (-not $ShieldVersion) { $ShieldVersion = "1.0.0" }

function Info($msg)    { Write-Host "==> $msg" -ForegroundColor Blue }
function Success($msg) { Write-Host "✓ $msg"   -ForegroundColor Green }
function Warn($msg)    { Write-Host "⚠ $msg"   -ForegroundColor Yellow }
function Err($msg)     { Write-Host "✗ $msg"   -ForegroundColor Red; exit 1 }

function Run($cmd, $argList) {
    & $cmd @argList
    if ($LASTEXITCODE -ne 0) { Err "命令失败: $cmd $($argList -join ' ')" }
}

# 从 mapping.txt 提取原始类名对应的混淆类名
# mapping.txt 格式：原始类名 -> 混淆类名:
function Parse-MappingClass($originalClass, $mappingFile) {
    $escaped = [regex]::Escape($originalClass)
    $line = Get-Content $mappingFile | Where-Object { $_ -match "^$escaped ->" } | Select-Object -First 1
    if ($line -match " -> (.+?):\s*$") { return $matches[1] }
    if ($line -match " -> (.+)$")       { return $matches[1].TrimEnd(':').Trim() }
    return $null
}

# 从 mapping.txt 提取原始方法名在指定类内的混淆方法名
function Parse-MappingMethod($originalClass, $originalMethod, $mappingFile) {
    $lines   = Get-Content $mappingFile
    $inClass = $false
    $escaped = [regex]::Escape($originalClass)
    foreach ($line in $lines) {
        if ($line -match "^$escaped ->") { $inClass = $true; continue }
        if ($inClass) {
            if ($line -match "^#") { continue }    # 跳过 R8 注释行（# {"id":...}），不能 break
            if ($line -notmatch "^\s") { break }   # 遇到下一个类块（非空白、非注释）才停止
            if ($line -match " $([regex]::Escape($originalMethod))\(") {
                if ($line -match "-> (\S+)\s*$") { return $matches[1] }
            }
        }
    }
    return $null
}

Write-Host "================================================" -ForegroundColor Green
Write-Host "Mocika Shield - shield-stub 构建" -ForegroundColor Green
Write-Host "================================================" -ForegroundColor Green
Write-Host ""

# 检查项目根目录
Push-Location $ProjectRoot
try {

if (-not (Test-Path "Cargo.toml") -or -not (Test-Path "shield-stub")) {
    Err "请在项目根目录运行此脚本"
}

Info "检查必要工具..."

if (-not (Get-Command java -ErrorAction SilentlyContinue)) {
    Err "未找到 Java，请安装 JDK 17+`n  下载: https://adoptium.net/"
}

if (-not (Test-Path "shield-stub\gradlew.bat") -and -not (Test-Path "shield-stub\gradlew")) {
    Err "未找到 shield-stub\gradlew 或 gradlew.bat"
}

# Android SDK
$androidSdk = $env:ANDROID_HOME
if (-not $androidSdk) { $androidSdk = $env:ANDROID_SDK_ROOT }
if (-not $androidSdk -or -not (Test-Path $androidSdk)) {
    Err "未设置 ANDROID_HOME 或 ANDROID_SDK_ROOT 环境变量`n  请安装 Android SDK 并设置环境变量"
}
Success "Android SDK: $androidSdk"

# Android NDK 29.0.14206865
$ndkVersion = "29.0.14206865"
$pinnedNdkPath = Join-Path $androidSdk "ndk\$ndkVersion"
if (Test-Path $pinnedNdkPath) {
    $ndkPath = $pinnedNdkPath
} else {
    $ndkPath = $env:ANDROID_NDK_ROOT
    if (-not $ndkPath) { $ndkPath = $env:NDK_HOME }
    if (-not $ndkPath) { $ndkPath = $pinnedNdkPath }
}
if (-not (Test-Path $ndkPath)) {
    Err "未找到 Android NDK $ndkVersion`n  期望路径: $ndkPath`n  请通过 Android Studio SDK Manager 安装 NDK $ndkVersion`n  或设置: `$env:ANDROID_NDK_ROOT = '<NDK路径>'"
}
Success "Android NDK: $ndkPath"

# Rust 工具链
if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
    Err "未找到 rustup，请先安装 Rust 工具链`n  下载: https://rustup.rs/"
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Err "未找到 cargo，请先安装 Rust 工具链`n  下载: https://rustup.rs/"
}
Success "Rust 工具链: 已安装"

# cargo-ndk
if (-not (Get-Command cargo-ndk -ErrorAction SilentlyContinue)) {
    Err "未安装 cargo-ndk`n  请运行: cargo install cargo-ndk"
}
Success "cargo-ndk: 已安装"

# Android Rust 目标架构
$androidTargets = @(
    "aarch64-linux-android",
    "armv7-linux-androideabi",
    "i686-linux-android",
    "x86_64-linux-android"
)
$installedTargets = (rustup target list --installed 2>$null) -join "`n"
foreach ($t in $androidTargets) {
    if ($installedTargets -notmatch "(?m)^$([regex]::Escape($t))$") {
        Warn "未安装 Android Rust target $t，正在安装..."
        Run "rustup" @("target", "add", $t)
    }
}
Success "Android Rust targets: 全部就绪"

New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
Success "创建输出目录: $OutputDir"
Write-Host ""

# ==========================================
# 步骤 1: 构建 shield-stub AAR（含 R8 混淆，Rust Native 库由 Gradle 触发）
# ==========================================
Info "步骤1: 构建 shield-stub AAR（含 R8 混淆）..."
Write-Host "  架构: arm64-v8a, armeabi-v7a, x86, x86_64" -ForegroundColor Yellow

$gradlew = if (Test-Path (Join-Path $ProjectRoot "shield-stub\gradlew.bat")) {
    Join-Path $ProjectRoot "shield-stub\gradlew.bat"
} else {
    Join-Path $ProjectRoot "shield-stub\gradlew"
}
Run $gradlew @("--no-daemon", "-p", "shield-stub", "assembleRelease")

$aarFile = Join-Path $ProjectRoot "shield-stub\build\outputs\aar\shield-stub-release.aar"
if (-not (Test-Path $aarFile)) { Err "未找到 AAR 文件: $aarFile" }
Success "AAR 构建完成: $aarFile"
Write-Host ""

# ==========================================
# 步骤 2: 解析 R8 mapping，提取混淆后的类名/方法名
# ==========================================
Info "步骤2: 解析 R8 mapping，提取混淆后的类名/方法名..."

$mappingFile = Join-Path $ProjectRoot "shield-stub\build\outputs\mapping\release\mapping.txt"
if (-not (Test-Path $mappingFile)) {
    Err "未找到 R8 mapping 文件: $mappingFile`n  请确认 proguard-rules.pro 已启用混淆且 R8 正常运行"
}
Success "R8 mapping: $mappingFile"

$origBinLoader = "dev.mocika.shield.loader.Ld"
$origStubApp   = "dev.mocika.shield.loader.StubApp"
$origStubFactory = "dev.mocika.shield.loader.StubComponentFactory"

$obfBinLoader = Parse-MappingClass $origBinLoader $mappingFile
$obfStubApp   = Parse-MappingClass $origStubApp   $mappingFile
$obfStubFactory = Parse-MappingClass $origStubFactory $mappingFile

if (-not $obfBinLoader -or -not $obfStubApp -or -not $obfStubFactory) {
    Err "无法从 mapping.txt 提取混淆类名`n  Ld: '$obfBinLoader'  StubApp: '$obfStubApp'  StubFactory: '$obfStubFactory'`n  请检查 proguard-rules.pro 是否正确配置了 allowobfuscation"
}

$obfMethodInject  = Parse-MappingMethod $origBinLoader "p"                 $mappingFile
$obfMethodExtract = Parse-MappingMethod $origBinLoader "q"                 $mappingFile
$obfMethodCheckEnv = Parse-MappingMethod $origBinLoader "r"                $mappingFile
$obfMethodGetSig  = Parse-MappingMethod $origBinLoader "getSignatureSha256" $mappingFile

# 方法名解析失败时保留原名（R8 可能因覆盖关系保留原方法名）
if (-not $obfMethodInject)  { $obfMethodInject  = "p" }
if (-not $obfMethodExtract) { $obfMethodExtract = "q" }
if (-not $obfMethodCheckEnv) { $obfMethodCheckEnv = "r" }
if (-not $obfMethodGetSig)  { $obfMethodGetSig  = "getSignatureSha256" }

# JVM 内部路径格式（点 → 斜线）
$obfBinLoaderJvm = $obfBinLoader.Replace(".", "/")

Write-Host "  Ld:      $origBinLoader → $obfBinLoader" -ForegroundColor Green
Write-Host "  StubApp: $origStubApp → $obfStubApp" -ForegroundColor Green
Write-Host "  StubFactory: $origStubFactory → $obfStubFactory" -ForegroundColor Green
Write-Host "  方法: p→$obfMethodInject, q→$obfMethodExtract, r→$obfMethodCheckEnv, getSignatureSha256→$obfMethodGetSig" -ForegroundColor Green
Write-Host ""

# ==========================================
# 步骤 3: 使用混淆后的类名/方法名重新编译 Rust Native 库
# ==========================================
Info "步骤3: 使用混淆后的类名/方法名重新编译 Rust Native 库..."

$rustSrc = Join-Path $ProjectRoot "shield-stub\src\main\rust"

# VBOXSF 大文件写入限制：Rust 链接器（lld）通过 mmap 写 .so 到 VBOXSF 共享目录时
# 产生大小正确但内容全零的文件（VirtualBox Guest Additions 已知缺陷）。
# 解决方案：CARGO_TARGET_DIR 指向本地磁盘（C:\Temp\mocika-shield-rust-target），
# 链接器写到本地 NTFS 后，再 Copy-Item（LOCAL→VBOXSF 跨卷传输，数据完整）到 OutputDir。
$localTargetDir = Join-Path $env:TEMP "mocika-shield-rust-target"
New-Item -ItemType Directory -Path $localTargetDir -Force | Out-Null
Write-Host "  Rust 本地编译缓存: $localTargetDir" -ForegroundColor Yellow

# 设置环境变量（build.rs 通过这些变量把混淆后的符号名嵌入 .so）
$env:STUB_BINLOADER_CLASS        = $obfBinLoaderJvm
$env:STUB_METHOD_INJECT_DEX      = $obfMethodInject
$env:STUB_METHOD_EXTRACT_DECRYPT = $obfMethodExtract
$env:STUB_METHOD_CHECK_ENV        = $obfMethodCheckEnv
$env:STUB_METHOD_GET_SIG         = $obfMethodGetSig
$env:ANDROID_NDK_ROOT            = $ndkPath
$env:CARGO_TARGET_DIR            = $localTargetDir   # 强制输出到本地磁盘

# 消除 .so 字符串段中的本机路径信息
$userHome = $env:USERPROFILE
if (-not $userHome) { $userHome = $env:HOMEPATH }
$env:RUSTFLAGS = "--remap-path-prefix `"${ProjectRoot}`"=. --remap-path-prefix `"${userHome}\.cargo`"=.cargo --remap-path-prefix `"${userHome}\.rustup`"=.rustup"

Push-Location $rustSrc
try {
    Run "cargo" @("ndk",
        "-t", "arm64-v8a",
        "-t", "armeabi-v7a",
        "-t", "x86",
        "-t", "x86_64",
        "-o", "..\..\..\build\jniLibs",
        "build", "--release")
} finally {
    Pop-Location
    # 清理临时环境变量（CARGO_TARGET_DIR 不清理，步骤4 通过 $localTargetDir 变量读取）
    Remove-Item Env:STUB_BINLOADER_CLASS        -ErrorAction SilentlyContinue
    Remove-Item Env:STUB_METHOD_INJECT_DEX      -ErrorAction SilentlyContinue
    Remove-Item Env:STUB_METHOD_EXTRACT_DECRYPT -ErrorAction SilentlyContinue
    Remove-Item Env:STUB_METHOD_CHECK_ENV        -ErrorAction SilentlyContinue
    Remove-Item Env:STUB_METHOD_GET_SIG         -ErrorAction SilentlyContinue
    Remove-Item Env:RUSTFLAGS                   -ErrorAction SilentlyContinue
    Remove-Item Env:CARGO_TARGET_DIR            -ErrorAction SilentlyContinue
}

Success "Rust 二次编译完成（.so 字符串已使用混淆名）"
Write-Host ""

# ==========================================
# 步骤 4: 从 AAR 提取 classes.jar → stub-classes.dex，并复制混淆版 .so
# ==========================================
Info "步骤4: 提取资源文件..."

$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())
New-Item -ItemType Directory -Path $tempDir | Out-Null
Write-Host "  临时目录: $tempDir" -ForegroundColor Yellow

try {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.IO.Compression.ZipFile]::ExtractToDirectory($aarFile, $tempDir)

    $classesJar = Join-Path $tempDir "classes.jar"
    if (-not (Test-Path $classesJar)) { Err "AAR 中未找到 classes.jar" }

    # 找最新 build-tools 版本（版本号数字排序）
    $buildToolsDir = Join-Path $androidSdk "build-tools"
    $latestBuildTools = Get-ChildItem $buildToolsDir -Directory |
        Sort-Object { [version]($_.Name -replace '[^0-9.]', '') } |
        Select-Object -Last 1
    if (-not $latestBuildTools) { Err "未找到 build-tools，请通过 SDK Manager 安装" }

    $d8 = Join-Path $latestBuildTools.FullName "d8.bat"
    if (-not (Test-Path $d8)) { Err "未找到 d8 工具: $d8" }
    Write-Host "  使用 d8: $d8" -ForegroundColor Yellow

    # 查找 android.jar
    $androidJar = ""
    $platformsDir = Join-Path $androidSdk "platforms"
    if (Test-Path $platformsDir) {
        $latestPlatform = Get-ChildItem $platformsDir -Directory |
            Where-Object { $_.Name -match "^android-\d+$" } |
            Sort-Object { [int]($_.Name -replace "android-", "") } |
            Select-Object -Last 1
        if ($latestPlatform) {
            $candidate = Join-Path $latestPlatform.FullName "android.jar"
            if (Test-Path $candidate) { $androidJar = $candidate }
        }
    }

    if ($androidJar) {
        Write-Host "  使用 android.jar: $androidJar" -ForegroundColor Yellow
        Run $d8 @("--output", $OutputDir, $classesJar, "--min-api", "21", "--lib", $androidJar)
    } else {
        Warn "未找到 android.jar，desugaring 可能产生警告"
        Run $d8 @("--output", $OutputDir, $classesJar, "--min-api", "21")
    }

    $classesDex = Join-Path $OutputDir "classes.dex"
    $stubDex    = Join-Path $OutputDir "stub-classes.dex"
    if (-not (Test-Path $classesDex)) { Err "DEX 文件生成失败" }
    Move-Item $classesDex $stubDex -Force
    Success "stub-classes.dex 生成成功"

    # 使用步骤3重新编译的 .so（JNI 符号已使用混淆名），不使用 AAR 内的旧版本。
    # 从本地磁盘（$localTargetDir，NTFS）Copy-Item 到 OutputDir\lib（VBOXSF）：
    # 跨卷复制不触发 VirtualBox server-side copy 快捷，数据完整传输。
    Write-Host "  复制混淆版 Native 库..." -ForegroundColor Yellow
    $libOutput = Join-Path $OutputDir "lib"
    New-Item -ItemType Directory -Path $libOutput -Force | Out-Null

    $archTriples = @{
        "arm64-v8a"   = "aarch64-linux-android"
        "armeabi-v7a" = "armv7-linux-androideabi"
        "x86"         = "i686-linux-android"
        "x86_64"      = "x86_64-linux-android"
    }
    foreach ($arch in @("arm64-v8a", "armeabi-v7a", "x86", "x86_64")) {
        $triple = $archTriples[$arch]
        # 从本地 NTFS 读取（步骤3 CARGO_TARGET_DIR=$localTargetDir，链接器输出在此）
        $soSrc = Join-Path $localTargetDir "$triple\release\libmocikashield.so"
        if (Test-Path $soSrc) {
            $soSize = (Get-Item $soSrc).Length
            if ($soSize -lt 100000) {
                Err "  $arch/libmocikashield.so 大小异常（$soSize bytes）：本地编译产物可能为空"
            }
            $dstDir = Join-Path $libOutput $arch
            New-Item -ItemType Directory -Path $dstDir -Force | Out-Null
            # LOCAL (NTFS C:\Temp) → VBOXSF (E:\)：跨卷 CopyFile，数据完整传输
            Copy-Item $soSrc (Join-Path $dstDir "libmocikashield.so") -Force
            Success "  $arch/libmocikashield.so ($([int]($soSize / 1024)) KB)"
        } else {
            Warn "  $($arch): $soSrc 未找到（步骤3 可能未编译此架构）"
        }
    }
} finally {
    Remove-Item $tempDir -Recurse -Force -ErrorAction SilentlyContinue
}

Success "资源提取完成"
Write-Host ""

# ==========================================
# 步骤 5: 生成 metadata.json（含混淆后的类名）
# ==========================================
Info "步骤5: 生成 metadata.json..."

$buildDate = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
@"
{
  "version": "$ShieldVersion",
  "build_date": "$buildDate",
  "stub_dex": "stub-classes.dex",
  "stub_application": "$obfStubApp",
  "compression": "Zstd (level 19)",
  "supported_modes": [
    "traditional",
    "bin"
  ],
  "supported_architectures": [
    "arm64-v8a",
    "armeabi-v7a",
    "x86",
    "x86_64"
  ],
  "min_android_api": 21,
  "target_android_api": 35,
  "native_library": "libmocikashield.so",
  "native_name_placeholder": "mocikanativeslot",
  "native_name_length": 16,
  "native_name_scheme": 1,
  "runtime_protocol": 2,
  "cache_schema": 1,
  "environment_policy": true,
  "memory_dex": false
}
"@ | Set-Content (Join-Path $OutputDir "metadata.json") -Encoding UTF8

Success "metadata.json 生成完成（stub_application: $obfStubApp）"
Write-Host ""

# ==========================================
# 步骤 6: 创建资源包 ZIP
# ==========================================
# 根本原因：步骤4 原用 Copy-Item 在 VBOXSF 内部（E:\→E:\）复制 .so，
# VirtualBox 共享文件夹驱动将其作服务端快捷复制，导致文件大小正确但内容全为零。
# 已在步骤4 改用 ReadAllBytes→WriteAllBytes，OutputDir\lib 现在有真实内容。
# 步骤6 直接用 Compress-Archive 打包即可，无需外部工具。
Info "步骤6: 创建资源包..."

$resourcesZipName = "mocika-runtime-resources-$ShieldVersion.zip"
$resourcesZip     = Join-Path $OutputDir $resourcesZipName
$resourcesZipLink = Join-Path $OutputDir "resources.zip"

if (Test-Path $resourcesZip)     { Remove-Item $resourcesZip     -Force }
if (Test-Path $resourcesZipLink) { Remove-Item $resourcesZipLink -Force }

$filesToPack = @(
    (Join-Path $OutputDir "stub-classes.dex"),
    (Join-Path $OutputDir "lib"),
    (Join-Path $OutputDir "metadata.json")
)
Compress-Archive -Path $filesToPack -DestinationPath $resourcesZip
Copy-Item $resourcesZip $resourcesZipLink -Force

$sizeMB = "{0:N1} MB" -f ((Get-Item $resourcesZip).Length / 1MB)
$sizeBytes = (Get-Item $resourcesZip).Length
if ($sizeBytes -lt 500000) {
    Err "resources.zip 大小异常（$sizeBytes bytes < 500KB）：.so 内容可能仍为零，请检查步骤4输出"
}

$sizeMB = "{0:N1} MB" -f ((Get-Item $resourcesZip).Length / 1MB)
Success "资源包创建成功"
Write-Host "  文件: $resourcesZip" -ForegroundColor Green
Write-Host "  大小: $sizeMB" -ForegroundColor Green
Success "resources.zip 已更新"

# 候选资源只通过显式资源路径用于内部回归，不参与自动资源发现。
$standardMetadata = Get-Content (Join-Path $OutputDir "metadata.json") -Raw
@"
{
  "version": "$ShieldVersion",
  "build_date": "$buildDate",
  "stub_dex": "stub-classes.dex",
  "stub_application": "$obfStubApp",
  "stub_component_factory": "$obfStubFactory",
  "supported_architectures": ["arm64-v8a", "armeabi-v7a", "x86", "x86_64"],
  "min_android_api": 29,
  "target_android_api": 35,
  "native_library": "libmocikashield.so",
  "native_name_placeholder": "mocikanativeslot",
  "native_name_length": 16,
  "native_name_scheme": 1,
  "runtime_protocol": 3,
  "cache_schema": 1,
  "environment_policy": true,
  "memory_dex": true,
  "memory_dex_min_api": 29
}
"@ | Set-Content (Join-Path $OutputDir "metadata.json") -Encoding UTF8
$memoryResources = Join-Path $OutputDir "resources-memory.zip"
if (Test-Path $memoryResources) { Remove-Item $memoryResources -Force }
Compress-Archive -Path $filesToPack -DestinationPath $memoryResources
$standardMetadata | Set-Content (Join-Path $OutputDir "metadata.json") -Encoding UTF8
Success "内存 DEX 候选资源: $memoryResources"

Write-Host ""
Write-Host "================================================" -ForegroundColor Green
Write-Host "构建完成！" -ForegroundColor Green
Write-Host "================================================" -ForegroundColor Green
Write-Host ""
Write-Host "资源包位置:" -ForegroundColor Blue
Write-Host "  $resourcesZip"
Write-Host "  $resourcesZipLink"
Write-Host ""
Write-Host "包含内容:" -ForegroundColor Blue
Write-Host "  ✓ stub-classes.dex（运行时 Java 类，R8 已混淆）"
Write-Host "  ✓ lib\*\libmocikashield.so（Native 库，JNI 符号已使用混淆名）"
Write-Host "  ✓ metadata.json（元数据，含混淆后的壳类名）"
Write-Host ""
Write-Host "下一步:" -ForegroundColor Yellow
Write-Host "  使用 shield-cli 加固 APK"
Write-Host "  示例: .\target\release\shield.exe -i input.apk -o output.apk"
Write-Host ""

} finally {
    Pop-Location
}
