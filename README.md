# JustSay

JustSay is a Windows 10/11 system tray press-to-talk voice input app written in Rust.

Hold a global hotkey to record. Release the hotkey to transcribe speech, optionally refine the transcript with an LLM, and paste the final text into the currently focused input field.

[中文说明](README.zh.md)

## Project Status

JustSay is maintained when time allows. Issues and pull requests are welcome, but response times may vary.

## Screenshots

![JustSay usage overlay](screenshot/用法.png)

![JustSay settings window](screenshot/设置.png)

## Features

- System tray app with no normal taskbar window.
- Press-to-talk global hotkeys with presets and custom recording: Right Ctrl, CapsLock, Right Alt, Ctrl+Space, F13, Pause, and modifier combinations.
- Fn is not used as the default hotkey because Windows usually cannot reliably capture Fn keys.
- Microphone capture via `cpal`, converted to 16 kHz mono WAV for STT backends.
- Default recognition language: Simplified Chinese (`zh-CN`).
- Language choices: English, Simplified Chinese, Traditional Chinese, Japanese, Korean.
- STT compatibility modes:
  - OpenAI-style `/audio/transcriptions`
  - Aliyun Qwen-ASR `/chat/completions`
- Optional Smart Understanding. When enabled, JustSay uses a chat model to decide whether the utterance is dictation, repair, rewriting, writing, note-taking, search, URL opening, app launch, or a JustSay internal action.
- Optional LLM refinement and second-pass repair. Confidence scores are used internally for repair decisions and logs, not shown in the overlay.
- Non-activating always-on-top overlay with RMS-driven waveform and optional before/after debug panels.
- Clipboard-preserving text injection using Win32 Clipboard API and `SendInput`.
- Best-effort IME/layout handling before paste.
- Local config under AppData, API keys encrypted with Windows DPAPI.
- Logs under LocalAppData and an `Open Logs` tray menu item.
- Optional start-at-login through the current-user Registry Run key.

## Configuration

Open the tray menu and choose `Settings`.

For OpenAI-compatible STT:

```text
STT Mode: OpenAI /audio/transcriptions
STT Base URL: https://api.openai.com/v1
STT Model: whisper-1
```

For Aliyun Qwen-ASR:

```text
STT Mode: Aliyun Qwen-ASR /chat/completions
STT Base URL: https://dashscope.aliyuncs.com/compatible-mode/v1
STT Model: qwen3-asr-flash
```

LLM refinement uses an OpenAI-compatible Chat Completions endpoint. Use a chat model, not an ASR model. The refiner asks the model to return JSON with corrected text, a confidence score, and a short reason. If the first score is below 85, JustSay runs a second pass that re-evaluates likely speech recognition errors from the original STT text and first-pass result.

```text
LLM Base URL: https://dashscope.aliyuncs.com/compatible-mode/v1
LLM Model: a chat model available in your account
```

Smart Understanding is disabled by default. It reuses the LLM Chat Completions settings to understand natural speech before choosing a text or action path. JustSay is still an input-first assistant, not a general agent: it can dictate, repair recognition errors, rewrite recent context, compose short drafts, turn speech into notes, open searches or URLs, and launch apps through Start Menu shortcut resolution. It does not run commands, modify files, control other apps, make professional decisions, or perform complex automation. If understanding fails or is low-confidence for an action, JustSay falls back to the normal refine-and-paste flow.

Configuration is stored in:

```text
%APPDATA%\JustSay\config.toml
```

API keys are encrypted with Windows DPAPI before being written to the local config file.

## Requirements

- Windows 10 or Windows 11.
- Rust stable toolchain.
- Windows MSVC Rust target: `x86_64-pc-windows-msvc`.
- An OpenAI-compatible STT API key, or an Aliyun DashScope-compatible API key for Qwen-ASR.
- Optional: an OpenAI-compatible chat model API key for LLM refinement and Smart Understanding.

## Logs

Logs are written to:

```text
%LOCALAPPDATA%\JustSay\logs
```

Use the tray menu item `Open Logs` to open the latest log file.

When LLM refinement or Smart Understanding are enabled, logs can include dictated text, LLM before/after text, internal scores, understanding JSON, and action results for debugging. The overlay shows only user-readable text or action results. Do not share logs publicly if they contain private dictated content.

## Build

Install the Windows MSVC Rust target:

```powershell
rustup target add x86_64-pc-windows-msvc
```

Run from source:

```powershell
.\scripts\build.ps1 run
```

Build with the helper script:

```powershell
.\scripts\build.ps1 build
```

Or build directly with Cargo:

```powershell
cargo build --release --target x86_64-pc-windows-msvc
```

The executable is written to:

```text
target\x86_64-pc-windows-msvc\release\justsay.exe
```

## CI

This project uses GitHub Actions only. The CI workflow runs formatting checks, `cargo check`, tests, and a release build on pull requests and pushes to `main` or `master`.

## Release Workflow

Release builds are handled by GitHub Actions and are tag-only. Update [CHANGELOG.md](CHANGELOG.md), commit the change, then push a version tag to build and publish `justsay.exe` directly as a GitHub Release asset:

```powershell
git tag v0.1.0
git push origin v0.1.0
```

The workflow builds on `windows-latest`, compresses the executable with UPX, and uploads the `.exe` itself without wrapping it in a zip.

## License

JustSay is released under the [MIT License](LICENSE).

## Security Notes

JustSay uses a global low-level keyboard hook, clipboard access, and simulated paste. These are expected for this kind of app, but security software may flag unsigned builds. Production distribution should use code signing and clear documentation.

JustSay does not require administrator privileges.
