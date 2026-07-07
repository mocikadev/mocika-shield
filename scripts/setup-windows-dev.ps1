# Mocika Shield - Windows 开发环境一键配置脚本（Scoop）
#
# 用法：
#   .\scripts\setup-windows-dev.ps1
#
# 前置要求：
#   已安装 Scoop（https://scoop.sh）
#   以普通用户身份运行（不需要管理员）
#
# 脚本完成后仍需手动处理：
#   1. 安装 Visual Studio Build Tools 2022（含 C++ 桌面开发组件）
#      https://visualstudio.microsoft.com/visual-cpp-build-tools/
#   2. 重新打开终端，让 PATH 生效

param(
    [string]$NdkVersion = "29.0.14206865"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Info($msg)    { Write-Host "==> $msg" -ForegroundColor Blue }
function Success($msg) { Write-Host "  ✓ $msg" -ForegroundColor Green }
function Warn($msg)    { Write-Host "  ⚠ $msg" -ForegroundColor Yellow }
function Err($msg)     { Write-Host "  ✗ $msg" -ForegroundColor Red; exit 1 }
function Step($n, $msg){ Write-Host "`n[$n] $msg" -ForegroundColor Cyan }

function Ensure-ScoopPkg($pkg, $bucket = "") {
    $installed = scoop list 2>$null | Select-String "^$pkg\s"
    if ($installed) {
        Success "$pkg 已安装，跳过"
    } else {
        if ($bucket) {
            Info "安装 $pkg（来自 $bucket bucket）..."
            scoop install "$bucket/$pkg"
        } else {
            Info "安装 $pkg..."
            scoop install $pkg
        }
        Success "$pkg 安装完成"
    }
}

function Ensure-ScoopBucket($name, $url = "") {
    $exists = scoop bucket list 2>$null | Select-String "^$name\s"
    if ($exists) {
        Success "bucket '$name' 已添加，跳过"
    } else {
        if ($url) {
            scoop bucket add $name $url
        } else {
            scoop bucket add $name
        }
        Success "bucket '$name' 已添加"
    }
}

function Ensure-CargoPkg($pkg) {
    $cmd = $pkg -replace '-', ''
    $found = Get-Command $pkg -ErrorAction SilentlyContinue
    if (-not $found) {
        $found = Get-Command "cargo-$pkg" -ErrorAction SilentlyContinue
    }
    if ($found) {
        Success "$pkg 已安装，跳过"
    } else {
        Info "安装 $pkg（cargo install，需要网络，请稍候）..."
        cargo install $pkg
        Success "$pkg 安装完成"
    }
}

function Ensure-RustupTarget($target) {
    $installed = (rustup target list --installed 2>$null) -join "`n"
    if ($installed -match "(?m)^$([regex]::Escape($target))$") {
        Success "target '$target' 已安装，跳过"
    } else {
        Info "添加 Rust target: $target..."
        rustup target add $target
        Success "target '$target' 添加完成"
    }
}

# ============================================================
Write-Host ""
Write-Host "  ================================================" -ForegroundColor Green
Write-Host "  Mocika Shield - Windows 开发环境配置" -ForegroundColor Green
Write-Host "  ================================================" -ForegroundColor Green
Write-Host ""

# ============================================================
Step 1 "检查 Scoop"
if (-not (Get-Command scoop -ErrorAction SilentlyContinue)) {
    Err "未找到 Scoop。请先安装：`n  Set-ExecutionPolicy RemoteSigned -Scope CurrentUser`n  irm get.scoop.sh | iex"
}
Success "Scoop 已安装：$(scoop --version 2>$null | Select-Object -First 1)"

# ============================================================
Step 2 "添加 Scoop bucket"
Ensure-ScoopBucket "java"
Ensure-ScoopBucket "extras"

# ============================================================
Step 3 "安装基础工具（Scoop）"
Ensure-ScoopPkg "git"
Ensure-ScoopPkg "make"
Ensure-ScoopPkg "7zip"
Ensure-ScoopPkg "nodejs-lts"

# ============================================================
Step 4 "安装 Rust 工具链（Scoop）"
Ensure-ScoopPkg "rustup"

# 刷新 PATH，让 rustup/cargo 生效
$env:PATH = [System.Environment]::GetEnvironmentVariable("PATH", "User") + ";" +
            [System.Environment]::GetEnvironmentVariable("PATH", "Machine")

if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
    Err "rustup 安装后仍未找到，请关闭终端重新打开后再运行此脚本"
}

