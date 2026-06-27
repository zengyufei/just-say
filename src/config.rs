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
}

impl Hotkey {
    pub fn display_name(&self) -> String {
        match self {
            Self::RightCtrl => "Right Ctrl".to_string(),
            Self::CapsLock => "CapsLock".to_string(),
            Self::RightAlt => "Right Alt".to_string(),
            Self::CtrlSpace => "Ctrl+Space".to_string(),
            Self::FnScanCode { scan_code } => format!("Fn scan code 0x{scan_code:x}"),
        }
    }

    pub fn from_combo_index(index: isize) -> Self {
        match index {
            1 => Self::CapsLock,
            2 => Self::RightAlt,
            3 => Self::CtrlSpace,
            _ => Self::RightCtrl,
        }
    }

    pub fn combo_index(&self) -> usize {
        match self {
            Self::RightCtrl => 0,
            Self::CapsLock => 1,
            Self::RightAlt => 2,
            Self::CtrlSpace => 3,
            Self::FnScanCode { .. } => 0,
        }
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
