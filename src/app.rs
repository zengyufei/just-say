use crate::{
    audio::Recorder,
    config::{Config, ConfigStore, Hotkey, Language},
    refiner::OpenAiRefiner,
    transcriber::OpenAiTranscriber,
};
use parking_lot::Mutex;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub struct AppController {
    config: Mutex<ConfigStore>,
    recorder: Mutex<Option<Recorder>>,
    busy: AtomicBool,
    status: Mutex<String>,
    stats: Mutex<AppStats>,
    runtime: tokio::runtime::Runtime,
}

#[derive(Clone, Debug, Default)]
pub struct AppStats {
    pub recordings: u64,
    pub stt_successes: u64,
    pub stt_failures: u64,
    pub paste_failures: u64,
    pub total_final_chars: u64,
    pub last_duration_ms: u64,
    pub last_rms_avg: f32,
    pub last_rms_peak: f32,
    pub last_stt_chars: usize,
    pub last_final_chars: usize,
    pub last_error: Option<String>,
}

pub struct ApiSettingsInput {
    pub stt_api_base_url: String,
    pub stt_model: String,
    pub stt_api_key_plain: String,
    pub stt_compatibility: crate::config::SttCompatibility,
    pub llm_api_base_url: String,
    pub llm_model: String,
    pub llm_api_key_plain: String,
}

impl AppController {
    pub fn new(config: ConfigStore, runtime: tokio::runtime::Runtime) -> Self {
        let status = format!(
            "Ready - {} - {}",
            config.config.language.display_name(),
            config.config.hotkey.display_name()
        );
        Self {
            config: Mutex::new(config),
            recorder: Mutex::new(None),
            busy: AtomicBool::new(false),
            status: Mutex::new(status),
            stats: Mutex::new(AppStats::default()),
            runtime,
        }
    }

    pub fn start_recording(self: &Arc<Self>) {
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        tracing::info!("recording started");
        crate::overlay::show("正在聆听...");
        let recorder = Recorder::start(|rms| {
            crate::overlay::set_rms(rms);
        });
        match recorder {
            Ok(recorder) => {
                *self.recorder.lock() = Some(recorder);
                self.set_status("Recording");
            }
            Err(err) => {
                tracing::error!(%err, "failed to start microphone recording");
                crate::overlay::set_text("麦克风不可用");
                self.set_status("Microphone unavailable");
                self.busy.store(false, Ordering::SeqCst);
            }
        }
    }

    pub fn stop_recording(self: &Arc<Self>) {
        let Some(recorder) = self.recorder.lock().take() else {
            self.busy.store(false, Ordering::SeqCst);
            return;
        };
        crate::overlay::set_text("Transcribing...");
        let audio = match recorder.stop() {
            Ok(audio) => audio,
            Err(err) => {
                tracing::error!(%err, "recording failed");
                crate::overlay::set_text("录音失败");
                self.set_status("Recording failed");
                self.busy.store(false, Ordering::SeqCst);
                return;
            }
        };
        tracing::info!(
            duration_ms = audio.duration_ms,
            wav_bytes = audio.wav_bytes.len(),
            rms_avg = audio.rms_avg,
            rms_peak = audio.rms_peak,
            "recording stopped"
        );
        {
            let mut stats = self.stats.lock();
            stats.recordings += 1;
            stats.last_duration_ms = audio.duration_ms;
            stats.last_rms_avg = audio.rms_avg;
            stats.last_rms_peak = audio.rms_peak;
            stats.last_error = None;
        }
        maybe_save_debug_audio(&audio);

        let controller = Arc::clone(self);
        self.runtime.spawn(async move {
            controller.finish_audio(audio).await;
        });
    }

