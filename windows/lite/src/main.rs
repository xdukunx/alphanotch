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
mod buddy;
mod cards;
mod claude;
mod coach;
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
mod keys;
mod layout;
mod lyrics;
mod log;
mod mochi;
mod net;
mod nusapages;
mod ole;
mod overlay;
mod pipe;
mod prayer;
mod reminders;
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
mod pages;
mod stocks;
mod ui;
mod util;
mod weather;
mod views;
mod win_user;

use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, LPARAM, WPARAM};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, FindWindowW, GetMessageW, PostMessageW, TranslateMessage, MSG,
};

/// After a panic the process is about to abort: ask a tiny detached shell to start the island
/// again in a couple of seconds (the single-instance lock goes with the dying process). Three
/// crashes within two minutes stop the loop, so a bug on start-up cannot spin forever.
fn relaunch_after_crash() {
    use std::os::windows::process::CommandExt;
    // The test hook below must not carry over to the island that is started again.
    std::env::remove_var("COUCOU_TEST_PANIC");
    let Ok(exe) = std::env::current_exe() else { return };
    let file = settings::local_dir().join("restarts.txt");
    let now = util::unix_now();
    let mut recent: Vec<f64> = std::fs::read_to_string(&file)
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|t| t.parse::<f64>().ok())
        .filter(|t| now - t < 120.0)
        .collect();
    if recent.len() >= 3 {
        log::line("not restarting: three crashes in two minutes");
        return;
    }
    recent.push(now);
    let _ = std::fs::write(&file, recent.iter().map(|t| format!("{t:.0}")).collect::<Vec<_>>().join(" "));
    // The new copy waits for this one to be gone (`--after <pid>`), so the single-instance lock
    // is free when it starts. No shell and no quoting involved.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new(&exe)
        .arg("--after")
        .arg(std::process::id().to_string())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

/// `--after <pid>`: wait (up to 15 s) for that process to exit before going on.
fn wait_for_previous_instance() {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--after" {
            if let Some(pid) = args.next().and_then(|p| p.parse::<u32>().ok()) {
                unsafe {
                    if let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
                        WaitForSingleObject(h, 15_000);
                        let _ = CloseHandle(h);
                    }
                }
            }
        }
    }
}

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
        relaunch_after_crash();
    }));

    wait_for_previous_instance();
    platform::init_dpi();

    // Test hook: `COUCOU_TEST_PANIC=1` crashes five seconds after the start, to check that the
    // island comes back by itself.
    if std::env::var_os("COUCOU_TEST_PANIC").is_some() {
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(5));
            panic!("test panic requested by COUCOU_TEST_PANIC");
        });
    }

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
    weather::start(&loaded.weather_city);
    stocks::start(&loaded.stocks);
    let handle = ctx::AppHandle::new(loaded);

    log::line(format!("--- Coucou Lite {} started ---", env!("CARGO_PKG_VERSION")));
    hooks::ensure_hook_exe(&handle);

    let island = app::App::new(handle.clone());
    handle.set_hwnd(island.platform.hwnd);
    let hwnd = island.platform.hwnd;
    app::install(island);
    app::with_app(|a| {
        a.sync_reminder_timer();
        a.apply_crown();
        ui::set_nusa(a.st.settings.theme_nusa);
        a.init_system_watch();
    });
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
