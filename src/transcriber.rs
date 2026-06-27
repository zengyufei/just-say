use crate::{
    audio::AudioChunk,
    config::{Language, SttCompatibility},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::multipart;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct OpenAiTranscriber {
    pub api_base_url: String,
    pub api_key: String,
    pub model: String,
    pub compatibility: SttCompatibility,
}

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
}

impl OpenAiTranscriber {
    pub async fn transcribe(
        &self,
        audio: AudioChunk,
        language: Language,
    ) -> anyhow::Result<String> {
        if self.api_key.trim().is_empty() {
            anyhow::bail!("STT API key is not configured");
        }
        match self.compatibility {
            SttCompatibility::OpenAiAudioTranscriptions => {
                self.transcribe_openai_audio(audio, language).await
            }
            SttCompatibility::AliyunQwenAsrChat => {
                self.transcribe_aliyun_qwen_asr(audio, language).await
            }
        }
    }

    async fn transcribe_openai_audio(
        &self,
        audio: AudioChunk,
        language: Language,
    ) -> anyhow::Result<String> {
        let url = format!(
            "{}/audio/transcriptions",
            self.api_base_url.trim_end_matches('/')
        );
        let file_part = multipart::Part::bytes(audio.wav_bytes)
            .file_name("speech.wav")
            .mime_str("audio/wav")?;
        let form = multipart::Form::new()
            .text("model", self.model.clone())
            .text("language", language.bcp47().to_string())
            .part("file", file_part);
        let client = reqwest::Client::new();
        let response = client
            .post(url)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("STT API failed with {status}: {body}");
        }
        let parsed: TranscriptionResponse = serde_json::from_str(&body)?;
        Ok(parsed.text.trim().to_string())
    }

    async fn transcribe_aliyun_qwen_asr(
        &self,
        audio: AudioChunk,
        language: Language,
    ) -> anyhow::Result<String> {
        let url = format!(
            "{}/chat/completions",
            self.api_base_url.trim_end_matches('/')
        );
        let data = format!("data:audio/wav;base64,{}", STANDARD.encode(audio.wav_bytes));
        let body = AliyunChatRequest {
            model: self.model.clone(),
            stream: false,
            asr_options: AliyunAsrOptions {
                language: aliyun_language_code(&language).map(str::to_string),
                enable_itn: true,
            },
            messages: vec![AliyunChatMessage {
                role: "user",
                content: vec![AliyunContent::InputAudio {
                    input_audio: AliyunInputAudio {
                        data,
                        format: "wav",
                    },
                }],
            }],
        };
        let client = reqwest::Client::new();
        let response = client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;
        let status = response.status();
        let raw = response.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("Aliyun Qwen-ASR failed with {status}: {raw}");
        }
        let parsed: AliyunChatResponse = serde_json::from_str(&raw)?;
        let Some(choice) = parsed.choices.into_iter().next() else {
            anyhow::bail!("Aliyun Qwen-ASR returned no choices");
        };
        Ok(choice.message.content.trim().to_string())
    }
}

#[derive(Debug, Serialize)]
struct AliyunChatRequest {
    model: String,
    stream: bool,
    asr_options: AliyunAsrOptions,
    messages: Vec<AliyunChatMessage>,
}

#[derive(Debug, Serialize)]
struct AliyunAsrOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,
    enable_itn: bool,
}

#[derive(Debug, Serialize)]
struct AliyunChatMessage {
    role: &'static str,
    content: Vec<AliyunContent>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AliyunContent {
    InputAudio { input_audio: AliyunInputAudio },
}

#[derive(Debug, Serialize)]
struct AliyunInputAudio {
    data: String,
    format: &'static str,
}

#[derive(Debug, Deserialize)]
struct AliyunChatResponse {
    choices: Vec<AliyunChoice>,
}

#[derive(Debug, Deserialize)]
struct AliyunChoice {
    message: AliyunChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct AliyunChoiceMessage {
    content: String,
}

fn aliyun_language_code(language: &Language) -> Option<&'static str> {
    match language {
        Language::EnUs => Some("en"),
        Language::ZhCn | Language::ZhTw => Some("zh"),
        Language::JaJp => Some("ja"),
        Language::KoKr => Some("ko"),
    }
}
