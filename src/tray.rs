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

const CLASS_NAME: &str = "VoiceTrayHiddenWindow";
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
            wide_null("VoiceTray").as_ptr(),
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
        let tip = wide_null("VoiceTray");
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
    let menu = CreatePopupMenu();
    let lang_menu = CreatePopupMenu();
    let hotkey_menu = CreatePopupMenu();
    let llm_menu = CreatePopupMenu();

    append_disabled(menu, &format!("Status: {}", controller.status()));
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
        wide_null("Language").as_ptr(),
    );

    append_checked(
        hotkey_menu,
        CMD_HOTKEY_RCTRL,
        "Right Ctrl",
        config.hotkey == Hotkey::RightCtrl,
    );
    append_checked(
        hotkey_menu,
        CMD_HOTKEY_CAPS,
        "CapsLock",
        config.hotkey == Hotkey::CapsLock,
    );
    append_checked(
        hotkey_menu,
        CMD_HOTKEY_RALT,
        "Right Alt",
        config.hotkey == Hotkey::RightAlt,
    );
    append_checked(
        hotkey_menu,
        CMD_HOTKEY_CTRL_SPACE,
        "Ctrl+Space",
        config.hotkey == Hotkey::CtrlSpace,
    );
    AppendMenuW(
        menu,
        MF_POPUP,
        hotkey_menu as usize,
        wide_null("Hotkey").as_ptr(),
    );

    append_checked(
        llm_menu,
        CMD_LLM_ENABLE,
        if config.llm.enabled {
            "Disable"
        } else {
            "Enable"
        },
        config.llm.enabled,
    );
    AppendMenuW(
        llm_menu,
        MF_STRING,
        CMD_LLM_SETTINGS as usize,
        wide_null("Settings").as_ptr(),
    );
    AppendMenuW(
        menu,
        MF_POPUP,
        llm_menu as usize,
        wide_null("LLM Refinement").as_ptr(),
    );

    append_checked(menu, CMD_STARTUP, "Start at login", config.start_at_login);
    AppendMenuW(
        menu,
        MF_STRING,
        CMD_SETTINGS as usize,
        wide_null("Settings").as_ptr(),
    );
    AppendMenuW(
        menu,
        MF_STRING,
        CMD_OPEN_LOGS as usize,
        wide_null("Open Logs").as_ptr(),
    );
    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
    AppendMenuW(
        menu,
        MF_STRING,
        CMD_QUIT as usize,
        wide_null("Quit").as_ptr(),
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
        let name = wide_null("VoiceTray");
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
