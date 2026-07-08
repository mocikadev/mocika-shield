# Windows 平台发布脚本（PowerShell）
# 生成：CLI（exe + zip）、GUI（NSIS .exe 安装包）
#
# 用法:
#   .\scripts\release-windows.ps1
#   .\scripts\release-windows.ps1 -Version 1.2.3
#
# 前置要求:
#   rustup target add x86_64-pc-windows-msvc
#   安装 Visual Studio Build Tools 2022（含 MSVC + Windows SDK）
#   安装 NSIS: https://nsis.sourceforge.io/
#   cargo install tauri-cli
#
# 注意: Tauri v2 GUI 不支持从 Linux/macOS 交叉编译到 Windows，必须在 Windows 原生运行

param(
    [string]$Version = $env:VERSION
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Root      = Split-Path -Parent $PSScriptRoot
$Arch      = "x86_64"
$DistDir   = Join-Path $Root "dist\windows"

if (-not $Version) { $Version = "1.0.0" }

# ========== 输出工具 ==========
function Info($msg)    { Write-Host "==> $msg" -ForegroundColor Blue }
function Success($msg) { Write-Host "✓ $msg"   -ForegroundColor Green }
function Warn($msg)    { Write-Host "⚠ $msg"   -ForegroundColor Yellow }
function Err($msg)     { Write-Host "✗ $msg"   -ForegroundColor Red; exit 1 }

function Run($cmd, $argList) {
    & $cmd @argList
    if ($LASTEXITCODE -ne 0) { Err "命令失败: $cmd $($argList -join ' ')" }
}

# ========== 检查运行环境 ==========
function Check-Env {
    Info "检查运行环境..."

    # Rust / cargo
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Err "Rust 未安装，请访问 https://rustup.rs/ 安装"
    }

    # Java（shield-stub 构建需要）
    if (-not (Get-Command java -ErrorAction SilentlyContinue)) {
        Err "未安装 Java，shield-stub 构建需要 Java 17+`n  下载: https://adoptium.net/"
    }

    # x86_64-pc-windows-msvc Rust target
    $targets = (rustup target list --installed 2>$null) -join "`n"
    if ($targets -notmatch "(?m)^x86_64-pc-windows-msvc$") {
        Warn "未安装 Windows MSVC 目标，正在安装..."
        Run "rustup" @("target", "add", "x86_64-pc-windows-msvc")
    }

    # MSVC 链接器（通过 cl.exe / link.exe 判断）
    $vsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    $hasVs = $false
    if (Test-Path $vsWhere) {
        $vsPath = & $vsWhere -latest -products * -requires Microsoft.VisualCpp.Tools.HostX64.TargetX64 -property installationPath 2>$null
        $hasVs = ($vsPath -ne $null -and $vsPath -ne "")
    }
    if (-not $hasVs) {
        # 回退：直接找 link.exe
        $hasVs = $null -ne (Get-Command link.exe -ErrorAction SilentlyContinue)
    }
    if (-not $hasVs) {
        Err "未检测到 MSVC 链接器（link.exe）。`n  请安装 Visual Studio Build Tools 2022 并勾选「使用 C++ 的桌面开发」:`n  https://visualstudio.microsoft.com/visual-cpp-build-tools/"
    }

    # NSIS（Tauri GUI bundle 需要）
    $nsisMake = Get-Command makensis -ErrorAction SilentlyContinue
    if (-not $nsisMake) {
        # 常见安装路径
        $nsisPath = @(
            "$env:ProgramFiles\NSIS\makensis.exe",
            "${env:ProgramFiles(x86)}\NSIS\makensis.exe"
        ) | Where-Object { Test-Path $_ } | Select-Object -First 1
        if (-not $nsisPath) {
            Err "未安装 NSIS，GUI 打包需要。`n  下载: https://nsis.sourceforge.io/Download`n  安装后确保 makensis.exe 在 PATH 中"
        }
    }

    # cargo-tauri（GUI 构建需要）
    $hasTauri = (Get-Command cargo-tauri -ErrorAction SilentlyContinue) -or
                ((cargo tauri --version 2>$null) -and $LASTEXITCODE -eq 0)
    if (-not $hasTauri) {
        Warn "未安装 tauri-cli，正在安装（需要网络，请稍候）..."
        Run "cargo" @("install", "tauri-cli")
    }

    # Node/npm（Tauri React 前端构建需要）
    if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
        Err "未安装 Node.js/npm，GUI 前端构建需要。`n  下载: https://nodejs.org/"
    }

    # Android SDK
    $androidSdk = $env:ANDROID_HOME
    if (-not $androidSdk) { $androidSdk = $env:ANDROID_SDK_ROOT }
    if (-not $androidSdk -or -not (Test-Path $androidSdk)) {
        Err "未设置 ANDROID_HOME 或 ANDROID_SDK_ROOT，shield-stub 构建需要 Android SDK。`n  请安装 Android Studio 或命令行工具: https://developer.android.com/studio#command-tools`n  安装后设置环境变量: `$env:ANDROID_HOME = 'C:\Users\<你>\AppData\Local\Android\Sdk'"
    }
    Write-Host "  Android SDK: $androidSdk"

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
        Err "未找到 Android NDK $ndkVersion。`n  期望路径: $ndkPath`n  请通过 Android Studio SDK Manager 安装 NDK $ndkVersion`n  或设置: `$env:ANDROID_NDK_ROOT = '<NDK路径>'"
    }
    Write-Host "  Android NDK: $ndkPath"

    # cargo-ndk（shield-stub Rust 交叉编译需要）
    if (-not (Get-Command cargo-ndk -ErrorAction SilentlyContinue)) {
        Warn "未安装 cargo-ndk，正在安装（需要网络，请稍候）..."
        Run "cargo" @("install", "cargo-ndk")
    }

    # Android Rust 目标架构
    $androidTargets = @(
        "aarch64-linux-android",
        "armv7-linux-androideabi",
        "i686-linux-android",
        "x86_64-linux-android"
    )
    foreach ($t in $androidTargets) {
        if ($targets -notmatch "(?m)^$([regex]::Escape($t))$") {
            Warn "未安装 Android Rust target $t，正在安装..."
            Run "rustup" @("target", "add", $t)
        }
    }
    Write-Host "  Android Rust targets: 全部就绪"

    Success "环境检查通过"
    Write-Host "  Rust:  $(rustc --version)"
    Write-Host "  版本:  $Version"
}

