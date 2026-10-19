// Short overlays on the idle bar, the way Nusa does it: the bar widens for three seconds to say
// that the charger was plugged in, the battery is low or a headset was connected, then folds
// back. Nothing polls: Windows tells us (WM_POWERBROADCAST, WM_DEVICECHANGE) and we look
// only then. While the island is hidden the overlay peeks it open and it hides again.

use windows::core::GUID;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL, STGM_READ};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::UI::WindowsAndMessaging::{
    RegisterDeviceNotificationW, DBT_DEVTYP_DEVICEINTERFACE, DEVICE_NOTIFY_WINDOW_HANDLE,
    DEV_BROADCAST_DEVICEINTERFACE_W,
};

use crate::anim::now;
use crate::app::{App, Timer};
use crate::gfx::{hex, rgba, Gfx};
use crate::layout::Mode;
use crate::sound;
use crate::text::{self, Align, Face};
use crate::ui::pal;

/// Size of the idle bar while an overlay is up.
pub const OVERLAY_W: f32 = 330.0;
pub const OVERLAY_H: f32 = 46.0;
const SHOW_FOR: f32 = 3.4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OverlayKind {
    Charging,
    Unplugged,
    Low,
    Headphones,
    HeadphonesOff,
    Info,
    Volume,
    Caps,
}

#[derive(Clone, Debug)]
pub struct Overlay {
    pub kind: OverlayKind,
    pub title: String,
    pub sub: String,
    pub metric: String,
}

/// (charger connected, battery percent) — None when this machine has no battery data.
pub fn power() -> Option<(bool, Option<u8>)> {
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut s).ok()? };
    let pct = (s.BatteryLifePercent <= 100).then_some(s.BatteryLifePercent);
    // No battery at all (a desktop PC): nothing to announce.
    if s.BatteryFlag == 128 {
        return None;
    }
    match s.ACLineStatus {
        0 => Some((false, pct)),
        1 => Some((true, pct)),
        _ => None,
    }
}

/// What a connected output device calls itself: "Headphones (AirPods Pro)".
pub fn default_output_name() -> Option<String> {
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let store = dev.OpenPropertyStore(STGM_READ).ok()?;
        let v = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let name = v.to_string();
        (!name.trim().is_empty()).then_some(name)
    }
}

/// Whether an output's name says it is worn on the head.
pub fn looks_like_headphones(name: &str) -> bool {
    let n = name.to_lowercase();
    ["headphone", "headset", "airpods", "earbud", "earphone", "buds", "hands-free", "bluetooth"]
        .iter()
        .any(|k| n.contains(k))
}

/// "Headphones (AirPods Pro)" → "AirPods Pro"; a name without brackets stays as it is.
pub fn short_name(name: &str) -> String {
    match (name.find('('), name.rfind(')')) {
        (Some(a), Some(b)) if b > a + 1 => name[a + 1..b].trim().to_string(),
        _ => name.trim().to_string(),
    }
}

/// Registers the window for audio device arrivals and removals.
pub fn register_audio_notifications(hwnd: windows::Win32::Foundation::HWND) {
    unsafe {
        let mut f = DEV_BROADCAST_DEVICEINTERFACE_W::default();
        f.dbcc_size = std::mem::size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>() as u32;
        f.dbcc_devicetype = DBT_DEVTYP_DEVICEINTERFACE.0;
        // KSCATEGORY_AUDIO
        f.dbcc_classguid = GUID::from_u128(0x6994AD04_93EF_11D0_A3CC_00A0C9223196);
        let _ = RegisterDeviceNotificationW(HANDLE(hwnd.0), &f as *const _ as *const _, DEVICE_NOTIFY_WINDOW_HANDLE);
    }
}

impl App {
    /// Starts the overlay; skipped while the island is open for something else.
    pub fn show_overlay(&mut self, o: Overlay) {
        if self.st.mode == Mode::Expanded || self.st.paused {
            return;
        }
        self.overlay = Some(o);
        self.overlay_level = None;
        sound::play("blip");
        if self.st.mode == Mode::Hidden {
            self.reveal();
        }
        self.cancel_timer(|t| matches!(t, Timer::OverlayEnd));
        self.later(SHOW_FOR, Timer::OverlayEnd);
        self.animate_geometry(false);
        self.ensure_running();
    }

    pub fn end_overlay(&mut self) {
        self.overlay_level = None;
        if self.overlay.take().is_some() {
            self.animate_geometry(true);
            self.ensure_running();
        }
    }

