use crate::config::{Hotkey, HotkeyModifiers};
use parking_lot::Mutex;
use std::sync::{mpsc::Sender, OnceLock};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    VK_CAPITAL, VK_CONTROL, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU,
    VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT, VK_SPACE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT,
    LLKHF_EXTENDED, LLKHF_INJECTED, WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN,
    WM_SYSKEYUP,
};

#[derive(Debug, Clone, Copy)]
pub enum HotkeyEvent {
    Pressed,
    Released,
}

pub const WM_HOTKEY_CAPTURED: u32 = WM_APP + 50;
pub const CAPTURE_OK: WPARAM = 1;
pub const CAPTURE_CANCEL: WPARAM = 2;

static STATE: OnceLock<Mutex<HookState>> = OnceLock::new();
static CAPTURED_HOTKEY: OnceLock<Mutex<Option<Hotkey>>> = OnceLock::new();

#[derive(Clone)]
struct HookState {
    sender: Sender<HotkeyEvent>,
    hotkey: Hotkey,
    active: bool,
    keys: KeyState,
    capture: Option<CaptureState>,
}

#[derive(Clone, Copy, Default)]
struct KeyState {
    ctrl: bool,
    alt: bool,
    shift: bool,
    win: bool,
}

#[derive(Clone, Copy, Default)]
struct CaptureState {
    hwnd: isize,
    modifiers: HotkeyModifiers,
    modifier_candidate: Option<KeySpec>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct KeySpec {
    vk: u32,
    scan_code: u32,
    extended: bool,
}

pub struct HotkeyHook {
    hook: HHOOK,
}

impl HotkeyHook {
    pub fn install(sender: Sender<HotkeyEvent>, hotkey: Hotkey) -> anyhow::Result<Self> {
        if matches!(hotkey, Hotkey::FnScanCode { .. }) {
            tracing::warn!(
                "Fn scan-code hotkey requested; Fn is usually handled by keyboard firmware and may not be observable"
            );
        }
        let state = HookState {
            sender,
            hotkey,
            active: false,
            keys: KeyState::default(),
            capture: None,
        };
        let _ = STATE.set(Mutex::new(state.clone()));
        if let Some(existing) = STATE.get() {
            *existing.lock() = state;
        }
        let module = unsafe { GetModuleHandleW(std::ptr::null()) };
        let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module, 0) };
        if hook.is_null() {
            anyhow::bail!("SetWindowsHookExW(WH_KEYBOARD_LL) failed");
        }
        Ok(Self { hook })
    }
}

pub fn update_hotkey(hotkey: Hotkey) {
    if let Some(state) = STATE.get() {
        let mut state = state.lock();
        state.hotkey = hotkey;
        state.active = false;
        state.keys = KeyState::default();
    }
}

pub fn begin_capture(hwnd: HWND) -> anyhow::Result<()> {
    let Some(state) = STATE.get() else {
        anyhow::bail!("keyboard hook is not installed");
    };
    *CAPTURED_HOTKEY.get_or_init(|| Mutex::new(None)).lock() = None;
    let mut state = state.lock();
    state.capture = Some(CaptureState {
        hwnd: hwnd as isize,
        modifiers: HotkeyModifiers::default(),
        modifier_candidate: None,
    });
    state.active = false;
    Ok(())
}

pub fn take_captured_hotkey() -> Option<Hotkey> {
    CAPTURED_HOTKEY.get().and_then(|value| value.lock().take())
}

impl Drop for HotkeyHook {
    fn drop(&mut self) {
        if !self.hook.is_null() {
            unsafe {
                UnhookWindowsHookEx(self.hook);
            }
        }
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
    }
    let kb = &*(lparam as *const KBDLLHOOKSTRUCT);
    let event = wparam as u32;
    let is_down = event == WM_KEYDOWN || event == WM_SYSKEYDOWN;
    let is_up = event == WM_KEYUP || event == WM_SYSKEYUP;

    let Some(state_lock) = STATE.get() else {
        return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
    };
    let mut state = state_lock.lock();

    if state.capture.is_some() && handle_capture_event(&mut state, kb, is_down, is_up) {
        return 1;
    }

    let key_vk = effective_vk(kb);
    let injected = (kb.flags & LLKHF_INJECTED) != 0;
    update_key_state(&mut state.keys, key_vk, is_down, is_up);

    if injected && !state.active {
        return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
    }