# ========== 构建 shield-stub（resources.zip）==========
function Build-Stub {
    Info "构建 shield-stub（resources.zip）..."
    Push-Location $Root
    try {
        & "scripts\build-stub.ps1" -ShieldVersion $Version
        if ($LASTEXITCODE -ne 0) { Err "shield-stub 构建失败" }
    } finally {
        Pop-Location
    }
    $zip = Join-Path $Root "shield-stub\build\outputs\resources\resources.zip"
    if (-not (Test-Path $zip)) { Err "shield-stub 构建失败，resources.zip 未生成" }
    Success "shield-stub 构建完成"
}

# ========== 构建 CLI ==========
function Build-Cli {
    Info "构建 CLI（x86_64-pc-windows-msvc）..."
    Push-Location $Root
    try {
        Run cargo @("build", "--release",
            "-p", "shield-cli",
            "--target", "x86_64-pc-windows-msvc")
    } finally {
        Pop-Location
    }
    $bin = Join-Path $Root "target\x86_64-pc-windows-msvc\release\shield.exe"
    if (-not (Test-Path $bin)) { Err "CLI 构建失败，产物不存在: $bin" }
    Success "CLI 构建完成"
    Get-Item $bin | Select-Object Name, Length
}

# ========== 构建 GUI（Tauri NSIS bundle）==========
function Build-Gui {
    Info "构建 GUI（cargo tauri build --bundles nsis）..."
    Push-Location (Join-Path $Root "apps\shield-gui")
    try {
        Run npm @("ci")
        Run cargo @("tauri", "build", "--bundles", "nsis")
    } finally {
        Pop-Location
    }
    Success "GUI 构建完成"
}

# ========== 准备输出目录 ==========
function Prepare-Dirs {
    Info "准备输出目录..."
    if (Test-Path $DistDir) { Remove-Item $DistDir -Recurse -Force }
    New-Item -ItemType Directory -Path "$DistDir\cli","$DistDir\gui-nsis" | Out-Null
    Success "目录准备完成: $DistDir"
}

