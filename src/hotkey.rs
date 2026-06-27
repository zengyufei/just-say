use crate::config::Hotkey;
use parking_lot::Mutex;
use std::sync::{mpsc::Sender, OnceLock};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    VK_CAPITAL, VK_CONTROL, VK_LCONTROL, VK_MENU, VK_RCONTROL, VK_RMENU, VK_SPACE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

#[derive(Debug, Clone, Copy)]
pub enum HotkeyEvent {
    Pressed,
    Released,
}

static STATE: OnceLock<Mutex<HookState>> = OnceLock::new();

#[derive(Clone)]
struct HookState {
    sender: Sender<HotkeyEvent>,
    hotkey: Hotkey,
    active: bool,
    ctrl_down: bool,
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
            ctrl_down: false,
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
        state.ctrl_down = false;
    }
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
    let vk = kb.vkCode;
    if vk == VK_CONTROL as u32 || vk == VK_LCONTROL as u32 || vk == VK_RCONTROL as u32 {
        state.ctrl_down = is_down || (!is_up && state.ctrl_down);
    }

    let matched_down = matches_hotkey(&state.hotkey, kb, is_down, state.ctrl_down);
    let matched_up = matches_hotkey(&state.hotkey, kb, is_up, state.ctrl_down);

    if matched_down && !state.active {
        state.active = true;
        let _ = state.sender.send(HotkeyEvent::Pressed);
        return 1;
    }
    if matched_up && state.active {
        state.active = false;
        let _ = state.sender.send(HotkeyEvent::Released);
        return 1;
    }
    if state.active && should_suppress_while_active(&state.hotkey, kb) {
        return 1;
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

fn matches_hotkey(hotkey: &Hotkey, kb: &KBDLLHOOKSTRUCT, phase: bool, ctrl_down: bool) -> bool {
    if !phase {
        return false;
    }
    match hotkey {
        Hotkey::RightCtrl => kb.vkCode == VK_RCONTROL as u32,
        Hotkey::CapsLock => kb.vkCode == VK_CAPITAL as u32,
        Hotkey::RightAlt => kb.vkCode == VK_RMENU as u32,
        Hotkey::CtrlSpace => ctrl_down && kb.vkCode == VK_SPACE as u32,
        Hotkey::FnScanCode { scan_code } => kb.scanCode == *scan_code,
    }
}

fn should_suppress_while_active(hotkey: &Hotkey, kb: &KBDLLHOOKSTRUCT) -> bool {
    match hotkey {
        Hotkey::RightCtrl => kb.vkCode == VK_RCONTROL as u32,
        Hotkey::CapsLock => kb.vkCode == VK_CAPITAL as u32,
        Hotkey::RightAlt => kb.vkCode == VK_RMENU as u32 || kb.vkCode == VK_MENU as u32,
        Hotkey::CtrlSpace => kb.vkCode == VK_SPACE as u32 || kb.vkCode == VK_CONTROL as u32,
        Hotkey::FnScanCode { scan_code } => kb.scanCode == *scan_code,
    }
}