Info "设置默认 toolchain 为 stable-msvc..."
rustup default stable-msvc
Success "Rust toolchain: $(rustc --version 2>$null)"

# ============================================================
Step 5 "添加 Rust target"
Ensure-RustupTarget "x86_64-pc-windows-msvc"
Ensure-RustupTarget "aarch64-linux-android"
Ensure-RustupTarget "armv7-linux-androideabi"
Ensure-RustupTarget "i686-linux-android"
Ensure-RustupTarget "x86_64-linux-android"

# ============================================================
Step 6 "安装 Rust cargo 工具"
Ensure-CargoPkg "cargo-ndk"
Ensure-CargoPkg "tauri-cli"

# ============================================================
Step 7 "安装 Java（Temurin JDK 17）"
Ensure-ScoopPkg "temurin17-jdk" "java"

$env:PATH = [System.Environment]::GetEnvironmentVariable("PATH", "User") + ";" +
            [System.Environment]::GetEnvironmentVariable("PATH", "Machine")
if (Get-Command java -ErrorAction SilentlyContinue) {
    Success "Java: $(java -version 2>&1 | Select-Object -First 1)"
}

# ============================================================
Step 8 "安装 Android 命令行工具（android-clt）"
Ensure-ScoopPkg "android-clt"

$env:PATH = [System.Environment]::GetEnvironmentVariable("PATH", "User") + ";" +
            [System.Environment]::GetEnvironmentVariable("PATH", "Machine")

