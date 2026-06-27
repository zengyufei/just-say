use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
};

pub fn paste_text(text: &str) -> anyhow::Result<()> {
    if text.trim().is_empty() {
        return Ok(());
    }

    let snapshot = crate::clipboard::set_unicode_text_preserving(text)?;
    let ime_guard = crate::ime::ImeGuard::switch_to_us_best_effort();
    std::thread::sleep(std::time::Duration::from_millis(40));
    send_ctrl_v()?;
    std::thread::sleep(std::time::Duration::from_millis(180));
    drop(ime_guard);
    if let Err(err) = crate::clipboard::restore(snapshot) {
        tracing::warn!(%err, "failed to restore clipboard");
    }
    Ok(())
}

fn send_ctrl_v() -> anyhow::Result<()> {
    let mut inputs = [
        key_input(VK_CONTROL, 0),
        key_input(VK_V, 0),
        key_input(VK_V, KEYEVENTF_KEYUP),
        key_input(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_mut_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if sent != inputs.len() as u32 {
        anyhow::bail!("SendInput Ctrl+V failed");
    }
    Ok(())
}

fn key_input(vk: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}