    async fn finish_audio(self: Arc<Self>, audio: crate::audio::AudioChunk) {
        let config = self.config.lock().config.clone();
        let stt_key = match crate::dpapi::unprotect_from_base64(&config.stt.encrypted_api_key) {
            Ok(Some(key)) => key,
            Ok(None) => {
                self.fail("STT API 未配置", "STT API key is not configured");
                return;
            }
            Err(err) => {
                tracing::error!(%err, "failed to decrypt STT key");
                self.fail("STT 配置错误", "STT configuration error");
                return;
            }
        };
        let transcriber = OpenAiTranscriber {
            api_base_url: config.stt.api_base_url.clone(),
            api_key: stt_key,
            model: config.stt.model.clone(),
            compatibility: config.stt.compatibility.clone(),
        };
        let raw_text = match transcriber.transcribe(audio, config.language.clone()).await {
            Ok(text) => text,
            Err(err) => {
                tracing::error!(%err, "STT failed");
                {
                    let mut stats = self.stats.lock();
                    stats.stt_failures += 1;
                    stats.last_error = Some(format!("STT failed: {err}"));
                }
                self.fail("识别失败", "STT failed");
                return;
            }
        };
        if raw_text.is_empty() {
            {
                let mut stats = self.stats.lock();
                stats.stt_failures += 1;
                stats.last_error = Some("No transcript".to_string());
            }
            self.fail("未识别到文字", "No transcript");
            return;
        }
        {
            let mut stats = self.stats.lock();
            stats.stt_successes += 1;
            stats.last_stt_chars = raw_text.chars().count();
            stats.last_error = None;
        }
        tracing::info!(stt_transcript_before_llm = %raw_text, "stt_transcript_before_llm");
        crate::overlay::set_text(&raw_text);

        let final_text = if config.llm.enabled {
            match crate::dpapi::unprotect_from_base64(&config.llm.encrypted_api_key) {
                Ok(Some(key)) if !key.is_empty() => {
                    crate::overlay::set_debug_texts(&raw_text, "LLM 正在优化...");
                    tracing::info!(before = %raw_text, "llm_refine_start");
                    crate::overlay::set_text("Refining...");
                    let refiner = OpenAiRefiner {
                        api_base_url: config.llm.api_base_url.clone(),
                        api_key: key,
                        model: config.llm.model.clone(),
                    };
                    match refiner.refine(&raw_text).await {
                        Ok(text) if !text.is_empty() => {
                            tracing::info!(
                                before = %raw_text,
                                after = %text,
                                "llm_refine_result"
                            );
                            crate::overlay::set_debug_texts(&raw_text, &text);
                            text
                        }
                        Ok(_) => {
                            tracing::warn!(before = %raw_text, "llm_refine_empty_result");
                            crate::overlay::set_debug_texts(
                                &raw_text,
                                "LLM 返回为空，使用识别内容",
                            );
                            raw_text.clone()
                        }
                        Err(err) => {
                            tracing::warn!(
                                %err,
                                before = %raw_text,
                                "LLM refinement failed; using raw STT text"
                            );
                            crate::overlay::set_text("Refine failed, using transcript");
                            crate::overlay::set_debug_texts(
                                &raw_text,
                                "LLM 优化失败，使用识别内容",
                            );
                            raw_text.clone()
                        }
                    }
                }
                Ok(_) => {
                    tracing::info!(before = %raw_text, "llm_refine_skipped_missing_key");
                    crate::overlay::set_debug_texts(&raw_text, "LLM 未配置 API Key");
                    raw_text.clone()
                }
                Err(err) => {
                    tracing::warn!(%err, before = %raw_text, "failed to decrypt LLM key");
                    crate::overlay::set_debug_texts(&raw_text, "LLM 配置错误，使用识别内容");
                    raw_text.clone()
                }
            }
        } else {
            tracing::info!(before = %raw_text, "llm_refine_disabled");
            crate::overlay::set_debug_texts(&raw_text, "LLM 未启用");
            raw_text
        };

        crate::overlay::set_text("Pasting...");
        if let Err(err) = crate::injector::paste_text(&final_text) {
            tracing::error!(%err, "text injection failed");
            {
                let mut stats = self.stats.lock();
                stats.paste_failures += 1;
                stats.last_final_chars = final_text.chars().count();
                stats.last_error = Some(format!("Paste failed: {err}"));
            }
            self.fail("粘贴失败", "Paste failed");
            return;
        }
        {
            let mut stats = self.stats.lock();
            let final_chars = final_text.chars().count();
            stats.last_final_chars = final_chars;
            stats.total_final_chars += final_chars as u64;
            stats.last_error = None;
        }
        self.set_status("Ready");
        crate::overlay::set_text("Done");
        tokio::time::sleep(std::time::Duration::from_millis(520)).await;
        crate::overlay::hide();
        self.busy.store(false, Ordering::SeqCst);
    }

