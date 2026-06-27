use crate::{
    app::AppController,
    config::{Hotkey, Language},
    hotkey::{HotkeyEvent, HotkeyHook},
    util::wide_null,
};
use parking_lot::Mutex;
use std::sync::{mpsc, Arc, OnceLock};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegSetValueExW, HKEY_CURRENT_USER,
    KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows_sys::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW,
    GetCursorPos, GetMessageW, LoadIconW, PostQuitMessage, RegisterClassW, SetForegroundWindow,
    TrackPopupMenu, TranslateMessage, CS_HREDRAW, CS_VREDRAW, HMENU, IDI_APPLICATION, MF_CHECKED,
    MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MSG, TPM_BOTTOMALIGN, TPM_LEFTALIGN,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW,
    WS_OVERLAPPED,
};

const CLASS_NAME: &str = "JustSayHiddenWindow";
const TRAY_UID: u32 = 1;
const WM_TRAY: u32 = WM_APP + 1;

const CMD_LANG_EN: u32 = 1001;
const CMD_LANG_ZH_CN: u32 = 1002;
const CMD_LANG_ZH_TW: u32 = 1003;
const CMD_LANG_JA: u32 = 1004;
const CMD_LANG_KO: u32 = 1005;
const CMD_HOTKEY_RCTRL: u32 = 1101;
const CMD_HOTKEY_CAPS: u32 = 1102;
const CMD_HOTKEY_RALT: u32 = 1103;
const CMD_HOTKEY_CTRL_SPACE: u32 = 1104;
const CMD_HOTKEY_CUSTOM: u32 = 1105;
const CMD_LLM_ENABLE: u32 = 1201;
const CMD_LLM_SETTINGS: u32 = 1202;
const CMD_SETTINGS: u32 = 1301;
const CMD_STARTUP: u32 = 1302;
const CMD_OPEN_LOGS: u32 = 1303;
const CMD_QUIT: u32 = 9001;

static CONTROLLER: OnceLock<Arc<AppController>> = OnceLock::new();
static TRAY_HWND: OnceLock<Mutex<isize>> = OnceLock::new();

pub fn run(controller: Arc<AppController>) -> anyhow::Result<()> {
    let _ = CONTROLLER.set(controller.clone());
    crate::overlay::init()?;

    let hwnd = create_hidden_window()?;
    TRAY_HWND.get_or_init(|| Mutex::new(hwnd as isize));
    add_tray_icon(hwnd)?;

    let (tx, rx) = mpsc::channel::<HotkeyEvent>();
    let hook = HotkeyHook::install(tx, controller.config().hotkey.clone())?;
    std::thread::spawn({
        let controller = controller.clone();
        move || {
            for event in rx {
                match event {
                    HotkeyEvent::Pressed => controller.start_recording(),
                    HotkeyEvent::Released => controller.stop_recording(),
                }
            }
        }
    });

    let mut msg: MSG = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    drop(hook);
    remove_tray_icon(hwnd);
    crate::overlay::destroy();
    Ok(())
}

fn create_hidden_window() -> anyhow::Result<HWND> {
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class = wide_null(CLASS_NAME);
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance,
            lpszClassName: class.as_ptr(),
            hCursor: std::ptr::null_mut(),
            hIcon: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            cbClsExtra: 0,
            cbWndExtra: 0,
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide_null("JustSay").as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            anyhow::bail!("CreateWindowExW tray window failed");
        }
        Ok(hwnd)
    }
}

fn add_tray_icon(hwnd: HWND) -> anyhow::Result<()> {
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = TRAY_UID;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = LoadIconW(std::ptr::null_mut(), IDI_APPLICATION);
        let tip = wide_null("JustSay");
        for (dst, src) in nid.szTip.iter_mut().zip(tip.iter()) {
            *dst = *src;
        }
        if Shell_NotifyIconW(NIM_ADD, &nid) == 0 {
            anyhow::bail!("Shell_NotifyIconW(NIM_ADD) failed");
        }
        Ok(())
    }
}

