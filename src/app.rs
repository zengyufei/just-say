use crate::{
    actions::ActionExecutor,
    audio::Recorder,
    config::{Config, ConfigStore, Hotkey, Language},
    refiner::OpenAiRefiner,
    transcriber::OpenAiTranscriber,
    understanding::{
        RecentInteraction, UnderstandingKind, UnderstandingRequest, UnderstandingRouter,
    },
};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

const RECENT_CONTEXT_LIMIT: usize = 5;
const UNDERSTANDING_ACCEPT_CONFIDENCE: u8 = 85;

pub struct AppController {
    config: Mutex<ConfigStore>,
    recorder: Mutex<Option<Recorder>>,
    busy: AtomicBool,
    status: Mutex<String>,
    stats: Mutex<AppStats>,
    recent_context: Mutex<VecDeque<RecentInteraction>>,
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
    pub actions_enabled: bool,
}

enum SmartUnderstandingResult {
    Text {
        text: String,
        kind: UnderstandingKind,
    },
    ActionHandled,
    Fallback,
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
            recent_context: Mutex::new(VecDeque::with_capacity(RECENT_CONTEXT_LIMIT)),
            runtime,
        }
    }

    pub fn start_recording(self: &Arc<Self>) -> bool {
        if self.busy.swap(true, Ordering::SeqCst) {
            return false;
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
                true
            }
            Err(err) => {
                tracing::error!(%err, "failed to start microphone recording");
                self.fail("麦克风不可用", "Microphone unavailable");
                false
            }
        }
    }

    pub fn stop_recording(self: &Arc<Self>) {
        let Some(recorder) = self.recorder.lock().take() else {
            return;
        };
        crate::overlay::set_text("Transcribing...");
        let audio = match recorder.stop() {
            Ok(audio) => audio,
            Err(err) => {
                tracing::error!(%err, "recording failed");
                self.fail("录音失败", "Recording failed");
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

        let (final_text, final_kind) = match self.try_smart_understanding(&config, &raw_text).await
        {
            SmartUnderstandingResult::Text { text, kind } => (text, kind),
            SmartUnderstandingResult::ActionHandled => return,
            SmartUnderstandingResult::Fallback => (
                self.refine_traditional_text(&config, &raw_text).await,
                UnderstandingKind::Dictation,
            ),
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
        self.remember_interaction(
            raw_text.clone(),
            final_kind,
            final_text.clone(),
            String::new(),
        );
        self.set_status("Ready");
        crate::overlay::set_text("Done");
        tokio::time::sleep(std::time::Duration::from_millis(520)).await;
        crate::overlay::hide();
        self.busy.store(false, Ordering::SeqCst);
    }

    async fn try_smart_understanding(
        self: &Arc<Self>,
        config: &Config,
        raw_text: &str,
    ) -> SmartUnderstandingResult {
        if !config.actions.enabled {
            return SmartUnderstandingResult::Fallback;
        }
        let llm_key = match crate::dpapi::unprotect_from_base64(&config.llm.encrypted_api_key) {
            Ok(Some(key)) if !key.trim().is_empty() => key,
            Ok(_) => {
                tracing::info!(before = %raw_text, "smart_understanding_skipped_missing_llm_key");
                crate::overlay::set_debug_texts(raw_text, "智能理解需要 LLM API Key，改为输入文本");
                return SmartUnderstandingResult::Fallback;
            }
            Err(err) => {
                tracing::warn!(%err, before = %raw_text, "smart_understanding_key_decrypt_failed");
                crate::overlay::set_debug_texts(raw_text, "智能理解配置错误，改为输入文本");
                return SmartUnderstandingResult::Fallback;
            }
        };
        if config.llm.api_base_url.trim().is_empty() || config.llm.model.trim().is_empty() {
            tracing::info!(before = %raw_text, "smart_understanding_skipped_incomplete_llm_config");
            crate::overlay::set_debug_texts(raw_text, "智能理解 LLM 配置不完整，改为输入文本");
            return SmartUnderstandingResult::Fallback;
        }

        crate::overlay::set_text("Understanding...");
        crate::overlay::set_debug_texts(raw_text, "正在理解...");
        let request = UnderstandingRequest {
            raw_text: raw_text.to_string(),
            language: config.language.clone(),
            foreground_window_title: foreground_window_title(),
            recent_context: self.recent_context_snapshot(),
        };
        let router = UnderstandingRouter {
            api_base_url: config.llm.api_base_url.clone(),
            api_key: llm_key,
            model: config.llm.model.clone(),
        };
        let outcome = match router.understand(&request).await {
            Ok(outcome) => outcome,
            Err(err) => {
                tracing::warn!(
                    %err,
                    before = %raw_text,
                    fallback_path = "traditional_refine",
                    "smart_understanding_failed"
                );
                crate::overlay::set_debug_texts(raw_text, "智能理解失败，改为输入文本");
                return SmartUnderstandingResult::Fallback;
            }
        };

        let kind = outcome.decision.kind;
        let kind_label = kind.label();
        tracing::info!(
            before = %raw_text,
            understanding_kind = kind_label,
            understanding_confidence = outcome.decision.confidence,
            understanding_json = %outcome.raw_json,
            action_target = %outcome.decision.action_target,
            "smart_understanding_decision"
        );

        if kind == UnderstandingKind::Unsupported {
            tracing::info!(
                before = %raw_text,
                fallback_path = "traditional_refine",
                reason = %outcome.decision.reason,
                "smart_understanding_unsupported_fallback"
            );
            crate::overlay::set_debug_texts(raw_text, "暂不支持这个动作，改为输入文本");
            return SmartUnderstandingResult::Fallback;
        }

        if kind.is_text_output() {
            let mut final_text = outcome.decision.final_text.clone();
            let mut used_second_pass = false;
            if outcome.decision.confidence < UNDERSTANDING_ACCEPT_CONFIDENCE {
                if let Some(refiner) = self.refiner_from_config(config) {
                    match refiner
                        .repair_understanding(raw_text, &final_text, &outcome.decision.reason)
                        .await
                    {
                        Ok(report) if !report.final_text.is_empty() => {
                            tracing::info!(
                                before = %raw_text,
                                understanding_kind = kind_label,
                                understanding_confidence = outcome.decision.confidence,
                                used_second_pass = true,
                                second_after = %report.final_text,
                                second_score = report.second_score.unwrap_or(report.first_score),
                                second_reason = report.second_reason.as_deref().unwrap_or(&report.first_reason),
                                "smart_understanding_text_repaired"
                            );
                            final_text = report.final_text;
                            used_second_pass = true;
                        }
                        Ok(_) => tracing::warn!(
                            before = %raw_text,
                            fallback_path = "understanding_final_text",
                            "smart_understanding_repair_empty"
                        ),
                        Err(err) => tracing::warn!(
                            %err,
                            before = %raw_text,
                            fallback_path = "understanding_final_text",
                            "smart_understanding_repair_failed"
                        ),
                    }
                } else {
                    tracing::info!(
                        before = %raw_text,
                        fallback_path = "understanding_final_text",
                        "smart_understanding_low_confidence_without_refiner"
                    );
                }
            }
            tracing::info!(
                before = %raw_text,
                understanding_kind = kind_label,
                understanding_confidence = outcome.decision.confidence,
                used_second_pass,
                after = %final_text,
                "smart_understanding_text_result"
            );
            let debug_text = if used_second_pass {
                format!("二次优化\n{final_text}")
            } else {
                final_text.clone()
            };
            crate::overlay::set_debug_texts(raw_text, &debug_text);
            return SmartUnderstandingResult::Text {
                text: final_text,
                kind,
            };
        }

        if outcome.decision.confidence < UNDERSTANDING_ACCEPT_CONFIDENCE {
            tracing::info!(
                before = %raw_text,
                understanding_kind = kind_label,
                understanding_confidence = outcome.decision.confidence,
                fallback_path = "traditional_refine",
                "smart_understanding_low_confidence_action_fallback"
            );
            crate::overlay::set_debug_texts(raw_text, "动作理解不够确定，改为输入文本");
            return SmartUnderstandingResult::Fallback;
        }
        let executor = ActionExecutor::new(self.clone());
        match executor.execute(&outcome.decision) {
            Ok(result) => {
                tracing::info!(
                    before = %raw_text,
                    understanding_kind = kind_label,
                    understanding_confidence = outcome.decision.confidence,
                    result = %result.display,
                    "smart_understanding_action_executed"
                );
                {
                    let mut stats = self.stats.lock();
                    stats.last_final_chars = 0;
                    stats.last_error = None;
                }
                crate::overlay::set_text("Done");
                crate::overlay::set_debug_texts(
                    raw_text,
                    &format!("{}\n{}", outcome.decision.intent_summary, result.display),
                );
                self.remember_interaction(
                    raw_text.to_string(),
                    kind,
                    String::new(),
                    result.display.clone(),
                );
                self.set_status("Smart action executed");
                tokio::time::sleep(std::time::Duration::from_millis(900)).await;
                crate::overlay::hide();
                self.busy.store(false, Ordering::SeqCst);
                SmartUnderstandingResult::ActionHandled
            }
            Err(err) => {
                tracing::warn!(
                    %err,
                    before = %raw_text,
                    understanding_kind = kind_label,
                    fallback_path = "traditional_refine",
                    "smart_understanding_action_failed_fallback_to_text"
                );
                crate::overlay::set_debug_texts(
                    raw_text,
                    &format!("动作执行失败，改为输入文本: {err}"),
                );
                SmartUnderstandingResult::Fallback
            }
        }
    }

    async fn refine_traditional_text(&self, config: &Config, raw_text: &str) -> String {
        if config.llm.enabled {
            match self.refiner_from_config(config) {
                Some(refiner) => {
                    crate::overlay::set_debug_texts(raw_text, "LLM 正在优化...");
                    tracing::info!(before = %raw_text, "llm_refine_start");
                    crate::overlay::set_text("Refining...");
                    match refiner.refine_detailed(raw_text).await {
                        Ok(report) if !report.final_text.is_empty() => {
                            tracing::info!(
                                before = %raw_text,
                                first_after = %report.first_text,
                                first_score = report.first_score,
                                first_reason = %report.first_reason,
                                second_after = report.second_text.as_deref().unwrap_or(""),
                                second_score = report.second_score.unwrap_or(0),
                                second_reason = report.second_reason.as_deref().unwrap_or(""),
                                after = %report.final_text,
                                "llm_refine_result"
                            );
                            let debug_text = if let Some(second_text) = &report.second_text {
                                format!("二次优化\n{second_text}")
                            } else {
                                report.final_text.clone()
                            };
                            tracing::info!(
                                before = %raw_text,
                                score = report.first_score,
                                used_second_pass = report.second_text.is_some(),
                                "llm_refine_scored"
                            );
                            crate::overlay::set_debug_texts(raw_text, &debug_text);
                            report.final_text
                        }
                        Ok(_) => {
                            tracing::warn!(before = %raw_text, "llm_refine_empty_result");
                            crate::overlay::set_debug_texts(raw_text, "LLM 返回为空，使用识别内容");
                            raw_text.to_string()
                        }
                        Err(err) => {
                            tracing::warn!(
                                %err,
                                before = %raw_text,
                                "LLM refinement failed; using raw STT text"
                            );
                            crate::overlay::set_text("Refine failed, using transcript");
                            crate::overlay::set_debug_texts(raw_text, "LLM 优化失败，使用识别内容");
                            raw_text.to_string()
                        }
                    }
                }
                None => {
                    tracing::info!(before = %raw_text, "llm_refine_skipped_missing_key_or_config");
                    crate::overlay::set_debug_texts(raw_text, "LLM 配置不完整，使用识别内容");
                    raw_text.to_string()
                }
            }
        } else {
            tracing::info!(before = %raw_text, "llm_refine_disabled");
            crate::overlay::set_debug_texts(raw_text, "LLM 未启用");
            raw_text.to_string()
        }
    }

    fn refiner_from_config(&self, config: &Config) -> Option<OpenAiRefiner> {
        if config.llm.api_base_url.trim().is_empty() || config.llm.model.trim().is_empty() {
            return None;
        }
        match crate::dpapi::unprotect_from_base64(&config.llm.encrypted_api_key) {
            Ok(Some(key)) if !key.trim().is_empty() => Some(OpenAiRefiner {
                api_base_url: config.llm.api_base_url.clone(),
                api_key: key,
                model: config.llm.model.clone(),
            }),
            Ok(_) => None,
            Err(err) => {
                tracing::warn!(%err, "failed to decrypt LLM key");
                None
            }
        }
    }

    fn recent_context_snapshot(&self) -> Vec<RecentInteraction> {
        self.recent_context.lock().iter().cloned().collect()
    }

    fn remember_interaction(
        &self,
        raw_text: String,
        kind: UnderstandingKind,
        final_text: String,
        action_result: String,
    ) {
        let mut context = self.recent_context.lock();
        if context.len() >= RECENT_CONTEXT_LIMIT {
            context.pop_front();
        }
        context.push_back(RecentInteraction {
            raw_text,
            kind,
            final_text,
            action_result,
        });
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

    pub fn set_actions_enabled(&self, enabled: bool) {
        {
            let mut store = self.config.lock();
            store.config.actions.enabled = enabled;
            if let Err(err) = store.save() {
                tracing::error!(%err, "failed to save Smart Understanding state");
            }
        }
        self.set_status(if enabled {
            "Smart understanding enabled"
        } else {
            "Smart understanding disabled"
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
        store.config.actions.enabled = input.actions_enabled;
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

fn foreground_window_title() -> String {
    unsafe {
        let hwnd = windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow();
        if hwnd.is_null() {
            return String::new();
        }
        let len = windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW(
            hwnd,
            buf.as_mut_ptr(),
            buf.len() as i32,
        );
        crate::util::string_from_wide(&buf)
    }
}