    /// WM_POWERBROADCAST / PBT_APMPOWERSTATUSCHANGE.
    pub fn on_power_status(&mut self) {
        let Some((ac, pct)) = power() else { return };
        let metric = pct.map(|p| format!("{p}%")).unwrap_or_default();
        let before = self.ac_online;
        self.ac_online = Some(ac);
        if let Some(prev) = before {
            if prev != ac {
                self.batt_warned = 0;
                self.show_overlay(if ac {
                    Overlay { kind: OverlayKind::Charging, title: "Mengisi daya".into(), sub: "Charger terhubung".into(), metric }
                } else {
                    Overlay { kind: OverlayKind::Unplugged, title: "Memakai baterai".into(), sub: "Charger dicabut".into(), metric }
                });
                return;
            }
        }
        // Low battery: once at 20 % and once at 10 %.
        if let (false, Some(p)) = (ac, pct) {
            let level = if p <= 10 { 10 } else if p <= 20 { 20 } else { 0 };
            if level != 0 && (self.batt_warned == 0 || level < self.batt_warned) {
                self.batt_warned = level;
                self.show_overlay(Overlay {
                    kind: OverlayKind::Low,
                    title: "Baterai lemah".into(),
                    sub: "Colokkan charger".into(),
                    metric: format!("{p}%"),
                });
            }
        }
    }

    /// WM_DEVICECHANGE: an audio device came or went. Windows needs a moment to pick the
    /// default output, so the look happens a second and a half later.
    pub fn on_device_change(&mut self) {
        self.cancel_timer(|t| matches!(t, Timer::DeviceCheck));
        self.later(1.5, Timer::DeviceCheck);
    }

    pub fn check_headphones(&mut self) {
        let Some(name) = default_output_name() else { return };
        let prev = self.output_name.replace(name.clone());
        let Some(prev) = prev else { return };
        if prev == name {
            return;
        }
        if looks_like_headphones(&name) {
            self.show_overlay(Overlay {
                kind: OverlayKind::Headphones,
                title: short_name(&name),
                sub: "Tersambung".into(),
                metric: String::new(),
            });
        } else if looks_like_headphones(&prev) {
            self.show_overlay(Overlay {
                kind: OverlayKind::HeadphonesOff,
                title: short_name(&prev),
                sub: "Terputus".into(),
                metric: String::new(),
            });
        }
    }