fn remove_tray_icon(hwnd: HWND) {
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = TRAY_UID;
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TRAY => {
            if lparam as u32 == WM_RBUTTONUP || lparam as u32 == WM_LBUTTONUP {
                show_tray_menu(hwnd);
            }
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn show_tray_menu(hwnd: HWND) {
    let Some(controller) = CONTROLLER.get() else {
        return;
    };
    let config = controller.config();
    let ui = crate::i18n::UiText::for_language(&config.language);
    let menu = CreatePopupMenu();
    let lang_menu = CreatePopupMenu();
    let hotkey_menu = CreatePopupMenu();
    let llm_menu = CreatePopupMenu();

    append_disabled(
        menu,
        &format!(
            "{}: {}",
            ui.status,
            crate::i18n::status_label(&config.language, &controller.status())
        ),
    );
    append_disabled(
        menu,
        &format!(
            "{}: {}",
            ui.mic,
            truncate_menu_text(&crate::audio::default_input_device_name(), 64)
        ),
    );
    append_disabled(
        menu,
        &format!(
            "{}: {}",
            ui.hotkey,
            crate::i18n::hotkey_label(&config.language, &config.hotkey)
        ),
    );
    append_disabled(
        menu,
        &format!(
            "{}: {} / {}",
            ui.stt,
            crate::i18n::stt_compat_label(&config.language, &config.stt.compatibility),
            truncate_menu_text(&config.stt.model, 42)
        ),
    );
    append_disabled(
        menu,
        &format!(
            "{}: {} / {}",
            ui.llm,
            if config.llm.enabled {
                ui.enabled
            } else {
                ui.disabled
            },
            truncate_menu_text(&config.llm.model, 42)
        ),
    );
    for line in stats_menu_lines(&config.language, &controller.stats()) {
        append_disabled(menu, &line);
    }
    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());

    append_checked(
        lang_menu,
        CMD_LANG_EN,
        "English",
        config.language == Language::EnUs,
    );
    append_checked(
        lang_menu,
        CMD_LANG_ZH_CN,
        "简体中文",
        config.language == Language::ZhCn,
    );
    append_checked(
        lang_menu,
        CMD_LANG_ZH_TW,
        "繁體中文",
        config.language == Language::ZhTw,
    );
    append_checked(
        lang_menu,
        CMD_LANG_JA,
        "日本語",
        config.language == Language::JaJp,
    );
    append_checked(
        lang_menu,
        CMD_LANG_KO,
        "한국어",
        config.language == Language::KoKr,
    );
    AppendMenuW(
        menu,
        MF_POPUP,
        lang_menu as usize,
        wide_null(ui.language).as_ptr(),
    );

    append_checked(
        hotkey_menu,
        CMD_HOTKEY_RCTRL,
        &crate::i18n::hotkey_label(&config.language, &Hotkey::RightCtrl),
        config.hotkey == Hotkey::RightCtrl,
    );
    append_checked(
        hotkey_menu,
        CMD_HOTKEY_CAPS,
        &crate::i18n::hotkey_label(&config.language, &Hotkey::CapsLock),
        config.hotkey == Hotkey::CapsLock,
    );
    append_checked(
        hotkey_menu,
        CMD_HOTKEY_RALT,
        &crate::i18n::hotkey_label(&config.language, &Hotkey::RightAlt),
        config.hotkey == Hotkey::RightAlt,
    );
    append_checked(
        hotkey_menu,
        CMD_HOTKEY_CTRL_SPACE,
        &crate::i18n::hotkey_label(&config.language, &Hotkey::CtrlSpace),
        config.hotkey == Hotkey::CtrlSpace,
    );
    append_checked(
        hotkey_menu,
        CMD_HOTKEY_CUSTOM,
        match config.language {
            Language::ZhCn => "自定义...",
            Language::ZhTw => "自訂...",
            _ => "Custom...",
        },
        config.hotkey.is_custom_like(),
    );
    AppendMenuW(
        menu,
        MF_POPUP,
        hotkey_menu as usize,
        wide_null(ui.hotkey).as_ptr(),
    );

    append_checked(
        llm_menu,
        CMD_LLM_ENABLE,
        if config.llm.enabled {
            ui.disable
        } else {
            ui.enable
        },
        config.llm.enabled,
    );
    AppendMenuW(
        llm_menu,
        MF_STRING,
        CMD_LLM_SETTINGS as usize,
        wide_null(ui.settings).as_ptr(),
    );
    AppendMenuW(
        menu,
        MF_POPUP,
        llm_menu as usize,
        wide_null(ui.llm_refinement).as_ptr(),
    );

    append_checked(menu, CMD_STARTUP, ui.start_at_login, config.start_at_login);
    AppendMenuW(
        menu,
        MF_STRING,
        CMD_SETTINGS as usize,
        wide_null(ui.settings).as_ptr(),
    );
    AppendMenuW(
        menu,
        MF_STRING,
        CMD_OPEN_LOGS as usize,
        wide_null(ui.open_logs).as_ptr(),
    );
    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
    AppendMenuW(
        menu,
        MF_STRING,
        CMD_QUIT as usize,
        wide_null(ui.quit).as_ptr(),
    );

    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_LEFTALIGN | TPM_BOTTOMALIGN,
        pt.x,
        pt.y,
        0,
        hwnd,
        std::ptr::null(),
    );
    DestroyMenu(menu);
    handle_command(cmd);
}

unsafe fn append_disabled(menu: HMENU, text: &str) {
    AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, wide_null(text).as_ptr());
}

unsafe fn append_checked(menu: HMENU, id: u32, text: &str, checked: bool) {
    let mut flags = MF_STRING;
    if checked {
        flags |= MF_CHECKED;
    }
    AppendMenuW(menu, flags, id as usize, wide_null(text).as_ptr());
}

