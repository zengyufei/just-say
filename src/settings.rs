use crate::{
    app::{ApiSettingsInput, AppController},
    config::{Hotkey, Language, SttCompatibility},
    hotkey::{CAPTURE_CANCEL, CAPTURE_OK, WM_HOTKEY_CAPTURED},
    util::{string_from_wide, wide_null},
};
use parking_lot::Mutex;
use std::sync::{Arc, OnceLock};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowTextLengthW,
    GetWindowTextW, MessageBoxW, RegisterClassW, SendMessageW, SetWindowTextW, ShowWindow,
    CBN_SELCHANGE, CBS_DROPDOWNLIST, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CW_USEDEFAULT,
    ES_AUTOHSCROLL, ES_PASSWORD, HMENU, MB_ICONERROR, MB_ICONINFORMATION, SW_SHOW, WM_CLOSE,
    WM_COMMAND, WM_CREATE, WM_NCDESTROY, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD,
    WS_EX_DLGMODALFRAME, WS_OVERLAPPED, WS_SYSMENU, WS_VISIBLE, WS_VSCROLL,
};

const CLASS_NAME: &str = "JustSaySettingsWindow";
const WINDOW_EX_STYLE: u32 = WS_EX_DLGMODALFRAME;
const WINDOW_STYLE: u32 = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
const CLIENT_WIDTH: i32 = 520;
const CLIENT_HEIGHT: i32 = 470;
const ID_BASE: isize = 101;
const ID_KEY: isize = 102;
const ID_MODEL: isize = 103;
const ID_STT_BASE: isize = 111;
const ID_STT_KEY: isize = 112;
const ID_STT_MODEL: isize = 113;
const ID_STT_COMPAT: isize = 114;
const ID_HOTKEY: isize = 115;
const ID_HOTKEY_CAPTURE: isize = 116;
const ID_TEST: isize = 201;
const ID_SAVE: isize = 202;

#[derive(Default)]
struct SettingsState {
    hwnd: isize,
    hotkey_combo: isize,
    hotkey_preview: isize,
    hotkey_capture_button: isize,
    stt_compat_combo: isize,
    stt_base_edit: isize,
    stt_key_edit: isize,
    stt_model_edit: isize,
    base_edit: isize,
    key_edit: isize,
    model_edit: isize,
    pending_hotkey: Option<Hotkey>,
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
        let ui = crate::i18n::UiText::for_language(&controller.config().language);
        let (window_width, window_height) = adjusted_window_size(CLIENT_WIDTH, CLIENT_HEIGHT);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE,
            class.as_ptr(),
            wide_null(ui.settings_title).as_ptr(),
            WINDOW_STYLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            window_width,
            window_height,
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

fn adjusted_window_size(client_width: i32, client_height: i32) -> (i32, i32) {
    unsafe {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: client_width,
            bottom: client_height,
        };
        if AdjustWindowRectEx(&mut rect, WINDOW_STYLE, 0, WINDOW_EX_STYLE) != 0 {
            (rect.right - rect.left, rect.bottom - rect.top)
        } else {
            (client_width, client_height + 56)
        }
    }
}

pub fn show_message(title: &str, body: &str) {
    let (title, body, is_error) = localized_message(title, body);
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide_null(body).as_ptr(),
            wide_null(title).as_ptr(),
            if is_error {
                MB_ICONERROR
            } else {
                MB_ICONINFORMATION
            },
        );
    }
}

fn localized_message(title: &str, body: &str) -> (String, String, bool) {
    let language = current_language();
    let ui = crate::i18n::UiText::for_language(&language);
    let title = match title {
        "Settings" => ui.settings,
        "LLM Test" => ui.llm_test,
        _ => title,
    }
    .to_string();

    let is_error = body.starts_with("Failed");
    let body = if body == "Saved" {
        ui.saved.to_string()
    } else if body == "Success" {
        ui.success.to_string()
    } else if body == "Failed to create settings window" {
        ui.failed_to_create_settings.to_string()
    } else if let Some(rest) = body.strip_prefix("Failed:") {
        format!("{}:{}", ui.failed_prefix, rest)
    } else {
        body.to_string()
    };

    (title, body, is_error)
}

