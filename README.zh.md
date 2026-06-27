# JustSay

JustSay 是一个使用 Rust 编写的 Windows 10/11 系统托盘语音输入工具。

按住全局热键开始录音，松开热键后进行语音识别，可选调用 LLM 对识别文本做整理，然后把最终文本粘贴到当前聚焦的输入框里。

[English README](README.md)

## 项目状态

JustSay 有空才维护。欢迎提交 issue 和 pull request，但响应时间不保证。

## 截图

![JustSay 使用效果](screenshot/用法.png)

![JustSay 设置窗口](screenshot/设置.png)

## 功能

- 系统托盘常驻运行，不显示普通任务栏窗口。
- Press-to-Talk 全局热键支持预设和自定义录入：Right Ctrl、CapsLock、Right Alt、Ctrl+Space、F13、Pause 以及修饰键组合。
- 默认不使用 Fn 键，因为 Windows 通常无法稳定捕获 Fn 键。
- 使用 `cpal` 采集麦克风音频，并转换为 16 kHz mono WAV 供 STT 后端使用。
- 默认识别语言为简体中文 `zh-CN`。
- 支持语言：英语、简体中文、繁體中文、日本語、한국어。
- STT 兼容模式：
  - OpenAI 风格 `/audio/transcriptions`
  - 阿里云 Qwen-ASR `/chat/completions`
- 可选 Smart Understanding 智能理解。开启后，JustSay 会先判断这句话是在直接输入、修复识别错误、改写、写作生成、记录备忘、搜索、打开网址、打开软件，还是打开 JustSay 内部页面。
- 可选 LLM refinement 和二次修复。评分只用于内部判断是否二次修复和日志排查，不显示在悬浮窗里。
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

LLM refinement 使用 OpenAI 兼容的 Chat Completions 接口。这里要填聊天模型，不要填 ASR 语音识别模型。Refiner 会要求模型返回包含纠错文本、置信评分和简短原因的 JSON；如果第一遍评分低于 85，JustSay 会结合 STT 原文和第一遍结果再做一次二阶段纠错。

```text
LLM Base URL: https://dashscope.aliyuncs.com/compatible-mode/v1
LLM Model: 你账号可用的聊天模型
```

Smart Understanding 默认关闭。它复用 LLM Chat Completions 配置，在粘贴或执行动作前先理解自然口语。JustSay 仍然是输入优先的助手，不是通用 Agent：它可以直接输入、修复识别错误、结合短期上下文改写上一条内容、为写作请求生成成稿、把口语整理成备忘记录、打开搜索或网址，也可以通过开始菜单快捷方式解析来启动应用。它不会执行命令、修改文件、控制其他应用、代做专业决策或执行复杂自动化。如果理解失败，或者动作置信度不足，JustSay 会回退到原来的优化并粘贴流程。

配置文件位置：

```text
%APPDATA%\JustSay\config.toml
```

API Key 写入本地配置前会使用 Windows DPAPI 加密。

## 环境要求

- Windows 10 或 Windows 11。
- Rust stable toolchain。
- Windows MSVC Rust target：`x86_64-pc-windows-msvc`。
- OpenAI 兼容 STT API Key，或阿里云 DashScope 兼容的 Qwen-ASR API Key。
- 可选：OpenAI 兼容聊天模型 API Key，用于 LLM refinement 和 Smart Understanding。

## 日志

日志目录：

```text
%LOCALAPPDATA%\JustSay\logs
```

可以通过托盘菜单的 `Open Logs` 打开最新日志。

启用 LLM refinement 或 Smart Understanding 后，日志可能包含口述文本、LLM 优化前后文本、内部评分、理解 JSON 和动作结果，方便调试效果。悬浮窗只显示用户可读的文本结果或动作结果，不显示评分。如果日志里包含私人输入内容，不要公开分享。

## 构建

安装 Windows MSVC Rust target：

```powershell
rustup target add x86_64-pc-windows-msvc
```

从源码运行：

```powershell
.\scripts\build.ps1 run
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
target\x86_64-pc-windows-msvc\release\justsay.exe
```

## CI

本项目只使用 GitHub Actions。CI workflow 会在 pull request 以及推送到 `main` 或 `master` 时运行格式检查、`cargo check`、测试和 release 构建。

## GitHub Release

Release 构建由 GitHub Actions 处理，并且只会在 push tag 时触发。发布前先更新 [CHANGELOG.md](CHANGELOG.md)，提交后再推送版本 tag，随后会自动构建并发布 `justsay.exe`：

```powershell
git tag v0.1.0
git push origin v0.1.0
```

Workflow 使用 `windows-latest` 构建，用 UPX 压缩 exe，并直接把 `.exe` 本体作为 Release asset 上传，不打 zip 包。

## 许可证

JustSay 使用 [MIT License](LICENSE) 发布。

## 安全说明

JustSay 会使用全局低级键盘 hook、剪贴板访问和模拟粘贴。这些能力是语音输入工具的预期行为，但未签名构建可能被安全软件提示。正式分发建议进行代码签名，并清楚说明这些行为。

JustSay 不需要管理员权限。
