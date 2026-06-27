use crate::{
    app::{ApiSettingsInput, AppController},
    config::{Hotkey, SttCompatibility},
    util::{string_from_wide, wide_null},
};
use parking_lot::Mutex;
use std::sync::{Arc, OnceLock};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowTextLengthW, GetWindowTextW,
    MessageBoxW, RegisterClassW, SendMessageW, SetWindowTextW, ShowWindow, CBN_SELCHANGE,
    CBS_DROPDOWNLIST, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CW_USEDEFAULT, ES_AUTOHSCROLL,
    ES_PASSWORD, HMENU, MB_ICONERROR, MB_ICONINFORMATION, SW_SHOW, WM_CLOSE, WM_COMMAND, WM_CREATE,
    WM_NCDESTROY, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_EX_DLGMODALFRAME, WS_OVERLAPPED,
    WS_SYSMENU, WS_VISIBLE, WS_VSCROLL,
};

const CLASS_NAME: &str = "JustSaySettingsWindow";
const ID_BASE: isize = 101;
const ID_KEY: isize = 102;
const ID_MODEL: isize = 103;
const ID_STT_BASE: isize = 111;
const ID_STT_KEY: isize = 112;
const ID_STT_MODEL: isize = 113;
const ID_STT_COMPAT: isize = 114;
const ID_HOTKEY: isize = 115;
const ID_TEST: isize = 201;
const ID_SAVE: isize = 202;

#[derive(Default)]
struct SettingsState {
    hwnd: isize,
    hotkey_combo: isize,
    stt_compat_combo: isize,
    stt_base_edit: isize,
    stt_key_edit: isize,
    stt_model_edit: isize,
    base_edit: isize,
    key_edit: isize,
    model_edit: isize,
    controller: Option<Arc<AppController>>,
}

static STATE: OnceLock<Mutex<SettingsState>> = OnceLock::new();

pub fn show(controller: Arc<AppController>) {
    let state = STATE.get_or_init(|| Mutex::new(SettingsState::default()));
    {
        let mut lock = state.lock();
        lock.controller = Some(controller.clone());
        if lock.hwnd != 0 {
            unsafe {
                ShowWindow(lock.hwnd as HWND, SW_SHOW);
            }
            return;
        }
    }

    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class = wide_null(CLASS_NAME);
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance,
            lpszClassName: class.as_ptr(),
            hCursor: std::ptr::null_mut(),
            hIcon: std::ptr::null_mut(),
            hbrBackground: (windows_sys::Win32::Graphics::Gdi::COLOR_WINDOW + 1) as _,
            lpszMenuName: std::ptr::null(),
            cbClsExtra: 0,
            cbWndExtra: 0,
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_DLGMODALFRAME,
            class.as_ptr(),
            wide_null("JustSay Settings").as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            520,
            444,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            show_message("Settings", "Failed to create settings window");
            return;
        }
        state.lock().hwnd = hwnd as isize;
        populate_from_config(&controller);
        ShowWindow(hwnd, SW_SHOW);
    }
}

