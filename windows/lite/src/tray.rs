// Notification-area icon: Open, Settings, Pause, Quit. The icon is drawn by the
// same character code as everything else — no .ico to ship.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, GetDC, ReleaseDC, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos,
    PostMessageW, SetForegroundWindow, TrackPopupMenu, HICON, ICONINFO, MF_CHECKED, MF_SEPARATOR,
    MF_STRING, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_CONTEXTMENU, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP,
};

use crate::gfx::Gfx;
use crate::mochi::engine::BotEngine;
use crate::platform::WM_TRAY;

#[derive(Clone, Copy, Debug)]
pub enum TrayCmd {
    Open,
    Settings,
    Pause,
    Quit,
}

pub struct Tray {
    hwnd: HWND,
    icon: HICON,
}

/// Mochi's face, 32×32, as an HICON.
fn make_icon() -> HICON {
    let mut g = Gfx::new(32, 32, 1.0);
    let mut e = BotEngine::new();
    e.particle_overhang = 0.0;
    let w = 42.0;
    g.save();
    g.translate(16.0 - w / 2.0, 16.0 - (w / 2.0 + w * 0.3 * 0.06));
    e.draw(&mut g, w, w);
    g.restore();

    unsafe {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: 32,
                biHeight: -32,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let screen = GetDC(None);
        let mut bits = std::ptr::null_mut();
        let color = CreateDIBSection(Some(screen), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap_or_default();
        ReleaseDC(None, screen);
        if !bits.is_null() {
            let dst = std::slice::from_raw_parts_mut(bits as *mut u8, 32 * 32 * 4);
            for (o, i) in dst.chunks_exact_mut(4).zip(g.pm.data().chunks_exact(4)) {
                // tiny-skia is premultiplied; icons want straight alpha.
                let a = i[3] as f32 / 255.0;
                let un = |c: u8| if a > 0.0 { ((c as f32 / a).min(255.0)) as u8 } else { 0 };
                o[0] = un(i[2]);
                o[1] = un(i[1]);
                o[2] = un(i[0]);
                o[3] = i[3];
            }
        }
        let mask = CreateBitmap(32, 32, 1, 1, Some([0u8; 128].as_ptr().cast()));
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(HGDIOBJ(mask.0));
        let _ = DeleteObject(HGDIOBJ(color.0));
        icon
    }
}

impl Tray {
    pub fn new(hwnd: HWND) -> Tray {
        let icon = make_icon();
        unsafe {
            let mut nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: 1,
                uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
                uCallbackMessage: WM_TRAY,
                hIcon: icon,
                ..Default::default()
            };
            for (i, c) in "Coucou".encode_utf16().enumerate() {
                nid.szTip[i] = c;
            }
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        }
        Tray { hwnd, icon }
    }

    /// Handles the icon's callback message; returns the chosen menu command, if any.
    pub fn on_message(&mut self, lparam: u32, paused: bool) -> Option<TrayCmd> {
        match lparam {
            WM_LBUTTONUP => Some(TrayCmd::Open),
            WM_RBUTTONUP | WM_CONTEXTMENU => self.menu(paused),
            _ => None,
        }
    }

    fn menu(&mut self, paused: bool) -> Option<TrayCmd> {
        unsafe {
            let menu = CreatePopupMenu().ok()?;
            let add = |id: usize, text: PCWSTR, flags| {
                let _ = AppendMenuW(menu, flags, id, text);
            };
            add(1, w!("Open Coucou"), MF_STRING);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            add(2, w!("Settings…"), MF_STRING);
            add(3, w!("Pause"), if paused { MF_STRING | MF_CHECKED } else { MF_STRING });
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            add(4, w!("Quit"), MF_STRING);

            let mut p = POINT::default();
            let _ = GetCursorPos(&mut p);
            let _ = SetForegroundWindow(self.hwnd);
            let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, p.x, p.y, Some(0), self.hwnd, None);
            let _ = PostMessageW(Some(self.hwnd), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(menu);
            match cmd.0 {
                1 => Some(TrayCmd::Open),
                2 => Some(TrayCmd::Settings),
                3 => Some(TrayCmd::Pause),
                4 => Some(TrayCmd::Quit),
                _ => None,
            }
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                uID: 1,
                ..Default::default()
            };
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            let _ = DestroyIcon(self.icon);
        }
    }
}