# 检测 ANDROID_HOME
$androidHome = $env:ANDROID_HOME
if (-not $androidHome) { $androidHome = $env:ANDROID_SDK_ROOT }
if (-not $androidHome) {
    # Scoop 安装的 android-clt 默认路径
    $candidate = Join-Path $env:USERPROFILE "scoop\apps\android-clt\current"
    if (Test-Path $candidate) { $androidHome = $candidate }
}
if (-not $androidHome -or -not (Test-Path $androidHome)) {
    Warn "无法自动检测 ANDROID_HOME，请手动设置："
    Warn '  [System.Environment]::SetEnvironmentVariable("ANDROID_HOME", "<路径>", "User")'
} else {
    Success "Android SDK: $androidHome"

    # 设置环境变量（用户级，永久生效）
    [System.Environment]::SetEnvironmentVariable("ANDROID_HOME", $androidHome, "User")
    [System.Environment]::SetEnvironmentVariable("ANDROID_SDK_ROOT", $androidHome, "User")
    $env:ANDROID_HOME = $androidHome
    $env:ANDROID_SDK_ROOT = $androidHome

    # ============================================================
    Step 9 "安装 Android NDK $NdkVersion（通过 sdkmanager）"

    $sdkmanager = Get-Command sdkmanager -ErrorAction SilentlyContinue
    if (-not $sdkmanager) {
        $sdkmanager = Get-Item "$androidHome\cmdline-tools\latest\bin\sdkmanager.bat" -ErrorAction SilentlyContinue
    }

    if ($sdkmanager) {
        $ndkPath = Join-Path $androidHome "ndk\$NdkVersion"
        if (Test-Path $ndkPath) {
            Success "Android NDK $NdkVersion 已安装：$ndkPath"
            [System.Environment]::SetEnvironmentVariable("ANDROID_NDK_ROOT", $ndkPath, "User")
            $env:ANDROID_NDK_ROOT = $ndkPath
        } else {
            Info "安装 Android NDK $NdkVersion（需要网络，文件较大，请稍候）..."
            # sdkmanager 需要接受许可证
            "y" | & $sdkmanager.Source "ndk;$NdkVersion"
            if (Test-Path $ndkPath) {
                Success "Android NDK $NdkVersion 安装完成：$ndkPath"
                [System.Environment]::SetEnvironmentVariable("ANDROID_NDK_ROOT", $ndkPath, "User")
                $env:ANDROID_NDK_ROOT = $ndkPath
            } else {
                Warn "NDK 安装后路径未找到，请手动确认：$ndkPath"
            }
        }
    } else {
        Warn "未找到 sdkmanager，请手动安装 NDK $NdkVersion"
        Warn "  sdkmanager `"ndk;$NdkVersion`""
    }
}

# ============================================================
Step 10 "安装 NSIS（GUI 打包需要）"
Ensure-ScoopPkg "nsis" "extras"

# ============================================================
Step 11 "检查 MSVC Build Tools"
$vsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$hasMsvc = $false
if (Test-Path $vsWhere) {
    $vsPath = & $vsWhere -latest -products * -requires Microsoft.VisualCpp.Tools.HostX64.TargetX64 -property installationPath 2>$null
    $hasMsvc = ($null -ne $vsPath -and $vsPath -ne "")
}
if (-not $hasMsvc) {
    $hasMsvc = $null -ne (Get-Command link.exe -ErrorAction SilentlyContinue)
}

if ($hasMsvc) {
    Success "MSVC Build Tools 已安装"
} else {
    Write-Host ""
    Write-Host "  ┌─────────────────────────────────────────────────────┐" -ForegroundColor Yellow
    Write-Host "  │  ⚠  需要手动安装 Visual Studio Build Tools 2022    │" -ForegroundColor Yellow
    Write-Host "  │                                                     │" -ForegroundColor Yellow
    Write-Host "  │  下载地址：                                         │" -ForegroundColor Yellow
    Write-Host "  │  https://visualstudio.microsoft.com/               │" -ForegroundColor Yellow
    Write-Host "  │           visual-cpp-build-tools/                  │" -ForegroundColor Yellow
    Write-Host "  │                                                     │" -ForegroundColor Yellow
    Write-Host "  │  安装时勾选：「使用 C++ 的桌面开发」               │" -ForegroundColor Yellow
    Write-Host "  └─────────────────────────────────────────────────────┘" -ForegroundColor Yellow
}

# ============================================================
Write-Host ""
Write-Host "  ================================================" -ForegroundColor Green
Write-Host "  环境配置完成！" -ForegroundColor Green
Write-Host "  ================================================" -ForegroundColor Green
Write-Host ""
Write-Host "  已安装/配置：" -ForegroundColor Blue
Write-Host "    ✓ Scoop 工具：git / make / 7zip / nodejs-lts / rustup / nsis"
Write-Host "    ✓ Node.js / npm：React GUI 前端构建"
Write-Host "    ✓ Java：Temurin JDK 17"
Write-Host "    ✓ Android SDK（android-clt）"
Write-Host "    ✓ Android NDK $NdkVersion"
Write-Host "    ✓ Rust toolchain（stable-msvc）"
Write-Host "    ✓ Rust targets：msvc / 4x android"
Write-Host "    ✓ cargo 工具：cargo-ndk / tauri-cli"
Write-Host "    ✓ NSIS"
Write-Host ""

if (-not $hasMsvc) {
    Write-Host "  ❌ 待手动安装：Visual Studio Build Tools 2022（含 C++ 组件）" -ForegroundColor Yellow
    Write-Host ""
}

Write-Host "  下一步：" -ForegroundColor Yellow
Write-Host "    1. 关闭并重新打开终端（刷新环境变量）"
if (-not $hasMsvc) {
    Write-Host "    2. 安装 Visual Studio Build Tools 2022"
    Write-Host "    3. 运行构建："
} else {
    Write-Host "    2. 运行构建："
}
Write-Host "         make build-stub"
Write-Host "         make build-cli"
Write-Host "         make build-gui"
Write-Host ""
