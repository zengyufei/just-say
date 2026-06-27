use crate::util::wide_null;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::GlobalFree;
use windows_sys::Win32::Foundation::{GetLastError, HWND};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, OpenClipboard,
    SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

#[derive(Default)]
pub struct ClipboardSnapshot {
    items: Vec<ClipboardItem>,
}

struct ClipboardItem {
    format: u32,
    bytes: Vec<u8>,
}

pub fn set_unicode_text_preserving(text: &str) -> anyhow::Result<ClipboardSnapshot> {
    let _guard = ClipboardGuard::open(null_mut())?;
    let snapshot = snapshot_clipboard();
    unsafe {
        if EmptyClipboard() == 0 {
            anyhow::bail!("EmptyClipboard failed: {}", GetLastError());
        }
    }
    set_unicode_text_open(text)?;
    Ok(snapshot)
}

pub fn restore(snapshot: ClipboardSnapshot) -> anyhow::Result<()> {
    let _guard = ClipboardGuard::open(null_mut())?;
    unsafe {
        if EmptyClipboard() == 0 {
            anyhow::bail!("EmptyClipboard restore failed: {}", GetLastError());
        }
    }
    for item in snapshot.items {
        if let Err(err) = set_raw_open(item.format, &item.bytes) {
            tracing::warn!(%err, format = item.format, "failed to restore clipboard format");
        }
    }
    Ok(())
}

fn snapshot_clipboard() -> ClipboardSnapshot {
    let mut items = Vec::new();
    let mut format = 0u32;
    loop {
        format = unsafe { EnumClipboardFormats(format) };
        if format == 0 {
            break;
        }
        let handle = unsafe { GetClipboardData(format) };
        if handle.is_null() {
            continue;
        }
        let size = unsafe { GlobalSize(handle) };
        if size == 0 {
            tracing::debug!(
                format,
                "clipboard format is not backed by movable memory; skipped"
            );
            continue;
        }
        let ptr = unsafe { GlobalLock(handle) };
        if ptr.is_null() {
            continue;
        }
        let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, size).to_vec() };
        unsafe {
            GlobalUnlock(handle);
        }
        items.push(ClipboardItem { format, bytes });
    }
    ClipboardSnapshot { items }
}

fn set_unicode_text_open(text: &str) -> anyhow::Result<()> {
    let mut wide = wide_null(text);
    let bytes = unsafe {
        std::slice::from_raw_parts_mut(wide.as_mut_ptr() as *mut u8, wide.len() * 2).to_vec()
    };
    set_raw_open(CF_UNICODETEXT as u32, &bytes)
}

fn set_raw_open(format: u32, bytes: &[u8]) -> anyhow::Result<()> {
    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if handle.is_null() {
            anyhow::bail!("GlobalAlloc failed for clipboard");
        }
        let ptr = GlobalLock(handle);
        if ptr.is_null() {
            GlobalFree(handle);
            anyhow::bail!("GlobalLock failed for clipboard");
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
        GlobalUnlock(handle);
        if SetClipboardData(format, handle).is_null() {
            GlobalFree(handle);
            anyhow::bail!("SetClipboardData failed: {}", GetLastError());
        }
    }
    Ok(())
}

struct ClipboardGuard;

impl ClipboardGuard {
    fn open(hwnd: HWND) -> anyhow::Result<Self> {
        for _ in 0..10 {
            unsafe {
                if OpenClipboard(hwnd) != 0 {
                    return Ok(Self);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        anyhow::bail!("Clipboard is busy")
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}
