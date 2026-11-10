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

/// Where the island is while a full-screen app (a game, a video, a presentation) may be in front.
/// Two booleans used to stand for this, and nothing stopped both being true.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GameScreen {
    /// Nothing full-screen in front: the island is where the settings put it.
    #[default]
    Normal,
    /// Hidden until the full-screen app goes (one screen, or moving is switched off).
    Hidden,
    /// On another screen until the full-screen app goes.
    Moved,
}

/// The next state, from what Windows reports and what the settings allow. Pure, so the rules
/// can be read and tested without a window: `busy` already includes the "hide in full screen"
/// setting, and `other_screen` says a free screen exists and moving there is allowed.
pub fn next_game_screen(cur: GameScreen, busy: bool, other_screen: bool) -> GameScreen {
    match (cur, busy) {
        (_, false) => GameScreen::Normal,
        (GameScreen::Normal, true) if other_screen => GameScreen::Moved,
        (GameScreen::Normal, true) => GameScreen::Hidden,
        // Already out of the way: stay put until the app goes, even if a screen comes or goes.
        (held, true) => held,
    }
}

impl App {
    /// Matches the hook to the setting.
    pub fn sync_fullscreen_hook(&mut self) {
        set_enabled(self.st.settings.hide_in_fullscreen, self.platform.hwnd);
        if !self.st.settings.hide_in_fullscreen && self.game != GameScreen::Normal {
            self.set_game_screen(false);
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
        if want != (self.game != GameScreen::Normal) {
            self.set_game_screen(want);
        }
    }

    /// Applies `next_game_screen` and does what entering or leaving each state takes.
    pub fn set_game_screen(&mut self, busy: bool) {
        let game_mon = if busy && self.game == GameScreen::Normal { crate::platform::foreground_monitor() } else { 0 };
        let other = busy && self.st.settings.game_other_screen && crate::platform::other_monitor(game_mon).is_some();
        let next = next_game_screen(self.game, busy, other);
        if next == self.game {
            return;
        }
        let was = std::mem::replace(&mut self.game, next);
        // Leaving.
        match was {
            GameScreen::Moved => {
                crate::platform::set_game_monitor(0);
                crate::log::line("full-screen app gone: island back on its screen");
                self.on_display_change();
            }
            GameScreen::Hidden => {
                crate::log::line("full-screen app gone: island back");
                self.platform.set_visible(true);
                self.on_display_change();
                self.ensure_running();
            }
            GameScreen::Normal => {}
        }
        // Entering.
        match next {
            GameScreen::Moved => {
                crate::platform::set_game_monitor(game_mon);
                crate::log::line(format!("full-screen app in front: island moved to the other screen (game on {game_mon:#x})"));
                self.on_display_change();
            }
            GameScreen::Hidden => {
                crate::log::line("full-screen app in front: island hidden");
                self.overlay = None;
                self.platform.set_frame_timer(false);
                self.platform.set_poll_timer(false);
                self.running = false;
                self.platform.set_visible(false);
            }
            GameScreen::Normal => {}
        }
        crate::stocks::set_active(next != GameScreen::Hidden && !self.st.paused && self.st.mode != crate::layout::Mode::Hidden);
    }

    /// Nothing may pop up: paused by the user, or hidden for a game.
    pub fn quiet(&self) -> bool {
        self.st.paused || self.game == GameScreen::Hidden
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_screen_app_moves_or_hides_the_island_and_it_comes_back() {
        use GameScreen::*;
        assert_eq!(next_game_screen(Normal, true, true), Moved);
        assert_eq!(next_game_screen(Normal, true, false), Hidden);
        assert_eq!(next_game_screen(Moved, false, true), Normal);
        assert_eq!(next_game_screen(Hidden, false, false), Normal);
        // A screen plugged in or out mid-game does not shuffle the island around.
        assert_eq!(next_game_screen(Hidden, true, true), Hidden);
        assert_eq!(next_game_screen(Moved, true, false), Moved);
        assert_eq!(next_game_screen(Normal, false, true), Normal);
    }
}
