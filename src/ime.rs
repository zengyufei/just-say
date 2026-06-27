use crate::util::wide_null;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::Ime::{
    ImmGetContext, ImmGetOpenStatus, ImmReleaseContext, ImmSetOpenStatus,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    ActivateKeyboardLayout, GetKeyboardLayout, LoadKeyboardLayoutW, HKL, KLF_ACTIVATE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

pub struct ImeGuard {
    hwnd: HWND,
    original_layout: HKL,
    original_ime_open: Option<bool>,
}

impl ImeGuard {
    pub fn switch_to_us_best_effort() -> Self {
        let hwnd = unsafe { GetForegroundWindow() };
        let thread_id = unsafe { GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) };
        let original_layout = unsafe { GetKeyboardLayout(thread_id) };
        let original_ime_open = ime_open(hwnd);

        let us = wide_null("00000409");
        unsafe {
            let hkl = LoadKeyboardLayoutW(us.as_ptr(), KLF_ACTIVATE);
            if !hkl.is_null() {
                let _ = ActivateKeyboardLayout(hkl, 0);
            } else {
                tracing::warn!("failed to load US keyboard layout before paste");
            }
        }
        if original_ime_open == Some(true) {
            set_ime_open(hwnd, false);
        }

        Self {
            hwnd,
            original_layout,
            original_ime_open,
        }
    }
}

impl Drop for ImeGuard {
    fn drop(&mut self) {
        if !self.original_layout.is_null() {
            unsafe {
                let _ = ActivateKeyboardLayout(self.original_layout, 0);
            }
        }
        if let Some(open) = self.original_ime_open {
            set_ime_open(self.hwnd, open);
        }
    }
}

fn ime_open(hwnd: HWND) -> Option<bool> {
    unsafe {
        let ctx = ImmGetContext(hwnd);
        if ctx.is_null() {
            return None;
        }
        let open = ImmGetOpenStatus(ctx) != 0;
        ImmReleaseContext(hwnd, ctx);
        Some(open)
    }
}

fn set_ime_open(hwnd: HWND, open: bool) {
    unsafe {
        let ctx = ImmGetContext(hwnd);
        if ctx.is_null() {
            tracing::warn!("ImmGetContext failed; cannot change IME state");
            return;
        }
        if ImmSetOpenStatus(ctx, i32::from(open)) == 0 {
            tracing::warn!("ImmSetOpenStatus failed");
        }
        ImmReleaseContext(hwnd, ctx);
    }
}
