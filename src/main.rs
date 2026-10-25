#![windows_subsystem = "windows"]

mod app;
mod config;
mod ext;
mod keys;
mod motion;
mod net;
mod search;
mod store;
mod tabs;
mod web;
mod window;

use std::mem::size_of;

use windows::Win32::{
    System::{Com::*, WinRT::*},
    UI::HiDpi::*,
};

fn main() -> windows::core::Result<()> {
    let url = std::env::args().nth(1);
    if window::forward(url.as_deref()) {
        return Ok(());
    }
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let _queue = unsafe {
        CreateDispatcherQueueController(DispatcherQueueOptions {
            dwSize: size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_NONE,
        })?
    };

    let config = config::Config::load();
    let hwnd = window::create(&config)?;
    app::launch(hwnd, config, url)?;
    window::run();

    if app::restarting() {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe).spawn();
        }
    }
    Ok(())
}
