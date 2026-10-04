// Coucou Lite — Mochi without a webview.
//
// One small Win32 window drawn with tiny-skia, a tray icon, a named-pipe relay
// for Claude Code's hooks and a few pollers. Nothing runs while the island is
// hidden: no timers, no frame loop, no cursor poll.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod activity;
mod agenthooks;
mod anim;
mod app;
mod cards;
mod claude;
mod ctx;
mod dashboard;
mod files;
mod fsm;
mod gtasks;
mod gfx;
mod handlers;
mod hooks;
mod icons;
mod integrations;
mod layout;
mod log;
mod mochi;
mod ole;
mod pipe;
mod platform;
mod rt;
mod secrets;
mod settings;
mod settings_win;
#[cfg(feature = "snapshot")]
mod snapshot;
mod sound;
mod state;
mod text;
mod todos;
mod textfield;
mod ticker;
mod tray;
mod ui;
mod util;
mod views;
mod win_user;

use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, LPARAM, WPARAM};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, FindWindowW, GetMessageW, PostMessageW, TranslateMessage, MSG,
};

fn main() {
    #[cfg(feature = "snapshot")]
    {
        let args: Vec<String> = std::env::args().collect();
        if let Some(i) = args.iter().position(|a| a == "--snapshot") {
            snapshot::run(args.get(i + 1).map(String::as_str).unwrap_or("snap"));
            return;
        }
    }

    // `--agent-hooks <agent> <action>`: install or inspect hooks from the command line, no window.
    {
        let args: Vec<String> = std::env::args().collect();
        if let Some(i) = args.iter().position(|a| a == "--agent-hooks") {
            std::process::exit(agenthooks::run(&args[i + 1..]));
        }
    }

    // A panic aborts (release profile), so say why in the log before it does.
    std::panic::set_hook(Box::new(|info| {
        log::line(format!("PANIC: {info}"));
    }));

    platform::init_dpi();

    // One island per user: a second launch just asks the first to open.
    unsafe {
        let _mutex = CreateMutexW(None, true, w!("Local\\CoucouLite"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(hwnd) = FindWindowW(w!("CoucouIsland"), None) {
                let _ = PostMessageW(Some(hwnd), platform::WM_OPEN, WPARAM(0), LPARAM(0));
            }
            return;
        }
        // The mutex must live as long as the process.
        std::mem::forget(_mutex);
    }

    rt::init();
    activity::start();
    gtasks::start();
    let loaded = settings::load();
    let handle = ctx::AppHandle::new(loaded);

    log::line(format!("--- Coucou Lite {} started ---", env!("CARGO_PKG_VERSION")));
    hooks::ensure_hook_exe(&handle);

    let island = app::App::new(handle.clone());
    handle.set_hwnd(island.platform.hwnd);
    let hwnd = island.platform.hwnd;
    app::install(island);
    ole::register(hwnd);

    pipe::start(handle.clone());
    integrations::start(handle.clone());
    app::with_app(|a| a.launch());
    if std::env::args().any(|a| a == "--settings") {
        log::line("opening settings (flag)");
        settings_win::show();
    }

    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    // Dropping the app removes the tray icon.
    app::uninstall();
}
