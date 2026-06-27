use crate::util::wide_null;
use parking_lot::Mutex;
use rand::Rng;
use std::sync::{Arc, OnceLock};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreatePen, CreateRoundRectRgn, CreateSolidBrush, DeleteObject,
    DrawTextW, EndPaint, FillRgn, GetMonitorInfoW, GetStockObject, InvalidateRect,
    MonitorFromPoint, SelectObject, SetBkMode, SetTextColor, DEFAULT_GUI_FONT, DT_END_ELLIPSIS,
    DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK, HBRUSH, HDC, HMONITOR, HPEN,
    MONITORINFO, MONITOR_DEFAULTTONEAREST, PAINTSTRUCT, PS_SOLID, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, KillTimer, RegisterClassW,
    SetLayeredWindowAttributes, SetTimer, SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW,
    HWND_TOPMOST, LWA_ALPHA, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE, WM_NCDESTROY, WM_PAINT,
    WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};

const CLASS_NAME: &str = "VoiceTrayOverlayWindow";
const TIMER_ID: usize = 42;
const HEIGHT: i32 = 56;
const DEBUG_HEIGHT: i32 = 116;
const DEBUG_GAP: i32 = 10;
const DEBUG_BOX_GAP: i32 = 30;
const DEBUG_BOX_PAD: i32 = 12;
const DEBUG_MIN_WIDTH: i32 = 860;
const DEBUG_MAX_WIDTH: i32 = 1500;
const MIN_TEXT_WIDTH: f32 = 160.0;
const MAX_TEXT_WIDTH: f32 = 560.0;
const WAVE_AREA: i32 = 44;
const SIDE_PAD: i32 = 18;
const GAP: i32 = 12;
const BAR_WEIGHTS: [f32; 5] = [0.5, 0.8, 1.0, 0.75, 0.55];
const COLOR_KEY: u32 = 0x00ff00ff;

#[derive(Default)]
struct OverlayState {
    hwnd: isize,
    text: String,
    rms: f32,
    envelope: f32,
    width: f32,
    target_width: f32,
    visible: bool,
    stt_debug_text: String,
    llm_debug_text: String,
    debug_visible: bool,
    hide_pending: bool,
}

static STATE: OnceLock<Arc<Mutex<OverlayState>>> = OnceLock::new();

pub fn init() -> anyhow::Result<()> {
    let state = STATE
        .get_or_init(|| {
            Arc::new(Mutex::new(OverlayState {
                text: "Listening...".to_string(),
                width: 260.0,
                target_width: 260.0,
                ..Default::default()
            }))
        })
        .clone();

    let hwnd = create_window()?;
    state.lock().hwnd = hwnd as isize;
    Ok(())
}

pub fn show(text: &str) {
    set_text(text);
    if let Some(state) = STATE.get() {
        let hwnd = {
            let mut lock = state.lock();
            lock.hide_pending = false;
            lock.hwnd as HWND
        };
        if !hwnd.is_null() {
            position_window(hwnd);
            unsafe {
                SetTimer(hwnd, TIMER_ID, 16, None);
                ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                InvalidateRect(hwnd, std::ptr::null(), 1);
            }
            state.lock().visible = true;
        }
    }
}

pub fn hide() {
    if let Some(state) = STATE.get() {
        let hwnd = {
            let lock = state.lock();
            lock.hwnd as HWND
        };
        if hwnd.is_null() {
            return;
        }
        if is_cursor_inside_overlay() {
            let mut lock = state.lock();
            lock.hide_pending = true;
            unsafe {
                SetTimer(hwnd, TIMER_ID, 80, None);
            }
        } else {
            hide_now();
        }
    }
}

pub fn set_debug_texts(left: &str, right: &str) {
    if let Some(state) = STATE.get() {
        let mut lock = state.lock();
        lock.stt_debug_text = left.to_string();
        lock.llm_debug_text = right.to_string();
        lock.debug_visible = true;
        lock.hide_pending = false;
        let hwnd = lock.hwnd as HWND;
        drop(lock);
        if !hwnd.is_null() {
            position_window(hwnd);
            unsafe {
                InvalidateRect(hwnd, std::ptr::null(), 1);
            }
        }
    }
}

