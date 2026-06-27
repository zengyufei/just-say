# VoiceTray

VoiceTray 是一个使用 Rust 编写的 Windows 10/11 系统托盘语音输入工具。

按住全局热键开始录音，松开热键后进行语音识别，可选调用 LLM 对识别文本做整理，然后把最终文本粘贴到当前聚焦的输入框里。

[English README](README.md)

## 功能

- 系统托盘常驻运行，不显示普通任务栏窗口。
- Press-to-Talk 全局热键：Right Ctrl、CapsLock、Right Alt、Ctrl+Space。
- 默认不使用 Fn 键，因为 Windows 通常无法稳定捕获 Fn 键。
- 使用 `cpal` 采集麦克风音频，并转换为 16 kHz mono WAV 供 STT 后端使用。
- 默认识别语言为简体中文 `zh-CN`。
- 支持语言：英语、简体中文、繁體中文、日本語、한국어。
- STT 兼容模式：
  - OpenAI 风格 `/audio/transcriptions`
  - 阿里云 Qwen-ASR `/chat/completions`
- 可选 LLM refinement，用于整理口语化语音识别文本。
- 不抢焦点的置顶悬浮窗，包含真实 RMS 驱动的波形和可选的优化前后调试面板。
- 使用 Win32 Clipboard API 和 `SendInput` 注入文本，并尽量恢复原剪贴板内容。
- 粘贴前对输入法/键盘布局做 best-effort 处理，减少中文输入法干扰。
- 配置保存在 AppData，API Key 使用 Windows DPAPI 加密。
- 日志保存在 LocalAppData，托盘菜单提供 `Open Logs`。
- 支持通过当前用户 Registry Run key 设置开机自启动。

## 配置

右键托盘图标，打开 `Settings`。

OpenAI 兼容 STT：

```text
STT Mode: OpenAI /audio/transcriptions
STT Base URL: https://api.openai.com/v1
STT Model: whisper-1
```

阿里云 Qwen-ASR：

```text
STT Mode: Aliyun Qwen-ASR /chat/completions
STT Base URL: https://dashscope.aliyuncs.com/compatible-mode/v1
STT Model: qwen3-asr-flash
```

LLM refinement 使用 OpenAI 兼容的 Chat Completions 接口。这里要填聊天模型，不要填 ASR 语音识别模型。

```text
LLM Base URL: https://dashscope.aliyuncs.com/compatible-mode/v1
LLM Model: 你账号可用的聊天模型
```

配置文件位置：

```text
%APPDATA%\VoiceTray\config.toml
```

API Key 写入本地配置前会使用 Windows DPAPI 加密。

## 日志

日志目录：

```text
%LOCALAPPDATA%\VoiceTray\logs
```

可以通过托盘菜单的 `Open Logs` 打开最新日志。

启用 LLM refinement 后，日志会包含 STT 原文和 LLM 优化前后的文本，方便调试效果。如果日志里包含私人输入内容，不要公开分享。

## 构建

安装 Windows MSVC Rust target：

```powershell
rustup target add x86_64-pc-windows-msvc
```

使用脚本构建：

```powershell
.\scripts\build.ps1 build
```

或直接使用 Cargo：

```powershell
cargo build --release --target x86_64-pc-windows-msvc
```

构建产物：

```text
target\x86_64-pc-windows-msvc\release\voicetray.exe
```

## GitHub Release

GitHub Actions 只会在 push tag 时触发。推送 tag 后会自动构建并发布 `voicetray.exe`：

```powershell
git tag v0.1.0
git push origin v0.1.0
```

Workflow 使用 `windows-latest` 构建，用 UPX 压缩 exe，并直接把 `.exe` 本体作为 Release asset 上传，不打 zip 包。

## 安全说明

VoiceTray 会使用全局低级键盘 hook、剪贴板访问和模拟粘贴。这些能力是语音输入工具的预期行为，但未签名构建可能被安全软件提示。正式分发建议进行代码签名，并清楚说明这些行为。

VoiceTray 不需要管理员权限。
