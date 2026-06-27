use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    EnUs,
    #[default]
    ZhCn,
    ZhTw,
    JaJp,
    KoKr,
}

impl Language {
    pub fn bcp47(&self) -> &'static str {
        match self {
            Self::EnUs => "en-US",
            Self::ZhCn => "zh-CN",
            Self::ZhTw => "zh-TW",
            Self::JaJp => "ja-JP",
            Self::KoKr => "ko-KR",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::EnUs => "English",
            Self::ZhCn => "简体中文",
            Self::ZhTw => "繁體中文",
            Self::JaJp => "日本語",
            Self::KoKr => "한국어",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Hotkey {
    #[default]
    RightCtrl,
    CapsLock,
    RightAlt,
    CtrlSpace,
    FnScanCode {
        scan_code: u32,
    },
    Custom {
        trigger_vk: u32,
        trigger_scan_code: u32,
        extended: bool,
        modifiers: HotkeyModifiers,
    },
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct HotkeyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

impl Hotkey {
    pub fn from_key_event(
        trigger_vk: u32,
        trigger_scan_code: u32,
        extended: bool,
        modifiers: HotkeyModifiers,
    ) -> Self {
        let hotkey = Self::Custom {
            trigger_vk,
            trigger_scan_code,
            extended,
            modifiers,
        };
        hotkey.normalized()
    }

    pub fn display_name(&self) -> String {
        match self {
            Self::RightCtrl => "Right Ctrl".to_string(),
            Self::CapsLock => "CapsLock".to_string(),
            Self::RightAlt => "Right Alt".to_string(),
            Self::CtrlSpace => "Ctrl+Space".to_string(),
            Self::FnScanCode { scan_code } => format!("Fn scan code 0x{scan_code:x}"),
            Self::Custom {
                trigger_vk,
                trigger_scan_code,
                extended,
                modifiers,
            } => custom_hotkey_name(*trigger_vk, *trigger_scan_code, *extended, *modifiers),
        }
    }

    pub fn from_combo_index(index: isize) -> Self {
        match index {
            1 => Self::CapsLock,
            2 => Self::RightAlt,
            3 => Self::CtrlSpace,
            4 => Self::Custom {
                trigger_vk: VK_F13,
                trigger_scan_code: 0,
                extended: false,
                modifiers: HotkeyModifiers::default(),
            },
            _ => Self::RightCtrl,
        }
    }

    pub fn combo_index(&self) -> isize {
        match self {
            Self::RightCtrl => 0,
            Self::CapsLock => 1,
            Self::RightAlt => 2,
            Self::CtrlSpace => 3,
            Self::FnScanCode { .. } | Self::Custom { .. } => 4,
        }
    }

    pub fn is_custom_like(&self) -> bool {
        matches!(self, Self::FnScanCode { .. } | Self::Custom { .. })
    }

    fn normalized(self) -> Self {
        match self {
            Self::Custom {
                trigger_vk,
                trigger_scan_code: _,
                extended: _,
                modifiers,
            } if modifiers.is_empty() && trigger_vk == VK_RCONTROL => Self::RightCtrl,
            Self::Custom {
                trigger_vk,
                trigger_scan_code: _,
                extended: _,
                modifiers,
            } if modifiers.is_empty() && trigger_vk == VK_CAPITAL => Self::CapsLock,
            Self::Custom {
                trigger_vk,
                trigger_scan_code: _,
                extended: _,
                modifiers,
            } if modifiers.is_empty() && trigger_vk == VK_RMENU => Self::RightAlt,
            Self::Custom {
                trigger_vk,
                trigger_scan_code: _,
                extended: _,
                modifiers,
            } if modifiers == HotkeyModifiers::ctrl_only() && trigger_vk == VK_SPACE => {
                Self::CtrlSpace
            }
            Self::Custom {
                trigger_vk,
                trigger_scan_code,
                extended,
                modifiers,
            } => Self::Custom {
                trigger_vk,
                trigger_scan_code,
                extended,
                modifiers,
            },
            other => other,
        }
    }
}

impl HotkeyModifiers {
    pub fn ctrl_only() -> Self {
        Self {
            ctrl: true,
            alt: false,
            shift: false,
            win: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.ctrl && !self.alt && !self.shift && !self.win
    }
}

const VK_CAPITAL: u32 = 0x14;
const VK_RCONTROL: u32 = 0xa3;
const VK_RMENU: u32 = 0xa5;
const VK_SPACE: u32 = 0x20;
const VK_F13: u32 = 0x7c;

fn custom_hotkey_name(
    trigger_vk: u32,
    trigger_scan_code: u32,
    extended: bool,
    modifiers: HotkeyModifiers,
) -> String {
    let trigger_name = vk_name(trigger_vk)
        .map(str::to_string)
        .unwrap_or_else(|| format_unknown_key(trigger_vk, trigger_scan_code, extended));
    let mut parts = Vec::new();
    if modifiers.ctrl {
        parts.push("Ctrl".to_string());
    }
    if modifiers.alt {
        parts.push("Alt".to_string());
    }
    if modifiers.shift {
        parts.push("Shift".to_string());
    }
    if modifiers.win {
        parts.push("Win".to_string());
    }
    parts.push(trigger_name);
    parts.join("+")
}

fn format_unknown_key(trigger_vk: u32, trigger_scan_code: u32, extended: bool) -> String {
    if trigger_scan_code == 0 {
        format!("VK 0x{trigger_vk:02x}")
    } else if extended {
        format!("VK 0x{trigger_vk:02x} / Scan 0x{trigger_scan_code:02x} ext")
    } else {
        format!("VK 0x{trigger_vk:02x} / Scan 0x{trigger_scan_code:02x}")
    }
}

fn vk_name(vk: u32) -> Option<&'static str> {
    match vk {
        0x08 => Some("Backspace"),
        0x09 => Some("Tab"),
        0x0d => Some("Enter"),
        0x10 => Some("Shift"),
        0x11 => Some("Ctrl"),
        0x12 => Some("Alt"),
        0x13 => Some("Pause"),
        0x14 => Some("CapsLock"),
        0x1b => Some("Esc"),
        0x20 => Some("Space"),
        0x21 => Some("PageUp"),
        0x22 => Some("PageDown"),
        0x23 => Some("End"),
        0x24 => Some("Home"),
        0x25 => Some("Left"),
        0x26 => Some("Up"),
        0x27 => Some("Right"),
        0x28 => Some("Down"),
        0x2d => Some("Insert"),
        0x2e => Some("Delete"),
        0x5b => Some("Left Win"),
        0x5c => Some("Right Win"),
        0x60 => Some("Num0"),
        0x61 => Some("Num1"),
        0x62 => Some("Num2"),
        0x63 => Some("Num3"),
        0x64 => Some("Num4"),
        0x65 => Some("Num5"),
        0x66 => Some("Num6"),
        0x67 => Some("Num7"),
        0x68 => Some("Num8"),
        0x69 => Some("Num9"),
        0x6a => Some("Num*"),
        0x6b => Some("Num+"),
        0x6d => Some("Num-"),
        0x6e => Some("Num."),
        0x6f => Some("Num/"),
        0x70 => Some("F1"),
        0x71 => Some("F2"),
        0x72 => Some("F3"),
        0x73 => Some("F4"),
        0x74 => Some("F5"),
        0x75 => Some("F6"),
        0x76 => Some("F7"),
        0x77 => Some("F8"),
        0x78 => Some("F9"),
        0x79 => Some("F10"),
        0x7a => Some("F11"),
        0x7b => Some("F12"),
        0x7c => Some("F13"),
        0x7d => Some("F14"),
        0x7e => Some("F15"),
        0x7f => Some("F16"),
        0x80 => Some("F17"),
        0x81 => Some("F18"),
        0x82 => Some("F19"),
        0x83 => Some("F20"),
        0x84 => Some("F21"),
        0x85 => Some("F22"),
        0x86 => Some("F23"),
        0x87 => Some("F24"),
        0xa0 => Some("Left Shift"),
        0xa1 => Some("Right Shift"),
        0xa2 => Some("Left Ctrl"),
        0xa3 => Some("Right Ctrl"),
        0xa4 => Some("Left Alt"),
        0xa5 => Some("Right Alt"),
        0x30 => Some("0"),
        0x31 => Some("1"),
        0x32 => Some("2"),
        0x33 => Some("3"),
        0x34 => Some("4"),
        0x35 => Some("5"),
        0x36 => Some("6"),
        0x37 => Some("7"),
        0x38 => Some("8"),
        0x39 => Some("9"),
        0x41 => Some("A"),
        0x42 => Some("B"),
        0x43 => Some("C"),
        0x44 => Some("D"),
        0x45 => Some("E"),
        0x46 => Some("F"),
        0x47 => Some("G"),
        0x48 => Some("H"),
        0x49 => Some("I"),
        0x4a => Some("J"),
        0x4b => Some("K"),
        0x4c => Some("L"),
        0x4d => Some("M"),
        0x4e => Some("N"),
        0x4f => Some("O"),
        0x50 => Some("P"),
        0x51 => Some("Q"),
        0x52 => Some("R"),
        0x53 => Some("S"),
        0x54 => Some("T"),
        0x55 => Some("U"),
        0x56 => Some("V"),
        0x57 => Some("W"),
        0x58 => Some("X"),
        0x59 => Some("Y"),
        0x5a => Some("Z"),
        _ => None,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SttConfig {
    #[serde(default)]
    pub compatibility: SttCompatibility,
    pub api_base_url: String,
    pub model: String,
    pub encrypted_api_key: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SttCompatibility {
    #[default]
    OpenAiAudioTranscriptions,
    AliyunQwenAsrChat,
}

impl SttCompatibility {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::OpenAiAudioTranscriptions => "OpenAI /audio/transcriptions",
            Self::AliyunQwenAsrChat => "Aliyun Qwen-ASR /chat/completions",
        }
    }

    pub fn from_combo_index(index: isize) -> Self {
        match index {
            1 => Self::AliyunQwenAsrChat,
            _ => Self::OpenAiAudioTranscriptions,
        }
    }

    pub fn combo_index(&self) -> usize {
        match self {
            Self::OpenAiAudioTranscriptions => 0,
            Self::AliyunQwenAsrChat => 1,
        }
    }
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            compatibility: SttCompatibility::OpenAiAudioTranscriptions,
            api_base_url: "https://api.openai.com/v1".to_string(),
            model: "whisper-1".to_string(),
            encrypted_api_key: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LlmConfig {
    pub enabled: bool,
    pub api_base_url: String,
    pub model: String,
    pub encrypted_api_key: Option<String>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o-mini".to_string(),
            encrypted_api_key: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub language: Language,
    pub hotkey: Hotkey,
    pub start_at_login: bool,
    pub stt: SttConfig,
    pub llm: LlmConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: Language::ZhCn,
            hotkey: Hotkey::RightCtrl,
            start_at_login: false,
            stt: SttConfig::default(),
            llm: LlmConfig::default(),
        }
    }
}

#[derive(Debug)]
pub struct ConfigStore {
    path: PathBuf,
    pub config: Config,
}

impl ConfigStore {
    pub fn load_or_default() -> anyhow::Result<Self> {
        let mut path = crate::util::app_config_dir()?;
        path.push("config.toml");
        migrate_legacy_config_if_needed(&path);
        if !path.exists() {
            let store = Self {
                path,
                config: Config::default(),
            };
            store.save()?;
            return Ok(store);
        }
        let content = std::fs::read_to_string(&path)?;
        let config = toml::from_str(&content).unwrap_or_else(|err| {
            tracing::warn!(%err, "config parse failed; using defaults");
            Config::default()
        });
        Ok(Self { path, config })
    }

    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(&self.config)?;
        std::fs::write(&self.path, content)?;
        Ok(())
    }
}

fn migrate_legacy_config_if_needed(new_path: &std::path::Path) {
    if new_path.exists() {
        return;
    }
    let Some(mut old_path) = dirs::config_dir() else {
        return;
    };
    old_path.push("VoiceTray");
    old_path.push("config.toml");
    if !old_path.exists() {
        return;
    }
    if let Some(parent) = new_path.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            tracing::warn!(%err, path = %parent.display(), "failed to create JustSay config directory");
            return;
        }
    }
    match std::fs::copy(&old_path, new_path) {
        Ok(_) => tracing::info!(
            from = %old_path.display(),
            to = %new_path.display(),
            "migrated legacy VoiceTray config to JustSay"
        ),
        Err(err) => {
            tracing::warn!(%err, from = %old_path.display(), to = %new_path.display(), "failed to migrate legacy config")
        }
    }
}
