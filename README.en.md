# Mocika Shield — Open-source Android APK Hardening Tool

[简体中文](README.md) | English

[![Latest Release](https://img.shields.io/github/v/release/mocikadev/mocika-shield?style=flat-square&label=release&color=6366f1)](https://github.com/mocikadev/mocika-shield/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/mocikadev/mocika-shield/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/mocikadev/mocika-shield/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-green?style=flat-square)](LICENSE)

Mocika Shield is an open-source, offline Android APK hardening tool. It compresses and encrypts DEX files, then decrypts and loads them through a protected runtime stub. The goal is to raise the cost of static analysis, unauthorized repackaging, and runtime debugging—not to claim that an Android application can be made impossible to reverse engineer.

It provides a cross-platform desktop GUI for Windows, macOS, and Linux, plus a Rust CLI for source builds and automation.

> Use Mocika Shield only with applications you own or are authorized to protect. Do not use it to bypass third-party protections or platform security controls.

## Highlights

- **DEX encryption:** Zstd compression and ChaCha20-Poly1305 authenticated encryption, with keys derived through HKDF-SHA256
- **Certificate-bound anti-tampering:** the original signing certificate participates in key derivation; unauthorized re-signing prevents payload decryption
- **Runtime anti-debugging:** native Rust checks for ptrace attachment, Frida mappings, and Frida GLib thread names
- **Low-profile payload storage:** encrypted data is appended beyond the declared DEX `file_size`, with no visible `assets/app.bin`
- **APK signing and certificate management:** import or create signing certificates directly in the desktop application
- **Built-in ZIP alignment:** protected and signed outputs are aligned for 4 KB and 16 KB Android page-size requirements
- **Fully offline processing:** APKs, keystores, certificates, and passwords are never uploaded
- **Four Android ABIs:** arm64-v8a, armeabi-v7a, x86, and x86_64
- **Bilingual interface:** Simplified Chinese and English

## Download

Download the latest desktop package from [GitHub Releases](https://github.com/mocikadev/mocika-shield/releases/latest).

| Platform | Package |
|---|---|
| Windows | `MocikaShield_x.y.z_windows_x64_setup.exe` |
| macOS | `MocikaShield_x.y.z_macos_universal.dmg` |
| Linux | `MocikaShield_x.y.z_linux_amd64.AppImage` or `.deb` |

Java 17 or later is required for APK protection, signing, and certificate alias detection. A complete JDK is recommended.

The installers are not currently commercially code-signed or notarized. On macOS, if the system reports that the developer cannot be verified, remove the quarantine attribute after moving the application to `/Applications`:

```bash
xattr -rd com.apple.quarantine /Applications/MocikaShield.app
```

## Quick Start

1. Download and install the desktop application for your platform.
2. Open **Certificates** and import the certificate used to sign the original APK, or create a new PKCS12 keystore.
3. Optionally set the certificate as the default for automatic signing.
4. Open **Protect**, select an already signed APK, and start protection.
5. The output is created next to the original APK as `{name}_protected.apk` or `{name}_protected_signed.apk`.

The certificate must match the original application. Protected DEX data is bound to that certificate, so signing the output with a different certificate will prevent the application from starting.

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

At runtime, the stub performs anti-debugging checks, extracts the encrypted payload, derives the decryption key from the embedded material and installed certificate, decrypts the DEX files in native Rust code, injects them into the class loader, and starts the original `Application`.

## Security Scope

Mocika Shield is a hardening layer, not an absolute security boundary. It can increase the effort required for static decompilation, repackaging, tampering, and common debugging workflows. A determined attacker controlling the device may still analyze runtime behavior or modify the execution environment.

For defense in depth, keep sensitive secrets and authorization decisions on a trusted server, minimize client-side trust, and combine hardening with application-specific integrity checks.

## Current Limitations

- APK is currently the supported input format; production AAB protection is not yet available
- Input APKs must already be signed
- An APK that has already been protected cannot be protected again
- The GUI currently processes one APK at a time and has no batch queue
- Windows, macOS, and Linux packages are not commercially code-signed or notarized
- Compatibility may vary for frameworks that rely on runtime DEX scanning; please report reproducible cases

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

[MIT](LICENSE)
