# Mocika Shield — Open-source Android APK Hardening Tool

[简体中文](README.md) | English

[![Latest Release](https://img.shields.io/github/v/release/mocikadev/mocika-shield?style=flat-square&label=release&color=6366f1)](https://github.com/mocikadev/mocika-shield/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/mocikadev/mocika-shield/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/mocikadev/mocika-shield/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-green?style=flat-square)](#license)

Mocika Shield is an open-source, offline Android APK hardening tool. It compresses and encrypts DEX files, then decrypts and loads them through a protected runtime stub. The goal is to raise the cost of static analysis, unauthorized repackaging, and runtime debugging—not to claim that an Android application can be made impossible to reverse engineer.

It provides a cross-platform desktop GUI for Windows, macOS, and Linux, plus a Rust CLI for source builds and automation.

> Use Mocika Shield only with applications you own or are authorized to protect. Do not use it to bypass third-party protections or platform security controls.

## Highlights

- **APK hardening:** encrypts application DEX files and binds protected output to the original signing certificate
- **Runtime protection:** includes baseline anti-debugging and environment checks to raise the cost of common dynamic analysis
- **Android compatibility:** standard mode supports Android 5.0 and later, with a separately verified Android 4.4 industrial mode
- **Integrated signing workflow:** the desktop GUI covers APK hardening, certificate management, automatic signing, and standalone signing
- **Installation compatibility:** performs 4 KB / 16 KB ZIP alignment and handles common native-library and framework compatibility cases
- **Cross-platform and multi-ABI:** desktop packages support Windows, macOS, and Linux; Android output supports four mainstream ABIs
- **Local offline processing:** APKs, certificates, keystores, and passwords remain on the local computer
- **Bilingual interface:** Simplified Chinese and English

For protocol details, runtime loading, security boundaries, and compatibility internals, see the [technical internals](docs/design/internals.md) and [documentation index](docs/README.md).

## Download

Download the latest desktop package from [GitHub Releases](https://github.com/mocikadev/mocika-shield/releases/latest).

| Platform | Package |
|---|---|
| Windows | `MocikaShield_x.y.z_windows_x64_setup.exe` |
| macOS | `MocikaShield_x.y.z_macos_universal.dmg` |
| Linux | `MocikaShield_x.y.z_linux_amd64.AppImage` or `.deb` |

A full JDK 8 or later with `java` and `keytool` is required for APK protection, signing, and certificate alias detection. The runtime workflow does not require `javac`.

The installers are not currently commercially code-signed or notarized. On macOS, if the system reports that the developer cannot be verified, remove the quarantine attribute after moving the application to `/Applications`:

```bash
xattr -rd com.apple.quarantine /Applications/MocikaShield.app
```

## Quick Start

1. Download and install the desktop application for your platform.
2. Open **Certificates** and import the certificate used to sign the original APK, or create a new PKCS12 keystore.
3. Optionally set the certificate as the default for automatic signing.
4. Open **Protect**, select an already signed APK, and choose the target Android compatibility mode.
5. Keep the default Android 5.0+ mode unless the target fleet includes Android 4.4 industrial devices.
6. The output is created next to the original APK as `{name}_protected.apk` or `{name}_protected_signed.apk`.

The certificate must match the original application. Protected DEX data is bound to that certificate, so signing the output with a different certificate will prevent the application from starting.

### Android Compatibility

| Mode | Target | Status | Constraints |
|---|---|---|---|
| Android 5.0 and later (default) | API 21+ | Standard | Four ABIs; relevant regression coverage includes Android 5.0, 6.0, 9, 15, and 16 |
| Android 4.4 industrial compatibility | API 19+ | Verified with limits | The input APK must contain no native libraries, or only `armeabi-v7a` native libraries; verified on an Android 4.4.2 `armeabi-v7a`/NEON industrial board |

Compatibility mode does not lower the application's own `minSdkVersion`. See the [usage guide](docs/usage.md) and [Android 4.4 compatibility design](docs/design/android-4.4-compatibility.md) for details.

## How It Works

```text
Original signed APK
        ↓
Unpack resources without decompiling smali
        ↓
Replace Application with the runtime stub
        ↓
Read the original signing certificate fingerprint
        ↓
Compress and encrypt DEX files into a DEXB v5 payload
        ↓
Inject stub DEX and native libraries for four ABIs
        ↓
Rebuild and align the protected APK
        ↓
Sign with the original application's certificate
```

At runtime, the stub performs an environment check before reading the DEX cache. A cache hit proceeds directly to class-loader injection; a cache miss performs the same check again at the native decrypt boundary, extracts and decrypts the payload, writes the private DEX cache, injects the DEX files, and starts the original `Application`.

## Security Scope

Mocika Shield is a hardening layer, not an absolute security boundary. It can increase the effort required for static decompilation, repackaging, tampering, and common debugging workflows. A determined attacker controlling the device may still analyze runtime behavior or modify the execution environment.

For defense in depth, keep sensitive secrets and authorization decisions on a trusted server, minimize client-side trust, and combine hardening with application-specific integrity checks.

## Current Limitations

- APK is currently the supported input format; production AAB protection is not yet available
- Standard mode targets Android 5.0 (API 21) and later. Android 4.4 (API 19–20) is supported through the industrial compatibility mode; physical-device coverage currently focuses on `armeabi-v7a`/NEON hardware
- Input APKs must already be signed
- An APK that has already been protected cannot be protected again
- The GUI currently processes one APK at a time and has no batch queue
- Windows, macOS, and Linux packages are not commercially code-signed or notarized
- Frameworks with custom runtime DEX scanning may still require compatibility handling; ARouter 1.5.1 runtime scanning is supported, and other reproducible cases should be reported

## Build from Source

### Requirements

- Rust 1.70 or later
- Node.js 22 or later
- Java 17 or later
- Android SDK and NDK `29.0.14206865`
- `cargo-ndk` and Tauri CLI

The Android runtime stub must be built first because both the CLI and GUI depend on its generated `resources.zip`.

```bash
# Build the Android runtime stub first
make build-stub

# Build the CLI
make build-cli

# Build the Tauri desktop application
make build-gui

# Build everything in the required order
make build-all
```

See the [build guide](docs/ops/build.md) and [environment requirements](docs/ops/environment.md) for details.

## CLI

GitHub Releases provide desktop GUI packages for end users. The CLI remains available for source builds, local maintenance, and automation.

```bash
make build-stub
make build-cli

./target/release/shield protect \
  --input input.apk \
  --output protected.apk
```

The protected output must be signed with the same certificate as the original application before installation.

## Project Structure

```text
crates/shield-core       Shared Rust core for protection, signing, alignment, and environment detection
apps/shield-cli          Rust command-line entry point
apps/shield-gui          Tauri v2 + React desktop GUI
shield-stub              Android Java and Rust runtime loader
tools/stats-worker       Anonymous aggregate statistics service
scripts                  Build and release scripts
```

## Privacy

All APK and certificate operations run locally. The desktop application enables anonymous aggregate usage statistics by default to help understand launches and successful or failed operations. Users can disable telemetry in Settings. Telemetry does not include APK contents, package names, paths, certificates, passwords, or keystores.

See the [telemetry documentation](docs/ops/telemetry.md) for the exact data scope.

## Feedback and Security

- For bugs and compatibility problems, read the [support guide](docs/process/support.md) and open an [issue](https://github.com/mocikadev/mocika-shield/issues).
- For feature requests, use the [feature request form](https://github.com/mocikadev/mocika-shield/issues/new?template=feature_request.yml).
- For security-sensitive reports, follow [SECURITY.md](SECURITY.md) and do not publish APKs, keystores, passwords, or directly exploitable details in a public issue.

Diagnostic information can be copied from the application's **About** page. Review it before sharing and remove anything you do not want to disclose.

## License

This project is dual-licensed under **MIT OR Apache-2.0**. You may choose either license.

- [MIT License](LICENSE-MIT)
- [Apache License 2.0](LICENSE-APACHE)