pub fn show_message(title: &str, body: &str) {
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide_null(body).as_ptr(),
            wide_null(title).as_ptr(),
            if body.starts_with("Failed") {
                MB_ICONERROR
            } else {
                MB_ICONINFORMATION
            },
        );
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            create_controls(hwnd);
            0
        }
        WM_COMMAND => {
            let id = (wparam & 0xffff) as isize;
            let notify = ((wparam >> 16) & 0xffff) as u32;
            match id {
                ID_STT_COMPAT if notify == CBN_SELCHANGE => apply_stt_preset(),
                ID_TEST => test_current(),
                ID_SAVE => save_current(),
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_NCDESTROY => {
            if let Some(state) = STATE.get() {
                let mut lock = state.lock();
                lock.hwnd = 0;
                lock.hotkey_combo = 0;
                lock.stt_compat_combo = 0;
                lock.stt_base_edit = 0;
                lock.stt_key_edit = 0;
                lock.stt_model_edit = 0;
                lock.base_edit = 0;
                lock.key_edit = 0;
                lock.model_edit = 0;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn create_controls(hwnd: HWND) {
    label(hwnd, "Hotkey", 20, 24);
    label(hwnd, "STT Mode", 20, 68);
    label(hwnd, "STT Base URL", 20, 112);
    label(hwnd, "STT API Key", 20, 156);
    label(hwnd, "STT Model", 20, 200);
    label(hwnd, "LLM Base URL", 20, 254);
    label(hwnd, "LLM API Key", 20, 298);
    label(hwnd, "LLM Model", 20, 342);
    let hotkey = hotkey_combo(hwnd, ID_HOTKEY, 140, 20, 340, 120);
    let stt_compat = stt_combo(hwnd, ID_STT_COMPAT, 140, 64, 340, 120);
    let stt_base = edit(hwnd, ID_STT_BASE, 140, 108, 340, 24, false);
    let stt_key = edit(hwnd, ID_STT_KEY, 140, 152, 340, 24, true);
    let stt_model = edit(hwnd, ID_STT_MODEL, 140, 196, 340, 24, false);
    let base = edit(hwnd, ID_BASE, 140, 250, 340, 24, false);
    let key = edit(hwnd, ID_KEY, 140, 294, 340, 24, true);
    let model = edit(hwnd, ID_MODEL, 140, 338, 340, 24, false);
    button(hwnd, ID_TEST, "Test LLM", 270, 384, 100, 30);
    button(hwnd, ID_SAVE, "Save", 390, 384, 90, 30);
    if let Some(state) = STATE.get() {
        let mut lock = state.lock();
        lock.hotkey_combo = hotkey as isize;
        lock.stt_compat_combo = stt_compat as isize;
        lock.stt_base_edit = stt_base as isize;
        lock.stt_key_edit = stt_key as isize;
        lock.stt_model_edit = stt_model as isize;
        lock.base_edit = base as isize;
        lock.key_edit = key as isize;
        lock.model_edit = model as isize;
    }
}

fn populate_from_config(controller: &Arc<AppController>) {
    let config = controller.config();
    let stt_key = crate::dpapi::unprotect_from_base64(&config.stt.encrypted_api_key)
        .ok()
        .flatten()
        .unwrap_or_default();
    let key = crate::dpapi::unprotect_from_base64(&config.llm.encrypted_api_key)
        .ok()
        .flatten()
        .unwrap_or_default();
    if let Some(state) = STATE.get() {
        let lock = state.lock();
        unsafe {
            SendMessageW(
                lock.hotkey_combo as HWND,
                CB_SETCURSEL,
                config.hotkey.combo_index(),
                0,
            );
            SetWindowTextW(
                lock.stt_compat_combo as HWND,
                wide_null(config.stt.compatibility.display_name()).as_ptr(),
            );
            SendMessageW(
                lock.stt_compat_combo as HWND,
                CB_SETCURSEL,
                config.stt.compatibility.combo_index(),
                0,
            );
            SetWindowTextW(
                lock.stt_base_edit as HWND,
                wide_null(config.stt.api_base_url).as_ptr(),
            );
            SetWindowTextW(lock.stt_key_edit as HWND, wide_null(stt_key).as_ptr());
            SetWindowTextW(
                lock.stt_model_edit as HWND,
                wide_null(config.stt.model).as_ptr(),
            );
            SetWindowTextW(
                lock.base_edit as HWND,
                wide_null(config.llm.api_base_url).as_ptr(),
            );
            SetWindowTextW(lock.key_edit as HWND, wide_null(key).as_ptr());
            SetWindowTextW(
                lock.model_edit as HWND,
                wide_null(config.llm.model).as_ptr(),
            );
        }
    }
}

fn test_current() {
    let Some(state) = STATE.get() else { return };
    let lock = state.lock();
    let Some(controller) = lock.controller.clone() else {
        return;
    };
    let base = get_text(lock.base_edit as HWND);
    let key = get_text(lock.key_edit as HWND);
    let model = get_text(lock.model_edit as HWND);
    drop(lock);
    controller.test_llm_settings(base, model, key);
}

fn save_current() {
    let Some(state) = STATE.get() else { return };
    let lock = state.lock();
    let Some(controller) = lock.controller.clone() else {
        return;
    };
    let hotkey = selected_hotkey(lock.hotkey_combo as HWND);
    let stt_compat = selected_stt_compatibility(lock.stt_compat_combo as HWND);
    let stt_base = get_text(lock.stt_base_edit as HWND);
    let stt_key = get_text(lock.stt_key_edit as HWND);
    let stt_model = get_text(lock.stt_model_edit as HWND);
    let base = get_text(lock.base_edit as HWND);
    let key = get_text(lock.key_edit as HWND);
    let model = get_text(lock.model_edit as HWND);
    drop(lock);
    controller.set_hotkey(hotkey);
    match controller.update_api_settings(ApiSettingsInput {
        stt_api_base_url: stt_base,
        stt_model,
        stt_api_key_plain: stt_key,
        stt_compatibility: stt_compat,
        llm_api_base_url: base,
        llm_model: model,
        llm_api_key_plain: key,
    }) {
        Ok(()) => show_message("Settings", "Saved"),
        Err(err) => show_message("Settings", &format!("Failed: {err}")),
    }
}

fn get_text(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        let mut buf = vec![0u16; len as usize + 1];
        GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        string_from_wide(&buf)
    }
}

fn selected_stt_compatibility(hwnd: HWND) -> SttCompatibility {
    let index = unsafe { SendMessageW(hwnd, CB_GETCURSEL, 0, 0) };
    SttCompatibility::from_combo_index(index)
}

fn selected_hotkey(hwnd: HWND) -> Hotkey {
    let index = unsafe { SendMessageW(hwnd, CB_GETCURSEL, 0, 0) };
    Hotkey::from_combo_index(index)
}

fn apply_stt_preset() {
    let Some(state) = STATE.get() else { return };
    let lock = state.lock();
    let mode = selected_stt_compatibility(lock.stt_compat_combo as HWND);
    let current_base = get_text(lock.stt_base_edit as HWND);
    let current_model = get_text(lock.stt_model_edit as HWND);
    let should_replace_base = current_base.trim().is_empty()
        || current_base.trim() == "https://api.openai.com/v1"
        || current_base.trim() == "https://dashscope.aliyuncs.com/compatible-mode/v1";
    let should_replace_model = current_model.trim().is_empty()
        || current_model.trim() == "whisper-1"
        || current_model.trim() == "qwen3-asr-flash";

    let (base, model) = match mode {
        SttCompatibility::OpenAiAudioTranscriptions => ("https://api.openai.com/v1", "whisper-1"),
        SttCompatibility::AliyunQwenAsrChat => (
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
            "qwen3-asr-flash",
        ),
    };
    unsafe {
        if should_replace_base {
            SetWindowTextW(lock.stt_base_edit as HWND, wide_null(base).as_ptr());
        }
        if should_replace_model {
            SetWindowTextW(lock.stt_model_edit as HWND, wide_null(model).as_ptr());
        }
    }
}

unsafe fn label(parent: HWND, text: &str, x: i32, y: i32) {
    CreateWindowExW(
        0,
        wide_null("STATIC").as_ptr(),
        wide_null(text).as_ptr(),
        WS_CHILD | WS_VISIBLE,
        x,
        y,
        110,
        22,
        parent,
        std::ptr::null_mut(),
        GetModuleHandleW(std::ptr::null()),
        std::ptr::null(),
    );
}

unsafe fn edit(parent: HWND, id: isize, x: i32, y: i32, w: i32, h: i32, password: bool) -> HWND {
    CreateWindowExW(
        0,
        wide_null("EDIT").as_ptr(),
        wide_null("").as_ptr(),
        WS_CHILD
            | WS_VISIBLE
            | WS_BORDER
            | ES_AUTOHSCROLL as u32
            | if password { ES_PASSWORD as u32 } else { 0 },
        x,
        y,
        w,
        h,
        parent,
        id as HMENU,
        GetModuleHandleW(std::ptr::null()),
        std::ptr::null(),
    )
}

unsafe fn stt_combo(parent: HWND, id: isize, x: i32, y: i32, w: i32, h: i32) -> HWND {
    let hwnd = CreateWindowExW(
        0,
        wide_null("COMBOBOX").as_ptr(),
        wide_null("").as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_VSCROLL | CBS_DROPDOWNLIST as u32,
        x,
        y,
        w,
        h,
        parent,
        id as HMENU,
        GetModuleHandleW(std::ptr::null()),
        std::ptr::null(),
    );
    for item in [
        SttCompatibility::OpenAiAudioTranscriptions,
        SttCompatibility::AliyunQwenAsrChat,
    ] {
        let label = wide_null(item.display_name());
        SendMessageW(hwnd, CB_ADDSTRING, 0, label.as_ptr() as LPARAM);
    }
    hwnd
}

unsafe fn hotkey_combo(parent: HWND, id: isize, x: i32, y: i32, w: i32, h: i32) -> HWND {
    let hwnd = CreateWindowExW(
        0,
        wide_null("COMBOBOX").as_ptr(),
        wide_null("").as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_VSCROLL | CBS_DROPDOWNLIST as u32,
        x,
        y,
        w,
        h,
        parent,
        id as HMENU,
        GetModuleHandleW(std::ptr::null()),
        std::ptr::null(),
    );
    for item in [
        Hotkey::RightCtrl,
        Hotkey::CapsLock,
        Hotkey::RightAlt,
        Hotkey::CtrlSpace,
    ] {
        let label = wide_null(item.display_name());
        SendMessageW(hwnd, CB_ADDSTRING, 0, label.as_ptr() as LPARAM);
    }
    hwnd
}

unsafe fn button(parent: HWND, id: isize, text: &str, x: i32, y: i32, w: i32, h: i32) {
    CreateWindowExW(
        0,
        wide_null("BUTTON").as_ptr(),
        wide_null(text).as_ptr(),
        WS_CHILD | WS_VISIBLE,
        x,
        y,
        w,
        h,
        parent,
        id as HMENU,
        GetModuleHandleW(std::ptr::null()),
        std::ptr::null(),
    );
}
