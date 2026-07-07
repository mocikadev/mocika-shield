@echo off
setlocal enabledelayedexpansion

set "SCRIPT_DIR=%~dp0"
cd /d "%SCRIPT_DIR%"

echo ========================================
echo 构建 Mocika Shield Native 库 (Rust)
echo ========================================

where cargo-ndk >nul 2>&1
if %ERRORLEVEL% neq 0 (
    echo 错误: 未找到 cargo-ndk
    echo 请安装: cargo install cargo-ndk
    exit /b 1
)

if not defined ANDROID_NDK_ROOT if not defined NDK_HOME (
    echo 错误: 未设置 ANDROID_NDK_ROOT 或 NDK_HOME
    exit /b 1
)

set "OUTPUT_DIR=../../../build/jniLibs"
if not exist "%OUTPUT_DIR%" mkdir "%OUTPUT_DIR%"

echo 构建目标: arm64-v8a, armeabi-v7a, x86, x86_64
echo.

cargo ndk --platform 21 --target aarch64-linux-android --target armv7-linux-androideabi --target i686-linux-android --target x86_64-linux-android -o "%OUTPUT_DIR%" build --release
if %ERRORLEVEL% neq 0 (
    echo 错误: cargo ndk 构建失败
    exit /b 1
)

echo.
echo ========================================
echo ✓ 构建完成
echo ========================================
echo 产物位置:
for %%A in (arm64-v8a armeabi-v7a x86 x86_64) do (
    if exist "%OUTPUT_DIR%\%%A\libmocikashield.so" (
        echo   ✓ %%A\libmocikashield.so
    ) else (
        echo   ⚠ %%A\libmocikashield.so 未找到
    )
)

exit /b 0