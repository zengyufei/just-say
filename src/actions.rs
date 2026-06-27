use crate::{app::AppController, intent::IntentDecision, util::wide_null};
use std::sync::Arc;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONQUESTION, MB_YESNO, SW_SHOWNORMAL,
};

#[derive(Debug)]
pub struct ActionResult {
    pub display: String,
}

pub struct ActionExecutor {
    controller: Arc<AppController>,
}

impl ActionExecutor {
    pub fn new(controller: Arc<AppController>) -> Self {
        Self { controller }
    }

    pub fn execute(&self, decision: &IntentDecision) -> anyhow::Result<ActionResult> {
        match decision {
            IntentDecision::WebSearch { query } => {
                let url = format!("https://www.bing.com/search?q={}", percent_encode(query));
                open_target(&url)?;
                Ok(ActionResult {
                    display: format!("已打开搜索: {query}"),
                })
            }
            IntentDecision::OpenUrl { url } => {
                let normalized = normalize_url(url)?;
                open_target(&normalized)?;
                Ok(ActionResult {
                    display: format!("已打开网址: {normalized}"),
                })
            }
            IntentDecision::OpenApp { app_name } => {
                let app_name = sanitize_app_name(app_name)?;
                if !confirm_open_app(&app_name) {
                    return Ok(ActionResult {
                        display: format!("已取消打开应用: {app_name}"),
                    });
                }
                open_target(&app_name)?;
                Ok(ActionResult {
                    display: format!("已打开应用: {app_name}"),
                })
            }
            IntentDecision::OpenSettings => {
                crate::settings::show(self.controller.clone());
                Ok(ActionResult {
                    display: "已打开 JustSay 设置".to_string(),
                })
            }
            IntentDecision::OpenLogs => {
                crate::tray::open_logs();
                Ok(ActionResult {
                    display: "已打开 JustSay 日志".to_string(),
                })
            }
            IntentDecision::TextInput | IntentDecision::Unsupported { .. } => {
                anyhow::bail!("decision is not executable")
            }
        }
    }
}

fn open_target(target: &str) -> anyhow::Result<()> {
    unsafe {
        let open = wide_null("open");
        let target = wide_null(target);
        let result = ShellExecuteW(
            std::ptr::null_mut(),
            open.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
        if (result as isize) <= 32 {
            anyhow::bail!("ShellExecuteW failed: {}", result as isize);
        }
    }
    Ok(())
}

fn normalize_url(value: &str) -> anyhow::Result<String> {
    let value = value.trim();
    let normalized = if value.starts_with("https://") || value.starts_with("http://") {
        value.to_string()
    } else {
        format!("https://{value}")
    };
    let lowered = normalized.to_ascii_lowercase();
    if lowered.starts_with("https://") || lowered.starts_with("http://") {
        Ok(normalized)
    } else {
        anyhow::bail!("only http/https URLs are supported")
    }
}

fn sanitize_app_name(value: &str) -> anyhow::Result<String> {
    let app_name = value.trim();
    if app_name.is_empty() {
        anyhow::bail!("application name is empty");
    }
    if app_name.chars().any(|ch| {
        matches!(
            ch,
            '"' | '\'' | '`' | '|' | '&' | '<' | '>' | ';' | ':' | '/' | '\\' | '\n' | '\r'
        )
    }) {
        anyhow::bail!("application name contains unsupported characters");
    }
    Ok(app_name.to_string())
}

fn confirm_open_app(app_name: &str) -> bool {
    let body = format!("JustSay 识别到要打开应用:\n\n{app_name}\n\n是否继续？");
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide_null(body).as_ptr(),
            wide_null("JustSay Voice Actions").as_ptr(),
            MB_YESNO | MB_ICONQUESTION,
        ) == windows_sys::Win32::UI::WindowsAndMessaging::IDYES
    }
}

fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '~') {
            out.push(ch);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}