    fn fail(&self, overlay: &str, status: &str) {
        crate::overlay::set_text(overlay);
        self.set_status(status);
        self.busy.store(false, Ordering::SeqCst);
        self.runtime.spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(900)).await;
            crate::overlay::hide();
        });
    }

    pub fn set_status(&self, value: &str) {
        *self.status.lock() = value.to_string();
        tracing::info!(status = value);
    }

    pub fn status(&self) -> String {
        self.status.lock().clone()
    }

    pub fn stats(&self) -> AppStats {
        self.stats.lock().clone()
    }

    pub fn config(&self) -> Config {
        self.config.lock().config.clone()
    }

    pub fn set_language(&self, language: Language) {
        {
            let mut store = self.config.lock();
            store.config.language = language;
            if let Err(err) = store.save() {
                tracing::error!(%err, "failed to save language");
            }
        }
        self.set_status("Language updated");
    }

    pub fn set_hotkey(&self, hotkey: Hotkey) {
        {
            let mut store = self.config.lock();
            store.config.hotkey = hotkey.clone();
            if let Err(err) = store.save() {
                tracing::error!(%err, "failed to save hotkey");
            }
        }
        crate::hotkey::update_hotkey(hotkey);
        self.set_status("Hotkey updated");
    }

    pub fn set_llm_enabled(&self, enabled: bool) {
        {
            let mut store = self.config.lock();
            store.config.llm.enabled = enabled;
            if let Err(err) = store.save() {
                tracing::error!(%err, "failed to save LLM state");
            }
        }
        self.set_status(if enabled {
            "LLM refinement enabled"
        } else {
            "LLM refinement disabled"
        });
    }

    pub fn update_api_settings(&self, input: ApiSettingsInput) -> anyhow::Result<()> {
        let stt_encrypted = crate::dpapi::protect_to_base64(&input.stt_api_key_plain)?;
        let llm_encrypted = crate::dpapi::protect_to_base64(&input.llm_api_key_plain)?;
        let mut store = self.config.lock();
        store.config.stt.compatibility = input.stt_compatibility;
        store.config.stt.api_base_url = input.stt_api_base_url;
        store.config.stt.model = input.stt_model;
        store.config.stt.encrypted_api_key = stt_encrypted;
        store.config.llm.api_base_url = input.llm_api_base_url;
        store.config.llm.model = input.llm_model;
        store.config.llm.encrypted_api_key = llm_encrypted;
        store.save()?;
        self.set_status("API settings saved");
        Ok(())
    }

    pub fn test_llm_settings(&self, api_base_url: String, model: String, api_key_plain: String) {
        let refiner = OpenAiRefiner {
            api_base_url,
            api_key: api_key_plain,
            model,
        };
        self.runtime.spawn(async move {
            match refiner.test().await {
                Ok(()) => crate::settings::show_message("LLM Test", "Success"),
                Err(err) => crate::settings::show_message("LLM Test", &format!("Failed: {err}")),
            }
        });
    }

    pub fn set_start_at_login(&self, enabled: bool) {
        match crate::tray::set_startup_registry(enabled) {
            Ok(()) => {
                let mut store = self.config.lock();
                store.config.start_at_login = enabled;
                if let Err(err) = store.save() {
                    tracing::error!(%err, "failed to save startup state");
                }
                self.set_status(if enabled {
                    "Start at login enabled"
                } else {
                    "Start at login disabled"
                });
            }
            Err(err) => {
                tracing::error!(%err, "failed to update startup registry");
                self.set_status("Failed to update startup");
            }
        }
    }
}

fn maybe_save_debug_audio(audio: &crate::audio::AudioChunk) {
    if std::env::var("JUSTSAY_DEBUG_AUDIO").ok().as_deref() != Some("1") {
        return;
    }
    match crate::util::app_log_dir() {
        Ok(mut path) => {
            path.push("last-recording.wav");
            match std::fs::write(&path, &audio.wav_bytes) {
                Ok(()) => tracing::info!(path = %path.display(), "saved debug recording"),
                Err(err) => tracing::warn!(%err, "failed to save debug recording"),
            }
        }
        Err(err) => tracing::warn!(%err, "failed to resolve log directory for debug audio"),
    }
}