    let matched_down = matches_hotkey(&state.hotkey, kb, key_vk, is_down, state.keys);
    let matched_up = matches_hotkey(&state.hotkey, kb, key_vk, is_up, state.keys);
    let no_longer_matches = state.active
        && is_hotkey_relevant_event(&state.hotkey, kb, key_vk)
        && !hotkey_conditions_met(&state.hotkey, state.keys, key_vk);

    if matched_down && !state.active {
        state.active = true;
        let _ = state.sender.send(HotkeyEvent::Pressed);
        return 1;
    }
    if (matched_up || no_longer_matches) && state.active {
        state.active = false;
        let _ = state.sender.send(HotkeyEvent::Released);
        return 1;
    }
    if state.active && should_suppress_while_active(&state.hotkey, kb, key_vk) {
        return 1;
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

fn handle_capture_event(
    state: &mut HookState,
    kb: &KBDLLHOOKSTRUCT,
    is_down: bool,
    is_up: bool,
) -> bool {
    if !is_down && !is_up {
        return true;
    }
    let Some(mut capture) = state.capture else {
        return false;
    };
    let key = KeySpec {
        vk: effective_vk(kb),
        scan_code: kb.scanCode,
        extended: (kb.flags & LLKHF_EXTENDED) != 0,
    };

    if is_down && key.vk == VK_ESCAPE as u32 {
        finish_capture(state, capture.hwnd, None, CAPTURE_CANCEL);
        return true;
    }

    if is_modifier_vk(key.vk) {
        update_capture_modifiers(&mut capture.modifiers, key.vk, is_down, is_up);
        if is_down {
            capture.modifier_candidate = Some(key);
            state.capture = Some(capture);
            return true;
        }
        if is_up && capture.modifier_candidate == Some(key) {
            let hotkey = Hotkey::from_key_event(
                key.vk,
                key.scan_code,
                key.extended,
                HotkeyModifiers::default(),
            );
            finish_capture(state, capture.hwnd, Some(hotkey), CAPTURE_OK);
            return true;
        }
        state.capture = Some(capture);
        return true;
    }

    if is_down {
        let hotkey = Hotkey::from_key_event(key.vk, key.scan_code, key.extended, capture.modifiers);
        finish_capture(state, capture.hwnd, Some(hotkey), CAPTURE_OK);
    }
    true
}

fn finish_capture(state: &mut HookState, hwnd: isize, hotkey: Option<Hotkey>, result: WPARAM) {
    if let Some(hotkey) = hotkey {
        *CAPTURED_HOTKEY.get_or_init(|| Mutex::new(None)).lock() = Some(hotkey);
    }
    state.capture = None;
    unsafe {
        PostMessageW(hwnd as HWND, WM_HOTKEY_CAPTURED, result, 0);
    }
}

fn matches_hotkey(
    hotkey: &Hotkey,
    kb: &KBDLLHOOKSTRUCT,
    key_vk: u32,
    phase: bool,
    keys: KeyState,
) -> bool {
    if !phase {
        return false;
    }
    match hotkey {
        Hotkey::RightCtrl => key_vk == VK_RCONTROL as u32,
        Hotkey::CapsLock => key_vk == VK_CAPITAL as u32,
        Hotkey::RightAlt => key_vk == VK_RMENU as u32,
        Hotkey::CtrlSpace => keys.ctrl && key_vk == VK_SPACE as u32,
        Hotkey::FnScanCode { scan_code } => kb.scanCode == *scan_code,
        Hotkey::Custom {
            trigger_vk,
            modifiers,
            ..
        } => modifiers_match(*modifiers, keys) && key_vk == *trigger_vk,
    }
}

fn hotkey_conditions_met(hotkey: &Hotkey, keys: KeyState, key_vk: u32) -> bool {
    match hotkey {
        Hotkey::CtrlSpace => keys.ctrl && key_vk == VK_SPACE as u32,
        Hotkey::Custom {
            trigger_vk,
            modifiers,
            ..
        } => modifiers_match(*modifiers, keys) && key_vk == *trigger_vk,
        _ => true,
    }
}

fn is_hotkey_relevant_event(hotkey: &Hotkey, kb: &KBDLLHOOKSTRUCT, key_vk: u32) -> bool {
    match hotkey {
        Hotkey::RightCtrl => key_vk == VK_RCONTROL as u32,
        Hotkey::CapsLock => key_vk == VK_CAPITAL as u32,
        Hotkey::RightAlt => key_vk == VK_RMENU as u32,
        Hotkey::CtrlSpace => is_ctrl_vk(key_vk) || key_vk == VK_SPACE as u32,
        Hotkey::FnScanCode { scan_code } => kb.scanCode == *scan_code,
        Hotkey::Custom {
            trigger_vk,
            modifiers,
            ..
        } => key_vk == *trigger_vk || modifier_belongs_to_hotkey(key_vk, *modifiers),
    }
}

fn should_suppress_while_active(hotkey: &Hotkey, kb: &KBDLLHOOKSTRUCT, key_vk: u32) -> bool {
    match hotkey {
        Hotkey::RightCtrl => key_vk == VK_RCONTROL as u32,
        Hotkey::CapsLock => key_vk == VK_CAPITAL as u32,
        Hotkey::RightAlt => key_vk == VK_RMENU as u32,
        Hotkey::CtrlSpace => is_ctrl_vk(key_vk) || key_vk == VK_SPACE as u32,
        Hotkey::FnScanCode { scan_code } => kb.scanCode == *scan_code,
        Hotkey::Custom {
            trigger_vk,
            modifiers,
            ..
        } => key_vk == *trigger_vk || modifier_belongs_to_hotkey(key_vk, *modifiers),
    }
}

fn effective_vk(kb: &KBDLLHOOKSTRUCT) -> u32 {
    let extended = (kb.flags & LLKHF_EXTENDED) != 0;
    match kb.vkCode {
        vk if vk == VK_CONTROL as u32 || vk == VK_LCONTROL as u32 || vk == VK_RCONTROL as u32 => {
            if extended {
                VK_RCONTROL as u32
            } else {
                VK_LCONTROL as u32
            }
        }
        vk if vk == VK_MENU as u32 || vk == VK_LMENU as u32 || vk == VK_RMENU as u32 => {
            if extended {
                VK_RMENU as u32
            } else {
                VK_LMENU as u32
            }
        }
        vk => vk,
    }
}

fn update_key_state(keys: &mut KeyState, vk: u32, is_down: bool, is_up: bool) {
    let value = if is_down {
        Some(true)
    } else if is_up {
        Some(false)
    } else {
        None
    };
    let Some(value) = value else { return };
    if is_ctrl_vk(vk) {
        keys.ctrl = value;
    } else if is_alt_vk(vk) {
        keys.alt = value;
    } else if is_shift_vk(vk) {
        keys.shift = value;
    } else if is_win_vk(vk) {
        keys.win = value;
    }
}

fn update_capture_modifiers(modifiers: &mut HotkeyModifiers, vk: u32, is_down: bool, is_up: bool) {
    let value = if is_down {
        Some(true)
    } else if is_up {
        Some(false)
    } else {
        None
    };
    let Some(value) = value else { return };
    if is_ctrl_vk(vk) {
        modifiers.ctrl = value;
    } else if is_alt_vk(vk) {
        modifiers.alt = value;
    } else if is_shift_vk(vk) {
        modifiers.shift = value;
    } else if is_win_vk(vk) {
        modifiers.win = value;
    }
}

fn modifiers_match(required: HotkeyModifiers, keys: KeyState) -> bool {
    (!required.ctrl || keys.ctrl)
        && (!required.alt || keys.alt)
        && (!required.shift || keys.shift)
        && (!required.win || keys.win)
}

fn modifier_belongs_to_hotkey(vk: u32, modifiers: HotkeyModifiers) -> bool {
    (modifiers.ctrl && is_ctrl_vk(vk))
        || (modifiers.alt && is_alt_vk(vk))
        || (modifiers.shift && is_shift_vk(vk))
        || (modifiers.win && is_win_vk(vk))
}

fn is_modifier_vk(vk: u32) -> bool {
    is_ctrl_vk(vk) || is_alt_vk(vk) || is_shift_vk(vk) || is_win_vk(vk)
}

fn is_ctrl_vk(vk: u32) -> bool {
    vk == VK_CONTROL as u32 || vk == VK_LCONTROL as u32 || vk == VK_RCONTROL as u32
}

fn is_alt_vk(vk: u32) -> bool {
    vk == VK_MENU as u32 || vk == VK_LMENU as u32 || vk == VK_RMENU as u32
}

fn is_shift_vk(vk: u32) -> bool {
    vk == VK_SHIFT as u32 || vk == VK_LSHIFT as u32 || vk == VK_RSHIFT as u32
}

fn is_win_vk(vk: u32) -> bool {
    vk == VK_LWIN as u32 || vk == VK_RWIN as u32
}