pub fn set_text(text: &str) {
    if let Some(state) = STATE.get() {
        let mut lock = state.lock();
        lock.text = text.to_string();
        let text_width = (text.chars().count() as f32 * 9.5).clamp(MIN_TEXT_WIDTH, MAX_TEXT_WIDTH);
        lock.target_width = (SIDE_PAD * 2 + WAVE_AREA + GAP) as f32 + text_width;
        if lock.width <= 0.0 {
            lock.width = lock.target_width;
        }
        let hwnd = lock.hwnd as HWND;
        drop(lock);
        if !hwnd.is_null() {
            position_window(hwnd);
            unsafe {
                InvalidateRect(hwnd, std::ptr::null(), 1);
            }
        }
    }
}

pub fn set_rms(rms: f32) {
    if let Some(state) = STATE.get() {
        let mut lock = state.lock();
        lock.rms = rms.clamp(0.0, 1.0);
        let hwnd = lock.hwnd as HWND;
        drop(lock);
        if !hwnd.is_null() {
            unsafe {
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
        }
    }
}

fn create_window() -> anyhow::Result<HWND> {
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
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
            class.as_ptr(),
            wide_null("").as_ptr(),
            WS_POPUP,
            0,
            0,
            280,
            total_height(false),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            anyhow::bail!("CreateWindowExW overlay failed");
        }
        SetLayeredWindowAttributes(
            hwnd,
            COLOR_KEY,
            255,
            LWA_ALPHA | windows_sys::Win32::UI::WindowsAndMessaging::LWA_COLORKEY,
        );
        Ok(hwnd)
    }
}

