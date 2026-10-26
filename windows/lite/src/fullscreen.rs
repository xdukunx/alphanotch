// Hides the island while a game, a full-screen video or a presentation is in front.
//
// Windows keeps one answer for "is something full-screen using the display?":
// `SHQueryUserNotificationState` (it is what makes Teams and Discord stay quiet during a game).
// We ask it only when the foreground window changes, plus once every half minute as a
// fallback, so it costs nothing in between. While it says "busy" the island's window is
// hidden, its timers stop, and nothing pops up (reminders and prayer cards wait their turn).
//
// A foreground hook is the only way to hear about a window change without polling. It is
// installed only while the setting is on and holds no information about the windows: it posts
// a message to the island and returns.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::app::App;

/// Posted to the island's window when the foreground window changes.
pub const WM_FG: u32 = 0x8000 + 22;

const EVENT_SYSTEM_FOREGROUND: u32 = 3;
const WINEVENT_OUTOFCONTEXT: u32 = 0;

static HOOK: AtomicIsize = AtomicIsize::new(0);
static TARGET: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn on_event(_hook: HWINEVENTHOOK, _event: u32, _hwnd: HWND, _object: i32, _child: i32, _thread: u32, _time: u32) {
    let h = TARGET.load(Ordering::Relaxed);
    if h != 0 {
        let _ = PostMessageW(Some(HWND(h as *mut _)), WM_FG, WPARAM(0), LPARAM(0));
    }
}

/// Installs or removes the foreground hook.
pub fn set_enabled(on: bool, hwnd: HWND) {
    unsafe {
        let cur = HOOK.load(Ordering::Relaxed);
        if on && cur == 0 && !hwnd.0.is_null() {
            TARGET.store(hwnd.0 as isize, Ordering::Relaxed);
            let h = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_event), 0, 0, WINEVENT_OUTOFCONTEXT);
            if !h.0.is_null() {
                HOOK.store(h.0 as isize, Ordering::Relaxed);
            }
        } else if !on && cur != 0 {
            let _ = UnhookWinEvent(HWINEVENTHOOK(cur as *mut _));
            HOOK.store(0, Ordering::Relaxed);
        }
    }
}

/// A full-screen app (D3D game, borderless game, full-screen video) or a presentation is in front.
pub fn busy() -> bool {
    match unsafe { SHQueryUserNotificationState() } {
        Ok(s) => s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE,
        Err(_) => false,
    }
}

impl App {
    /// Matches the hook to the setting.
    pub fn sync_fullscreen_hook(&mut self) {
        set_enabled(self.st.settings.hide_in_fullscreen, self.platform.hwnd);
        if !self.st.settings.hide_in_fullscreen && self.game_hidden {
            self.set_game_hidden(false);
        }
    }

    /// The foreground window changed. A game takes a moment to go full-screen, so look twice.
    pub fn on_foreground_change(&mut self) {
        if !self.st.settings.hide_in_fullscreen {
            return;
        }
        // Now, then a second later and again a few seconds on: a window goes full-screen (and a
        // game quits) after the focus has already moved. These are Win32 timers, so they run
        // while the island is hidden.
        self.fullscreen_check();
        self.fs_rechecks = 1;
        self.platform.arm_fs_timer(1000);
    }

    pub fn fullscreen_check(&mut self) {
        let want = self.st.settings.hide_in_fullscreen && busy();
        if want != self.game_hidden {
            self.set_game_hidden(want);
        }
    }

    pub fn set_game_hidden(&mut self, on: bool) {
        if on == self.game_hidden {
            return;
        }
        self.game_hidden = on;
        crate::stocks::set_active(!on && !self.st.paused && self.st.mode != crate::layout::Mode::Hidden);
        crate::log::line(if on { "full-screen app in front: island hidden" } else { "full-screen app gone: island back" });
        if on {
            self.overlay = None;
            self.platform.set_frame_timer(false);
            self.platform.set_poll_timer(false);
            self.running = false;
            self.platform.set_visible(false);
        } else {
            self.platform.set_visible(true);
            self.on_display_change();
            self.ensure_running();
        }
    }

    /// Nothing may pop up: paused by the user, or hidden for a game.
    pub fn quiet(&self) -> bool {
        self.st.paused || self.game_hidden
    }
}
