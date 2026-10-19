// Volume keys and Caps Lock, for the overlays on the idle bar.
//
// A low-level keyboard hook is the only way Windows lets an app see the media keys. It is
// installed only while the feature is on, looks at four key codes and nothing else, never
// stores or sends a key, and does no work itself: it posts a message to the island's window
// and returns. Switching the feature off removes the hook.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_SYSKEYDOWN,
};

use crate::anim::now;
use crate::app::{App, Timer};
use crate::layout::Mode;
use crate::overlay::{Overlay, OverlayKind};

/// Posted to the island's window by the hook: wParam = the key's virtual-key code.
pub const WM_KEYS: u32 = 0x8000 + 20;

pub const VK_VOLUME_MUTE: u32 = 0xAD;
pub const VK_VOLUME_DOWN: u32 = 0xAE;
pub const VK_VOLUME_UP: u32 = 0xAF;
pub const VK_CAPITAL: u32 = 0x14;

static HOOK: AtomicIsize = AtomicIsize::new(0);
static TARGET: AtomicIsize = AtomicIsize::new(0);

fn watched(vk: u32) -> bool {
    matches!(vk, VK_VOLUME_MUTE | VK_VOLUME_DOWN | VK_VOLUME_UP | VK_CAPITAL)
}

unsafe extern "system" fn hook_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && (wp.0 as u32 == WM_KEYDOWN || wp.0 as u32 == WM_SYSKEYDOWN) {
        let k = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        if watched(k.vkCode) {
            let h = TARGET.load(Ordering::Relaxed);
            if h != 0 {
                let _ = PostMessageW(Some(HWND(h as *mut _)), WM_KEYS, WPARAM(k.vkCode as usize), LPARAM(0));
            }
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

/// Installs or removes the hook.
pub fn set_enabled(on: bool, hwnd: HWND) {
    unsafe {
        let cur = HOOK.load(Ordering::Relaxed);
        if on && cur == 0 && !hwnd.0.is_null() {
            TARGET.store(hwnd.0 as isize, Ordering::Relaxed);
            if let Ok(h) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) {
                HOOK.store(h.0 as isize, Ordering::Relaxed);
            }
        } else if !on && cur != 0 {
            let _ = UnhookWindowsHookEx(HHOOK(cur as *mut _));
            HOOK.store(0, Ordering::Relaxed);
        }
    }
}

/// (level 0…1, muted) of the default output.
pub fn volume() -> Option<(f32, bool)> {
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let vol: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None).ok()?;
        let level = vol.GetMasterVolumeLevelScalar().ok()?;
        let mute = vol.GetMute().ok()?.as_bool();
        Some((level.clamp(0.0, 1.0), mute))
    }
}

pub fn caps_on() -> bool {
    unsafe { GetKeyState(VK_CAPITAL as i32) & 1 != 0 }
}

impl App {
    /// Turns the hook on or off to match the setting.
    pub fn sync_key_hook(&mut self) {
        let on = self.st.settings.feature_on("keys");
        set_enabled(on, self.platform.hwnd);
    }

    /// A watched key went down. The system has not applied it yet, so look a moment later.
    pub fn on_special_key(&mut self, vk: u32) {
        if !self.st.settings.feature_on("keys") || self.st.mode == Mode::Expanded {
            return;
        }
        self.later(0.07, Timer::KeyCheck(vk));
    }

    pub fn key_check(&mut self, vk: u32) {
        match vk {
            VK_CAPITAL => {
                let on = caps_on();
                self.show_overlay(Overlay {
                    kind: OverlayKind::Caps,
                    title: "Caps Lock".into(),
                    sub: if on { "Aktif".into() } else { "Mati".into() },
                    metric: String::new(),
                });
                self.overlay_level = None;
            }
            VK_VOLUME_MUTE | VK_VOLUME_DOWN | VK_VOLUME_UP => {
                let Some((level, muted)) = volume() else { return };
                self.show_overlay(Overlay {
                    kind: OverlayKind::Volume,
                    title: if muted { "Senyap".into() } else { "Volume".into() },
                    sub: String::new(),
                    metric: format!("{}%", (level * 100.0).round() as i32),
                });
                self.overlay_level = Some(if muted { 0.0 } else { level });
            }
            _ => {}
        }
        let _ = now();
    }
}