fn position_window(hwnd: HWND) {
    let mut pt = POINT { x: 0, y: 0 };
    unsafe {
        GetCursorPos(&mut pt);
    }
    let monitor: HMONITOR = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        rcMonitor: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        rcWork: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        dwFlags: 0,
    };
    unsafe {
        GetMonitorInfoW(monitor, &mut info);
    }
    let width = STATE
        .get()
        .map(|s| {
            let lock = s.lock();
            effective_width(&lock)
        })
        .unwrap_or(280);
    let total_height = STATE
        .get()
        .map(|s| total_height(s.lock().debug_visible))
        .unwrap_or(HEIGHT);
    let x = info.rcWork.left + ((info.rcWork.right - info.rcWork.left) - width) / 2;
    let capsule_y = info.rcWork.bottom - HEIGHT - 28;
    let y = (capsule_y - (total_height - HEIGHT)).max(info.rcWork.top);
    unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            x,
            y,
            width,
            total_height,
            SWP_NOACTIVATE,
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
        WM_TIMER => {
            tick(hwnd);
            0
        }
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_NCDESTROY => {
            if let Some(state) = STATE.get() {
                state.lock().hwnd = 0;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn tick(hwnd: HWND) {
    if let Some(state) = STATE.get() {
        let hide_pending = state.lock().hide_pending;
        if hide_pending {
            if is_cursor_inside_overlay() {
                return;
            }
            hide_now();
            return;
        }
        let mut lock = state.lock();
        let next_width = lock.width + (lock.target_width - lock.width) * 0.22;
        lock.width = next_width;
        let input = lock.rms;
        let coeff = if input > lock.envelope { 0.40 } else { 0.15 };
        lock.envelope += (input - lock.envelope) * coeff;
        drop(lock);
        position_window(hwnd);
        InvalidateRect(hwnd, std::ptr::null(), 0);
    }
}

unsafe fn paint(hwnd: HWND) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    let Some(state) = STATE.get() else {
        EndPaint(hwnd, &ps);
        return;
    };
    let lock = state.lock();
    let width = effective_width(&lock);
    let capsule_width = lock.width.round() as i32;
    let text = lock.text.clone();
    let level = lock.envelope.max(0.04);
    let debug_visible = lock.debug_visible;
    let stt_debug_text = lock.stt_debug_text.clone();
    let llm_debug_text = lock.llm_debug_text.clone();
    drop(lock);

    let key_brush = CreateSolidBrush(COLOR_KEY);
    let total_h = total_height(debug_visible);
    let clear_rgn = CreateRoundRectRgn(0, 0, width, total_h, 0, 0);
    FillRgn(hdc, clear_rgn, key_brush);
    DeleteObject(clear_rgn as _);
    DeleteObject(key_brush as _);

    let capsule_y = if debug_visible {
        DEBUG_HEIGHT + DEBUG_GAP
    } else {
        0
    };
    if debug_visible {
        draw_debug_boxes(hdc, width, &stt_debug_text, &llm_debug_text);
    }

    let capsule_x = (width - capsule_width) / 2;
    let bg = CreateSolidBrush(rgb(26, 28, 32));
    let rgn = CreateRoundRectRgn(
        capsule_x,
        capsule_y,
        capsule_x + capsule_width,
        capsule_y + HEIGHT,
        HEIGHT,
        HEIGHT,
    );
    FillRgn(hdc, rgn, bg);
    DeleteObject(rgn as _);
    DeleteObject(bg as _);

    draw_wave(
        hdc,
        capsule_x + SIDE_PAD,
        capsule_y + (HEIGHT - 32) / 2,
        level,
    );
    draw_text(
        hdc,
        capsule_x + SIDE_PAD + WAVE_AREA + GAP,
        capsule_x + capsule_width - SIDE_PAD,
        capsule_y,
        HEIGHT,
        &text,
    );

    EndPaint(hwnd, &ps);
}

unsafe fn draw_debug_boxes(hdc: HDC, width: i32, left_text: &str, right_text: &str) {
    let box_width = (width - DEBUG_BOX_GAP) / 2;
    draw_debug_box(hdc, 0, 0, box_width, DEBUG_HEIGHT, "识别内容", left_text);
    draw_debug_box(
        hdc,
        box_width + DEBUG_BOX_GAP,
        0,
        box_width,
        DEBUG_HEIGHT,
        if right_text == "LLM 正在优化..." {
            "LLM 正在优化"
        } else {
            "优化后内容"
        },
        right_text,
    );
}

unsafe fn draw_debug_box(
    hdc: HDC,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    title: &str,
    body: &str,
) {
    let bg = CreateSolidBrush(rgb(0, 0, 0));
    let rgn = CreateRoundRectRgn(x, y, x + width, y + height, 10, 10);
    FillRgn(hdc, rgn, bg);
    DeleteObject(rgn as _);
    DeleteObject(bg as _);

    SetBkMode(hdc, TRANSPARENT as i32);
    let title_font = create_font(-14, 700);
    let body_font = create_font(-15, 400);
    SetTextColor(hdc, rgb(190, 220, 255));
    let old_font = if !title_font.is_null() {
        SelectObject(hdc, title_font as _)
    } else {
        SelectObject(hdc, GetStockObject(DEFAULT_GUI_FONT))
    };
    let mut title_rect = RECT {
        left: x + DEBUG_BOX_PAD,
        top: y + 8,
        right: x + width - DEBUG_BOX_PAD,
        bottom: y + 28,
    };
    let title_wide = wide_null(title);
    DrawTextW(
        hdc,
        title_wide.as_ptr(),
        -1,
        &mut title_rect,
        DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
    );

    SetTextColor(hdc, rgb(245, 247, 250));
    if !body_font.is_null() {
        SelectObject(hdc, body_font as _);
    }
    let mut body_rect = RECT {
        left: x + DEBUG_BOX_PAD,
        top: y + 32,
        right: x + width - DEBUG_BOX_PAD,
        bottom: y + height - 10,
    };
    let body_wide = wide_null(truncate_debug_text(body));
    DrawTextW(
        hdc,
        body_wide.as_ptr(),
        -1,
        &mut body_rect,
        DT_LEFT | DT_WORDBREAK | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
    SelectObject(hdc, old_font);
    if !title_font.is_null() {
        DeleteObject(title_font as _);
    }
    if !body_font.is_null() {
        DeleteObject(body_font as _);
    }
}

unsafe fn draw_wave(hdc: HDC, x: i32, y: i32, level: f32) {
    let brush = CreateSolidBrush(rgb(108, 220, 255));
    let pen: HPEN = CreatePen(PS_SOLID, 1, rgb(108, 220, 255));
    let old_brush = SelectObject(hdc, brush as _);
    let old_pen = SelectObject(hdc, pen as _);
    let mut rng = rand::thread_rng();
    for (idx, weight) in BAR_WEIGHTS.iter().enumerate() {
        let jitter = rng.gen_range(-0.04_f32..0.04_f32);
        let h = (6.0 + 26.0 * (level * (weight + jitter)).clamp(0.0, 1.0)).round() as i32;
        let bar_x = x + idx as i32 * 8 + 4;
        let top = y + (32 - h) / 2;
        let rgn = CreateRoundRectRgn(bar_x, top, bar_x + 4, top + h, 4, 4);
        FillRgn(hdc, rgn, brush as HBRUSH);
        DeleteObject(rgn as _);
    }
    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
    DeleteObject(brush as _);
    DeleteObject(pen as _);
}

unsafe fn draw_text(hdc: HDC, left: i32, right: i32, top: i32, height: i32, text: &str) {
    SetBkMode(hdc, TRANSPARENT as i32);
    SetTextColor(hdc, rgb(245, 247, 250));
    let font = create_font(-16, 500);
    let old_font = if !font.is_null() {
        SelectObject(hdc, font as _)
    } else {
        SelectObject(hdc, GetStockObject(DEFAULT_GUI_FONT))
    };
    let mut rect = RECT {
        left,
        top,
        right,
        bottom: top + height,
    };
    let wide = wide_null(text);
    DrawTextW(
        hdc,
        wide.as_ptr(),
        -1,
        &mut rect,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE,
    );
    SelectObject(hdc, old_font);
    if !font.is_null() {
        DeleteObject(font as _);
    }
}

unsafe fn create_font(height: i32, weight: i32) -> windows_sys::Win32::Graphics::Gdi::HFONT {
    let font_name = wide_null("Segoe UI");
    CreateFontW(
        height,
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        font_name.as_ptr(),
    )
}

pub fn destroy() {
    if let Some(state) = STATE.get() {
        let hwnd = state.lock().hwnd as HWND;
        if !hwnd.is_null() {
            unsafe {
                DestroyWindow(hwnd);
            }
        }
    }
}

fn hide_now() {
    if let Some(state) = STATE.get() {
        let hwnd = {
            let mut lock = state.lock();
            lock.visible = false;
            lock.hide_pending = false;
            lock.stt_debug_text.clear();
            lock.llm_debug_text.clear();
            lock.debug_visible = false;
            lock.hwnd as HWND
        };
        if !hwnd.is_null() {
            unsafe {
                KillTimer(hwnd, TIMER_ID);
                ShowWindow(hwnd, SW_HIDE);
            }
        }
    }
}

fn is_cursor_inside_overlay() -> bool {
    let Some(state) = STATE.get() else {
        return false;
    };
    let (visible, width, height, hwnd) = {
        let lock = state.lock();
        (
            lock.visible,
            effective_width(&lock),
            total_height(lock.debug_visible),
            lock.hwnd as HWND,
        )
    };
    if !visible || hwnd.is_null() {
        return false;
    }
    let mut cursor = POINT { x: 0, y: 0 };
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe {
        if GetCursorPos(&mut cursor) == 0 {
            return false;
        }
        if windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect) == 0 {
            return false;
        }
    }
    let right = rect.left + width;
    let bottom = rect.top + height;
    cursor.x >= rect.left && cursor.x <= right && cursor.y >= rect.top && cursor.y <= bottom
}

fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}

fn total_height(debug_visible: bool) -> i32 {
    if debug_visible {
        DEBUG_HEIGHT + DEBUG_GAP + HEIGHT
    } else {
        HEIGHT
    }
}

fn effective_width(state: &OverlayState) -> i32 {
    let capsule_width = state.width.round() as i32;
    if state.debug_visible {
        capsule_width.clamp(DEBUG_MIN_WIDTH, DEBUG_MAX_WIDTH)
    } else {
        capsule_width
    }
}

fn truncate_debug_text(text: &str) -> String {
    const MAX_CHARS: usize = 180;
    let mut out = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= MAX_CHARS {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}