# ========== 打包 CLI zip ==========
function Package-Cli {
    Info "打包 CLI..."

    $pkgName = "mocika-shield-cli-$Version-windows-$Arch"
    $pkgDir  = Join-Path $DistDir "cli\$pkgName"
    New-Item -ItemType Directory -Path "$pkgDir\bin","$pkgDir\lib","$pkgDir\resources" | Out-Null

    Copy-Item (Join-Path $Root "target\x86_64-pc-windows-msvc\release\shield.exe") "$pkgDir\bin\shield.exe"

    $apktool = Join-Path $Root "tools\apktool_3.0.1.jar"
    if (Test-Path $apktool) {
        Copy-Item $apktool "$pkgDir\lib\apktool.jar"
    } else {
        Warn "apktool.jar 未找到，请手动复制到 tools\ 目录"
    }

    $apksigner = Join-Path $Root "tools\apksigner.jar"
    if (Test-Path $apksigner) {
        Copy-Item $apksigner "$pkgDir\lib\apksigner.jar"
    } else {
        Warn "apksigner.jar 未找到，请手动复制到 tools\ 目录"
    }

    Copy-Item (Join-Path $Root "shield-stub\build\outputs\resources\resources.zip") `
        "$pkgDir\resources\resources.zip"

    @"
# Mocika Shield CLI v$Version

Windows x86_64 版本。

## 使用方法

在命令提示符或 PowerShell 中：

``````
bin\shield.exe -i input.apk -o protected.apk
``````

## 要求

- Windows 10/11 (64-bit)
- Java 17+（需完整 JDK，`java` / `javac` / `keytool` 须在 PATH 中）

## 目录结构

``````
mocika-shield-cli-$Version-windows-x86_64\
├── bin\shield.exe      # 可执行文件
├── lib\
│   ├── apktool.jar
│   └── apksigner.jar
├── resources\
│   └── resources.zip   # Android 壳资源
└── README.md
``````
"@ | Set-Content "$pkgDir\README.md" -Encoding UTF8

    $zipPath = Join-Path $DistDir "cli\$pkgName.zip"
    Compress-Archive -Path $pkgDir -DestinationPath $zipPath -Force
    Remove-Item $pkgDir -Recurse -Force

    Success "CLI zip 打包完成"
    Get-Item $zipPath | Select-Object Name, @{N="Size";E={"{0:N0} KB" -f ($_.Length/1KB)}}
}

# ========== 收集 GUI 产物 ==========
function Collect-Gui {
    Info "收集 GUI 产物..."

    $tauriTarget = Join-Path $Root "apps\shield-gui\src-tauri\target\release\bundle"
    $nsisDir     = Join-Path $tauriTarget "nsis"

    $exe = Get-ChildItem $nsisDir -Filter "*.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($exe) {
        $targetName = "MocikaShield_${Version}_windows_x64_setup.exe"
        Copy-Item $exe.FullName "$DistDir\gui-nsis\$targetName"
        Success "NSIS 安装包 → $targetName"
    } else {
        Warn "NSIS 安装包未找到，请确保已安装 NSIS 并配置 PATH"
    }
}

# ========== 生成校验和 ==========
function Generate-Checksums {
    Info "生成 SHA256 校验和..."
    Push-Location $DistDir
    try {
        $sha256 = [System.Security.Cryptography.SHA256]::Create()
        $lines = Get-ChildItem -Recurse -Include "*.zip","*.exe" |
            ForEach-Object {
                $bytes  = [System.IO.File]::ReadAllBytes($_.FullName)
                $hash   = ($sha256.ComputeHash($bytes) | ForEach-Object { $_.ToString("x2") }) -join ""
                $rel    = $_.FullName.Substring($DistDir.TrimEnd('\').Length + 1).Replace("\", "/")
                "$hash  ./$rel"
            } | Sort-Object
        $sha256.Dispose()
        $lines | Set-Content "checksums-sha256.txt" -Encoding UTF8
        Success "校验和已写入 checksums-sha256.txt"
        $lines | Write-Host
    } finally {
        Pop-Location
    }
}

# ========== 显示结果 ==========
function Show-Results {
    Write-Host ""
    Info "Windows 发布构建完成！v$Version"
    Write-Host ""
    Write-Host "📁 $DistDir\"
    Get-ChildItem $DistDir -Recurse -File | Sort-Object FullName | ForEach-Object {
        $size = "{0:N0} KB" -f ($_.Length / 1KB)
        Write-Host "  📄 $($_.FullName.Substring($DistDir.Length + 1)) ($size)"
    }
    Write-Host ""
    Write-Host "📦 CLI（zip，解压即用）:"
    $cliZip = Get-ChildItem "$DistDir\cli" -Filter "*.zip" -ErrorAction SilentlyContinue
    if ($cliZip) { $cliZip | ForEach-Object { Write-Host "   $($_.FullName)" } }
    else         { Write-Host "   (未生成)" }
    Write-Host ""
    Write-Host "📦 GUI NSIS 安装包（推荐）:"
    $guiExe = Get-ChildItem "$DistDir\gui-nsis" -Filter "*.exe" -ErrorAction SilentlyContinue
    if ($guiExe) { $guiExe | ForEach-Object { Write-Host "   $($_.FullName)" } }
    else         { Write-Host "   (未生成)" }
    Write-Host ""
    Success "全部完成！"
}

# ========== 主流程 ==========
Write-Host ""
Write-Host "  Mocika Shield — Windows 发布脚本（PowerShell）"
Write-Host "  版本: $Version  架构: $Arch"
Write-Host ""

Push-Location $Root
try {
    Check-Env
    Build-Stub
    Prepare-Dirs
    Build-Cli
    Build-Gui
    Package-Cli
    Collect-Gui
    Generate-Checksums
    Show-Results
} finally {
    Pop-Location
}