fn current_language() -> Language {
    STATE
        .get()
        .and_then(|state| {
            state
                .lock()
                .controller
                .as_ref()
                .map(|c| c.config().language)
        })
        .unwrap_or_default()
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
                ID_HOTKEY if notify == CBN_SELCHANGE => apply_hotkey_preset(),
                ID_HOTKEY_CAPTURE => begin_hotkey_capture(hwnd),
                ID_STT_COMPAT if notify == CBN_SELCHANGE => apply_stt_preset(),
                ID_TEST => test_current(),
                ID_SAVE => save_current(),
                _ => {}
            }
            0
        }
        WM_HOTKEY_CAPTURED => {
            finish_hotkey_capture(wparam);
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
                lock.hotkey_preview = 0;
                lock.hotkey_capture_button = 0;
                lock.stt_compat_combo = 0;
                lock.stt_base_edit = 0;
                lock.stt_key_edit = 0;
                lock.stt_model_edit = 0;
                lock.base_edit = 0;
                lock.key_edit = 0;
                lock.model_edit = 0;
                lock.pending_hotkey = None;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn create_controls(hwnd: HWND) {
    let language = current_language();
    let ui = crate::i18n::UiText::for_language(&language);
    label(hwnd, ui.hotkey, 20, 24);
    label(hwnd, ui.hotkey_preset, 20, 56);
    label(hwnd, ui.stt_mode, 20, 100);
    label(hwnd, ui.stt_base_url, 20, 144);
    label(hwnd, ui.stt_api_key, 20, 188);
    label(hwnd, ui.stt_model, 20, 232);
    label(hwnd, ui.llm_base_url, 20, 286);
    label(hwnd, ui.llm_api_key, 20, 330);
    label(hwnd, ui.llm_model, 20, 374);
    let hotkey_preview = static_text(hwnd, 140, 20, 220, 24, true);
    let hotkey_capture_button =
        button(hwnd, ID_HOTKEY_CAPTURE, ui.capture_hotkey, 370, 20, 110, 28);
    let hotkey = hotkey_combo(hwnd, ID_HOTKEY, 140, 52, 340, 120);
    let stt_compat = stt_combo(hwnd, ID_STT_COMPAT, 140, 96, 340, 120);
    let stt_base = edit(hwnd, ID_STT_BASE, 140, 140, 340, 24, false);
    let stt_key = edit(hwnd, ID_STT_KEY, 140, 184, 340, 24, true);
    let stt_model = edit(hwnd, ID_STT_MODEL, 140, 228, 340, 24, false);
    let base = edit(hwnd, ID_BASE, 140, 282, 340, 24, false);
    let key = edit(hwnd, ID_KEY, 140, 326, 340, 24, true);
    let model = edit(hwnd, ID_MODEL, 140, 370, 340, 24, false);
    button(hwnd, ID_TEST, ui.test_llm, 270, 424, 100, 30);
    button(hwnd, ID_SAVE, ui.save, 390, 424, 90, 30);
    if let Some(state) = STATE.get() {
        let mut lock = state.lock();
        lock.hotkey_combo = hotkey as isize;
        lock.hotkey_preview = hotkey_preview as isize;
        lock.hotkey_capture_button = hotkey_capture_button as isize;
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
        let mut lock = state.lock();
        lock.pending_hotkey = Some(config.hotkey.clone());
        unsafe {
            SendMessageW(
                lock.hotkey_combo as HWND,
                CB_SETCURSEL,
                config.hotkey.combo_index() as usize,
                0,
            );
            SetWindowTextW(
                lock.hotkey_preview as HWND,
                wide_null(crate::i18n::hotkey_label(&config.language, &config.hotkey)).as_ptr(),
            );
            SetWindowTextW(
                lock.stt_compat_combo as HWND,
                wide_null(crate::i18n::stt_compat_label(
                    &config.language,
                    &config.stt.compatibility,
                ))
                .as_ptr(),
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
    let hotkey = lock
        .pending_hotkey
        .clone()
        .unwrap_or_else(|| selected_hotkey(lock.hotkey_combo as HWND));
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

fn apply_hotkey_preset() {
    let Some(state) = STATE.get() else { return };
    let mut lock = state.lock();
    let index = unsafe { SendMessageW(lock.hotkey_combo as HWND, CB_GETCURSEL, 0, 0) };
    if index == 4 {
        return;
    }
    let hotkey = Hotkey::from_combo_index(index);
    lock.pending_hotkey = Some(hotkey.clone());
    update_hotkey_preview(&lock, &hotkey);
}

fn begin_hotkey_capture(hwnd: HWND) {
    let language = current_language();
    let ui = crate::i18n::UiText::for_language(&language);
    match crate::hotkey::begin_capture(hwnd) {
        Ok(()) => {
            if let Some(state) = STATE.get() {
                let lock = state.lock();
                unsafe {
                    SetWindowTextW(
                        lock.hotkey_preview as HWND,
                        wide_null(ui.press_hotkey).as_ptr(),
                    );
                }
            }
        }
        Err(err) => show_message("Settings", &format!("Failed: {err}")),
    }
}

fn finish_hotkey_capture(result: WPARAM) {
    let Some(state) = STATE.get() else { return };
    let mut lock = state.lock();
    if result == CAPTURE_OK {
        if let Some(hotkey) = crate::hotkey::take_captured_hotkey() {
            lock.pending_hotkey = Some(hotkey.clone());
            unsafe {
                SendMessageW(
                    lock.hotkey_combo as HWND,
                    CB_SETCURSEL,
                    hotkey.combo_index() as usize,
                    0,
                );
            }
            update_hotkey_preview(&lock, &hotkey);
        }
    } else if result == CAPTURE_CANCEL {
        if let Some(hotkey) = &lock.pending_hotkey {
            update_hotkey_preview(&lock, hotkey);
        }
    }
}

fn update_hotkey_preview(lock: &SettingsState, hotkey: &Hotkey) {
    let language = current_language();
    unsafe {
        SetWindowTextW(
            lock.hotkey_preview as HWND,
            wide_null(crate::i18n::hotkey_label(&language, hotkey)).as_ptr(),
        );
    }
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

unsafe fn static_text(parent: HWND, x: i32, y: i32, w: i32, h: i32, border: bool) -> HWND {
    CreateWindowExW(
        0,
        wide_null("STATIC").as_ptr(),
        wide_null("").as_ptr(),
        WS_CHILD | WS_VISIBLE | if border { WS_BORDER } else { 0 },
        x,
        y,
        w,
        h,
        parent,
        std::ptr::null_mut(),
        GetModuleHandleW(std::ptr::null()),
        std::ptr::null(),
    )
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
    let language = current_language();
    for item in [
        SttCompatibility::OpenAiAudioTranscriptions,
        SttCompatibility::AliyunQwenAsrChat,
    ] {
        let label = wide_null(crate::i18n::stt_compat_label(&language, &item));
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
    let language = current_language();
    for item in [
        Hotkey::RightCtrl,
        Hotkey::CapsLock,
        Hotkey::RightAlt,
        Hotkey::CtrlSpace,
        Hotkey::Custom {
            trigger_vk: 0x7c,
            trigger_scan_code: 0,
            extended: false,
            modifiers: Default::default(),
        },
    ] {
        let text = if item.is_custom_like() {
            match language {
                Language::ZhCn => "自定义...".to_string(),
                Language::ZhTw => "自訂...".to_string(),
                _ => "Custom...".to_string(),
            }
        } else {
            crate::i18n::hotkey_label(&language, &item)
        };
        let label = wide_null(text);
        SendMessageW(hwnd, CB_ADDSTRING, 0, label.as_ptr() as LPARAM);
    }
    hwnd
}

unsafe fn button(parent: HWND, id: isize, text: &str, x: i32, y: i32, w: i32, h: i32) -> HWND {
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
    )
}
