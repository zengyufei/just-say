use crate::{
    app::AppController,
    understanding::{UnderstandingDecision, UnderstandingKind},
    util::wide_null,
};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
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

    pub fn execute(&self, decision: &UnderstandingDecision) -> anyhow::Result<ActionResult> {
        match decision.kind {
            UnderstandingKind::WebSearch => {
                let query = sanitize_action_target(&decision.action_target)?;
                let url = format!("https://www.bing.com/search?q={}", percent_encode(query));
                open_target(&url)?;
                Ok(ActionResult {
                    display: format!("已打开搜索: {query}"),
                })
            }
            UnderstandingKind::OpenUrl => {
                let url = sanitize_action_target(&decision.action_target)?;
                let normalized = normalize_url(url)?;
                open_target(&normalized)?;
                Ok(ActionResult {
                    display: format!("已打开网址: {normalized}"),
                })
            }
            UnderstandingKind::OpenApp => {
                let app_name = sanitize_app_name(&decision.action_target)?;
                if !confirm_open_app(&app_name) {
                    return Ok(ActionResult {
                        display: format!("已取消打开应用: {app_name}"),
                    });
                }
                if let Some(target) = resolve_app_target(&app_name) {
                    tracing::info!(app_name = %app_name, target = %target.display(), "open_app_resolved");
                    open_target(target.as_os_str())?;
                } else {
                    tracing::info!(app_name = %app_name, "open_app_using_raw_name");
                    open_target(OsStr::new(&app_name))?;
                }
                Ok(ActionResult {
                    display: format!("已打开应用: {app_name}"),
                })
            }
            UnderstandingKind::OpenSettings => {
                crate::settings::show(self.controller.clone());
                Ok(ActionResult {
                    display: "已打开 JustSay 设置".to_string(),
                })
            }
            UnderstandingKind::OpenLogs => {
                crate::tray::open_logs();
                Ok(ActionResult {
                    display: "已打开 JustSay 日志".to_string(),
                })
            }
            UnderstandingKind::Dictation
            | UnderstandingKind::Repair
            | UnderstandingKind::Compose
            | UnderstandingKind::Rewrite
            | UnderstandingKind::Note
            | UnderstandingKind::Unsupported => {
                anyhow::bail!("decision is not executable")
            }
        }
    }
}

fn open_target(target: impl AsRef<OsStr>) -> anyhow::Result<()> {
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

fn resolve_app_target(app_name: &str) -> Option<PathBuf> {
    let normalized = normalize_for_match(app_name);
    let candidates = vec![normalized];

    for root in start_menu_roots() {
        if let Some(path) = find_matching_shortcut(&root, &candidates) {
            return Some(path);
        }
    }
    None
}

fn start_menu_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(program_data) = std::env::var_os("ProgramData") {
        roots.push(
            Path::new(&program_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }
    if let Some(app_data) = std::env::var_os("APPDATA") {
        roots.push(
            Path::new(&app_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }
    roots
}

fn find_matching_shortcut(dir: &Path, candidates: &[String]) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_matching_shortcut(&path, candidates) {
                return Some(found);
            }
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        let ext = ext.to_ascii_lowercase();
        if !matches!(ext.as_str(), "lnk" | "url" | "exe") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let normalized = normalize_for_match(stem);
        if candidates.iter().any(|candidate| {
            !candidate.is_empty()
                && (normalized.contains(candidate) || candidate.contains(&normalized))
        }) {
            return Some(path);
        }
    }
    None
}

fn normalize_for_match(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| ch.to_lowercase())
        .filter(|ch| ch.is_alphanumeric())
        .collect()
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

fn sanitize_action_target(value: &str) -> anyhow::Result<&str> {
    let value = value.trim();
    if value.is_empty() {
        anyhow::bail!("action target is empty");
    }
    Ok(value)
}

fn confirm_open_app(app_name: &str) -> bool {
    let body = format!("JustSay 识别到要打开应用:\n\n{app_name}\n\n是否继续？");
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide_null(body).as_ptr(),
            wide_null("JustSay Smart Understanding").as_ptr(),
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
