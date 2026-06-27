# VoiceTray

Windows 10/11 amd64 system tray press-to-talk voice input app written in Rust.

The default hotkey is Right Ctrl because Windows cannot reliably capture Fn keys; Fn support is only attempted as an advanced scan-code option and automatically falls back to a detectable hotkey when unavailable.

## Build

```powershell
.\scripts\build.ps1 build
.\scripts\build.ps1 run
```

Target:

```powershell
rustup target add x86_64-pc-windows-msvc
cargo build --release --target x86_64-pc-windows-msvc
```

## Configuration

Configuration is stored in `%APPDATA%\VoiceTray\config.toml`.

API keys are encrypted with Windows DPAPI before being written to the local configuration file.

Logs are written under `%LOCALAPPDATA%\VoiceTray\logs`.

## Notes

Global low-level keyboard hooks can be flagged by security software. Production distribution should use code signing and clear documentation.
