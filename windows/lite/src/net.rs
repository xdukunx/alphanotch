// Internet and Wi-Fi changes, for an overlay like Nusa's. Windows tells us
// (NetworkInformation.NetworkStatusChanged, on a background thread): we post a message to
// the island's window and look a moment later, once the connection has settled.

use windows::Networking::Connectivity::{NetworkConnectivityLevel, NetworkInformation, NetworkStatusChangedEventHandler};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::app::{App, Timer};
use crate::overlay::{Overlay, OverlayKind};

/// Posted to the island's window when the network status changes.
pub const WM_NET: u32 = 0x8000 + 21;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetState {
    pub online: bool,
    pub wifi: bool,
    pub name: String,
}

pub fn state() -> NetState {
    match NetworkInformation::GetInternetConnectionProfile() {
        Ok(p) => {
            let online = p.GetNetworkConnectivityLevel().map(|l| l == NetworkConnectivityLevel::InternetAccess).unwrap_or(false);
            let wifi = p.IsWlanConnectionProfile().unwrap_or(false);
            let name = p.ProfileName().map(|h| h.to_string()).unwrap_or_default();
            NetState { online, wifi, name }
        }
        Err(_) => NetState { online: false, wifi: false, name: String::new() },
    }
}

pub fn watch(hwnd: HWND) {
    let h = hwnd.0 as isize;
    let handler = NetworkStatusChangedEventHandler::new(move |_| {
        unsafe {
            let _ = PostMessageW(Some(HWND(h as *mut _)), WM_NET, WPARAM(0), LPARAM(0));
        }
        Ok(())
    });
    let _ = NetworkInformation::NetworkStatusChanged(&handler);
}

/// What changed between two readings, as an overlay (None when nothing worth saying did).
pub fn change(prev: &NetState, now: &NetState) -> Option<Overlay> {
    if prev.online && !now.online {
        return Some(Overlay { kind: OverlayKind::WifiOff, title: "Koneksi terputus".into(), sub: "Tidak ada internet".into(), metric: String::new() });
    }
    let label = |s: &NetState| if s.name.is_empty() { if s.wifi { "Wi-Fi".to_string() } else { "Internet".to_string() } } else { s.name.clone() };
    if !prev.online && now.online {
        return Some(Overlay { kind: OverlayKind::Wifi, title: label(now), sub: "Tersambung".into(), metric: String::new() });
    }
    if prev.online && now.online && prev.name != now.name && !now.name.is_empty() {
        return Some(Overlay { kind: OverlayKind::Wifi, title: label(now), sub: "Pindah jaringan".into(), metric: String::new() });
    }
    None
}

impl App {
    pub fn on_net_change(&mut self) {
        self.cancel_timer(|t| matches!(t, Timer::NetCheck));
        self.later(2.0, Timer::NetCheck);
    }

    pub fn net_check(&mut self) {
        let now = state();
        let prev = self.net_state.replace(now.clone());
        if let Some(prev) = prev {
            if let Some(o) = change(&prev, &now) {
                self.show_overlay(o);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(online: bool, name: &str) -> NetState {
        NetState { online, wifi: true, name: name.into() }
    }

    #[test]
    fn losing_and_getting_back_the_internet_are_announced() {
        assert_eq!(change(&st(true, "Rumah"), &st(false, "")).unwrap().title, "Koneksi terputus");
        let back = change(&st(false, ""), &st(true, "Rumah")).unwrap();
        assert_eq!((back.title.as_str(), back.sub.as_str()), ("Rumah", "Tersambung"));
        assert_eq!(change(&st(true, "Rumah"), &st(true, "Kantor")).unwrap().sub, "Pindah jaringan");
        assert!(change(&st(true, "Rumah"), &st(true, "Rumah")).is_none());
    }
}