    /// The overlay itself, in the bar: Mochi stays on the left as always.
    pub fn draw_overlay(&mut self, g: &mut Gfx, iw: f32, ih: f32) {
        let Some(o) = self.overlay.clone() else { return };
        let cy = ih / 2.0;
        let col = match o.kind {
            OverlayKind::Charging => "#22C55E",
            OverlayKind::Low => "#F4505E",
            OverlayKind::Headphones | OverlayKind::Volume => "#38BDF8",
            OverlayKind::Caps => "#F5A524",
            OverlayKind::Unplugged | OverlayKind::HeadphonesOff | OverlayKind::Info => "#9398A1",
        };
        let cx = 66.0;
        let c = hex(col);
        g.fill_style(crate::gfx::with_alpha(c, 0.14));
        g.begin_path();
        g.circle(cx, cy, 14.0);
        g.fill();
        g.stroke_style(crate::gfx::with_alpha(c, 0.85));
        g.line_width(1.6);
        g.begin_path();
        g.circle(cx, cy, 13.2);
        g.stroke();

        g.fill_style(c);
        g.stroke_style(c);
        match o.kind {
            OverlayKind::Charging | OverlayKind::Unplugged => {
                // A bolt.
                g.begin_path();
                for (i, (px, py)) in [(1.6f32, -7.0f32), (-4.2, 1.2), (-0.2, 1.2), (-1.6, 7.0), (4.2, -1.2), (0.2, -1.2)].iter().enumerate() {
                    if i == 0 { g.move_to(cx + px, cy + py) } else { g.line_to(cx + px, cy + py) }
                }
                g.close_path();
                g.fill();
            }
            OverlayKind::Headphones | OverlayKind::HeadphonesOff => {
                g.line_width(1.8);
                g.line_cap_round();
                g.begin_path();
                g.arc(cx, cy + 1.0, 6.2, std::f32::consts::PI, std::f32::consts::TAU, false);
                g.stroke();
                g.fill_round_rect(cx - 8.4, cy + 0.2, 3.6, 6.4, 1.4);
                g.fill_round_rect(cx + 4.8, cy + 0.2, 3.6, 6.4, 1.4);
            }
            OverlayKind::Low | OverlayKind::Info => {
                crate::icons::fill(g, crate::icons::Icon::Bang, cx, cy, 12.0, c);
            }
            OverlayKind::Volume => {
                let icon = if o.title == "Senyap" { crate::icons::Icon::SpeakerOff } else { crate::icons::Icon::SpeakerOn };
                crate::icons::fill(g, icon, cx, cy, 14.0, c);
            }
            OverlayKind::Caps => {
                crate::icons::fill(g, crate::icons::Icon::ArrowUp, cx, cy, 13.0, c);
            }
        }

        let metric_w = if o.metric.is_empty() { 0.0 } else { text::measure(&o.metric, Face::Medium, 15.0) + 14.0 };
        let tx = cx + 22.0;
        let room = (iw - 16.0 - metric_w - tx).max(40.0);
        text::draw(g, &text::ellipsize(&o.title, Face::Medium, 12.5, room), tx, cy - 7.0, Face::Medium, 12.5, hex(pal::INK), Align::Left);
        if let Some(level) = self.overlay_level {
            // Volume: a thin bar where the subtitle would be.
            g.fill_style(rgba(255, 255, 255, 0.14));
            g.fill_round_rect(tx, cy + 6.0, room, 4.0, 2.0);
            if level > 0.0 {
                g.fill_style(c);
                g.fill_round_rect(tx, cy + 6.0, (room * level).max(4.0), 4.0, 2.0);
            }
        } else {
            text::draw(g, &text::ellipsize(&o.sub, Face::Regular, 10.5, room), tx, cy + 8.0, Face::Regular, 10.5, hex(pal::DIM), Align::Left);
        }
        if !o.metric.is_empty() {
            text::draw(g, &o.metric, iw - 16.0, cy, Face::Medium, 15.0, hex(pal::INK), Align::Right);
        }
        let _ = (rgba(0, 0, 0, 0.0), now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headphones_are_recognised_by_name() {
        assert!(looks_like_headphones("Headphones (AirPods Pro)"));
        assert!(looks_like_headphones("Headset Earphone (WH-1000XM5 Hands-Free)"));
        assert!(!looks_like_headphones("Speakers (Realtek(R) Audio)"));
        assert!(!looks_like_headphones("LG ULTRAGEAR (NVIDIA High Definition Audio)"));
    }

    /// Manual: `cargo test -p coucou-lite output_on_this_machine -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn output_on_this_machine() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        println!("default output: {:?}", default_output_name());
        println!("power: {:?}", power());
    }

    #[test]
    fn the_device_name_is_shortened_to_what_is_in_brackets() {
        assert_eq!(short_name("Headphones (AirPods Pro)"), "AirPods Pro");
        assert_eq!(short_name("Speakers"), "Speakers");
    }
}

impl App {
    /// At start: remember the charger state and the output device, and ask Windows to tell
    /// us about audio devices. Nothing is polled afterwards.
    pub fn init_system_watch(&mut self) {
        if let Some((ac, _)) = power() {
            self.ac_online = Some(ac);
        }
        self.output_name = default_output_name();
        if !self.platform.hwnd.0.is_null() {
            register_audio_notifications(self.platform.hwnd);
        }
        self.sync_key_hook();
    }

    /// Redraws the idle bar when the minute changes (the resting clock), and only then.
    pub fn clock_tick(&mut self) {
        if self.st.mode != Mode::Compact || self.overlay.is_some() {
            return;
        }
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        let m = t.wHour as i32 * 60 + t.wMinute as i32;
        if m != self.clock_min {
            self.clock_min = m;
            self.ensure_running();
        }
    }

    /// Nusa's resting state: a small ring that fills through the hour, and the time.
    pub fn draw_rest_clock(&mut self, g: &mut Gfx, iw: f32, ih: f32) {
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        let label = format!("{:02}:{:02}", t.wHour, t.wMinute);
        let (x0, x1) = (crate::app::ACT_X, iw - crate::app::ACT_RIGHT);
        let mid = (x0 + x1) / 2.0;
        let cy = ih / 2.0;
        let tw = text::measure(&label, Face::Medium, 12.5);
        let ring_cx = mid - tw / 2.0 - 11.0;
        g.line_width(1.8);
        g.line_cap_round();
        g.stroke_style(rgba(255, 255, 255, 0.16));
        g.begin_path();
        g.circle(ring_cx, cy, 5.2);
        g.stroke();
        let frac = (t.wMinute as f32 + t.wSecond as f32 / 60.0) / 60.0;
        g.stroke_style(hex("#34D399"));
        g.begin_path();
        let a0 = -std::f32::consts::FRAC_PI_2;
        g.arc(ring_cx, cy, 5.2, a0, a0 + frac.max(0.02) * std::f32::consts::TAU, false);
        g.stroke();
        text::draw(g, &label, mid + 4.0, cy, Face::Medium, 12.5, hex(pal::INK), Align::Center);
    }
}
