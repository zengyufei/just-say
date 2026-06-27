#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod clipboard;
mod config;
mod dpapi;
mod hotkey;
mod i18n;
mod ime;
mod injector;
mod logger;
mod overlay;
mod refiner;
mod settings;
mod transcriber;
mod tray;
mod util;

use anyhow::Context;
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    unsafe {
        use windows_sys::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let _guard = logger::init().context("initialize logging")?;
    tracing::info!("JustSay starting");

    let config = config::ConfigStore::load_or_default().context("load config")?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("justsay-async")
        .build()
        .context("create tokio runtime")?;

    let controller = Arc::new(app::AppController::new(config, runtime));
    tray::run(controller)?;
    Ok(())
}