fn stats_menu_lines(language: &Language, stats: &crate::app::AppStats) -> Vec<String> {
    let ui = crate::i18n::UiText::for_language(language);
    let char_unit = match language {
        Language::ZhCn | Language::ZhTw => "字",
        _ => "chars",
    };
    let mut lines = vec![
        format!(
            "{}: {} {}, {} {}, {} {}",
            ui.stats,
            stats.recordings,
            ui.recordings,
            stats.stt_successes,
            ui.stt_ok,
            stats.stt_failures,
            ui.stt_failed
        ),
        format!(
            "{}: {:.1}s, {} {:.4}, {} {:.4}",
            ui.last_audio,
            stats.last_duration_ms as f32 / 1000.0,
            ui.rms_avg,
            stats.last_rms_avg,
            ui.peak,
            stats.last_rms_peak
        ),
        format!(
            "{}: {} {} {}, {} {} {}, {} {} {}",
            ui.text,
            ui.last_stt_chars,
            stats.last_stt_chars,
            char_unit,
            ui.final_chars,
            stats.last_final_chars,
            char_unit,
            ui.total_chars,
            stats.total_final_chars,
            char_unit
        ),
    ];
    if stats.paste_failures > 0 {
        lines.push(format!("{}: {}", ui.paste_failures, stats.paste_failures));
    }
    if let Some(error) = &stats.last_error {
        lines.push(format!(
            "{}: {}",
            ui.last_error,
            truncate_menu_text(error, 80)
        ));
    }
    lines
}

fn truncate_menu_text(value: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in value.chars().enumerate() {
        if idx >= max_chars {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

fn handle_command(cmd: i32) {
    let Some(controller) = CONTROLLER.get() else {
        return;
    };
    match cmd as u32 {
        CMD_LANG_EN => controller.set_language(Language::EnUs),
        CMD_LANG_ZH_CN => controller.set_language(Language::ZhCn),
        CMD_LANG_ZH_TW => controller.set_language(Language::ZhTw),
        CMD_LANG_JA => controller.set_language(Language::JaJp),
        CMD_LANG_KO => controller.set_language(Language::KoKr),
        CMD_HOTKEY_RCTRL => controller.set_hotkey(Hotkey::RightCtrl),
        CMD_HOTKEY_CAPS => controller.set_hotkey(Hotkey::CapsLock),
        CMD_HOTKEY_RALT => controller.set_hotkey(Hotkey::RightAlt),
        CMD_HOTKEY_CTRL_SPACE => controller.set_hotkey(Hotkey::CtrlSpace),
        CMD_HOTKEY_CUSTOM => crate::settings::show(controller.clone()),
        CMD_LLM_ENABLE => controller.set_llm_enabled(!controller.config().llm.enabled),
        CMD_LLM_SETTINGS | CMD_SETTINGS => crate::settings::show(controller.clone()),
        CMD_OPEN_LOGS => open_logs(),
        CMD_STARTUP => controller.set_start_at_login(!controller.config().start_at_login),
        CMD_QUIT => unsafe {
            if let Some(hwnd) = TRAY_HWND.get() {
                windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(*hwnd.lock() as HWND);
            }
        },
        _ => {}
    }
}

fn open_logs() {
    match crate::util::latest_log_file().or_else(|_| crate::util::app_log_dir()) {
        Ok(path) => unsafe {
            let path = wide_null(path.as_os_str());
            let open = wide_null("open");
            let result = ShellExecuteW(
                std::ptr::null_mut(),
                open.as_ptr(),
                path.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
            );
            if (result as isize) <= 32 {
                tracing::warn!(result = result as isize, "failed to open log file");
            }
        },
        Err(err) => tracing::error!(%err, "failed to resolve log path"),
    }
}

pub fn set_startup_registry(enabled: bool) -> anyhow::Result<()> {
    unsafe {
        let mut key = std::ptr::null_mut();
        let subkey = wide_null("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        let result = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            std::ptr::null_mut(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        );
        if result != 0 {
            anyhow::bail!("RegCreateKeyExW failed: {result}");
        }
        let name = wide_null("JustSay");
        if enabled {
            let exe = crate::util::exe_path()?;
            let value = wide_null(format!("\"{}\"", exe.display()));
            let bytes = std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2);
            let result = RegSetValueExW(
                key,
                name.as_ptr(),
                0,
                REG_SZ,
                bytes.as_ptr(),
                bytes.len() as u32,
            );
            RegCloseKey(key);
            if result != 0 {
                anyhow::bail!("RegSetValueExW failed: {result}");
            }
        } else {
            let result = RegDeleteValueW(key, name.as_ptr());
            RegCloseKey(key);
            if result != 0 {
                tracing::warn!(result, "RegDeleteValueW returned non-zero");
            }
        }
    }
    Ok(())
}
