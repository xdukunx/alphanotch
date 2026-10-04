// The island window: a borderless, always-on-top, never-activating layered
// window whose pixels carry their own alpha (UpdateLayeredWindow). Fully
// transparent pixels let the mouse through by themselves, so there is no
// click-through bookkeeping at all — only the island shape is ever clickable.
//
// While the island is hidden the window shrinks to a 240×6 invisible strip at
// the top edge (alpha 1/255, so it can still be hovered) and nothing else runs.

use std::ffi::c_void;

use tiny_skia::Pixmap;
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW,
    MonitorFromPoint, ReleaseDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL, VK_LBUTTON,
    VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetCursorPos, GetWindowLongPtrW, KillTimer, LoadCursorW,
    RegisterClassExW, SetForegroundWindow, SetTimer, SetWindowLongPtrW, ShowWindow, UpdateLayeredWindow,
    CS_DBLCLKS, GWL_EXSTYLE, IDC_ARROW, SW_SHOWNOACTIVATE, ULW_ALPHA, WM_CHAR, WM_DESTROY,
    WM_DISPLAYCHANGE, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::app;
use crate::ctx::WM_COUCOU_EVENT;
use crate::layout::{PANEL_H, PANEL_W, WAKE_STRIP_H, WAKE_STRIP_W};

pub const TIMER_FRAME: usize = 1;
pub const TIMER_POLL: usize = 2;

pub const WM_TRAY: u32 = 0x8000 + 2;
pub const WM_OPEN: u32 = 0x8000 + 3;

struct Dib {
    hdc: HDC,
    bmp: HBITMAP,
    bits: *mut u8,
    w: i32,
    h: i32,
}

pub struct Platform {
    pub hwnd: HWND,
    /// Device pixels per logical pixel on the monitor the island lives on.
    pub scale: f32,
    /// Window origin in device pixels.
    pub origin: (i32, i32),
    pub collapsed: bool,
    activating: bool,
    dib: Option<Dib>,
    frame_timer: bool,
    frame_ms: u32,
    poll_timer: bool,
    headless: bool,
    /// Rows (device px) the last present converted; the next one must cover them too.
    last_rows: usize,
}

impl Drop for Dib {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.bmp.0));
            let _ = DeleteDC(self.hdc);
        }
    }
}

fn lo(l: LPARAM) -> i32 {
    (l.0 & 0xffff) as i16 as i32
}
fn hi(l: LPARAM) -> i32 {
    ((l.0 >> 16) & 0xffff) as i16 as i32
}

pub fn init_dpi() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

pub fn left_button_down() -> bool {
    unsafe { (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0 }
}

pub fn ctrl_down() -> bool {
    unsafe { (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 }
}

pub fn shift_down() -> bool {
    unsafe { (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 }
}

/// Rectangle (device px) and scale of the display the island should live on.
pub fn target_monitor(pref: &str) -> (RECT, f32) {
    unsafe {
        let hmon = if pref == "cursor" {
            let (x, y) = cursor_pos();
            MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST)
        } else {
            MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY)
        };
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(hmon, &mut info);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        (info.rcMonitor, dx as f32 / 96.0)
    }
}

impl Platform {
    pub fn new(pref: &str) -> Platform {
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            let class = w!("CoucouIsland");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_DBLCLKS,
                lpfnWndProc: Some(wndproc),
                hInstance: hinst.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassExW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class,
                w!("Coucou"),
                WS_POPUP,
                0, 0, 1, 1,
                None, None, Some(hinst.into()), None,
            )
            .expect("island window");

            let mut p = Platform {
                hwnd,
                scale: 1.0,
                origin: (0, 0),
                collapsed: false,
                activating: false,
                dib: None,
                frame_timer: false,
                frame_ms: 16,
                poll_timer: false,
                headless: false,
                last_rows: 0,
            };
            p.apply_geometry(pref, false);
            p
        }
    }

    /// A window-less platform for the snapshot tool.
    #[cfg_attr(not(feature = "snapshot"), allow(dead_code))]
    pub fn headless(scale: f32) -> Platform {
        Platform {
            hwnd: HWND::default(),
            scale,
            origin: (0, 0),
            collapsed: false,
            activating: false,
            dib: None,
            frame_timer: false,
            frame_ms: 16,
            poll_timer: false,
            headless: true,
            last_rows: 0,
        }
    }

    pub fn show(&self) {
        if self.headless {
            return;
        }
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
    }

    /// Logical size of the window in its current form (panel or wake strip).
    pub fn logical_size(&self) -> (f32, f32) {
        if self.collapsed { (WAKE_STRIP_W, WAKE_STRIP_H) } else { (PANEL_W, PANEL_H) }
    }

    pub fn phys_size(&self) -> (i32, i32) {
        let (lw, lh) = self.logical_size();
        ((lw * self.scale).round().max(1.0) as i32, (lh * self.scale).round().max(1.0) as i32)
    }

    /// Places the window on the chosen display, top-centre.
    pub fn apply_geometry(&mut self, pref: &str, collapsed: bool) {
        if self.headless {
            self.collapsed = collapsed;
            return;
        }
        let (rc, scale) = target_monitor(pref);
        self.scale = scale;
        self.collapsed = collapsed;
        let (pw, _ph) = self.phys_size();
        self.origin = (rc.left + ((rc.right - rc.left) - pw) / 2, rc.top);
        self.dib = None; // size may have changed
        self.last_rows = 0;
    }

    fn ensure_dib(&mut self) {
        let (w, h) = self.phys_size();
        if let Some(d) = &self.dib {
            if d.w == w && d.h == h {
                return;
            }
        }
        self.dib = None;
        unsafe {
            let screen = GetDC(None);
            let hdc = CreateCompatibleDC(Some(screen));
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bmp = CreateDIBSection(Some(screen), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
                .unwrap_or_default();
            SelectObject(hdc, HGDIOBJ(bmp.0));
            ReleaseDC(None, screen);
            self.dib = Some(Dib { hdc, bmp, bits: bits as *mut u8, w, h });
        }
    }

    /// Shows `pm` (premultiplied RGBA, exactly `phys_size()`) as the window contents.
    pub fn present(&mut self, pm: &Pixmap, used_rows: usize) {
        if self.headless {
            return;
        }
        self.ensure_dib();
        let Some(d) = &self.dib else { return };
        if pm.width() as i32 != d.w || pm.height() as i32 != d.h || d.bits.is_null() {
            return;
        }
        // Only the rows that can hold anything are converted; the DIB below them
        // stays transparent, as long as rows the previous frame used are covered too.
        let rows = used_rows.max(self.last_rows).min(pm.height() as usize);
        self.last_rows = used_rows.min(pm.height() as usize);
        let n = rows * pm.width() as usize * 4;
        let src = &pm.data()[..n];
        unsafe {
            let dst = std::slice::from_raw_parts_mut(d.bits, src.len());
            for (o, i) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                o[0] = i[2];
                o[1] = i[1];
                o[2] = i[0];
                o[3] = i[3];
            }
            let screen = GetDC(None);
            let pt_dst = POINT { x: self.origin.0, y: self.origin.1 };
            let size = SIZE { cx: d.w, cy: d.h };
            let pt_src = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(
                self.hwnd,
                Some(screen),
                Some(&pt_dst),
                Some(&size),
                Some(d.hdc),
                Some(&pt_src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            ReleaseDC(None, screen);
        }
    }

    /// Cursor position in logical window coordinates.
    pub fn cursor_logical(&self) -> (f32, f32) {
        let (cx, cy) = cursor_pos();
        ((cx - self.origin.0) as f32 / self.scale, (cy - self.origin.1) as f32 / self.scale)
    }

    /// Lets a text field be typed in; the island otherwise never takes focus.
    pub fn set_activating(&mut self, on: bool) {
        if self.headless || self.activating == on {
            return;
        }
        self.activating = on;
        unsafe {
            let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            let flag = WS_EX_NOACTIVATE.0 as isize;
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, if on { ex & !flag } else { ex | flag });
            if on {
                let _ = SetForegroundWindow(self.hwnd);
                let _ = SetFocus(Some(self.hwnd));
            }
        }
    }

    pub fn capture(&self, on: bool) {
        if self.headless {
            return;
        }
        unsafe {
            if on {
                SetCapture(self.hwnd);
            } else {
                let _ = ReleaseCapture();
            }
        }
    }

    pub fn set_frame_timer(&mut self, on: bool) {
        if self.headless || on == self.frame_timer {
            return;
        }
        self.frame_timer = on;
        unsafe {
            if on {
                SetTimer(Some(self.hwnd), TIMER_FRAME, self.frame_ms, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), TIMER_FRAME);
            }
        }
    }

    /// 16 ms while things move for a reason (transitions), 33 ms for ambient motion.
    pub fn set_frame_interval(&mut self, ms: u32) {
        if self.headless || ms == self.frame_ms {
            return;
        }
        self.frame_ms = ms;
        if self.frame_timer {
            unsafe {
                SetTimer(Some(self.hwnd), TIMER_FRAME, ms, None);
            }
        }
    }

    /// Cursor poll + FSM deadlines. Off while hidden, so a hidden island costs nothing.
    pub fn set_poll_timer(&mut self, on: bool) {
        if self.headless || on == self.poll_timer {
            return;
        }
        self.poll_timer = on;
        unsafe {
            if on {
                SetTimer(Some(self.hwnd), TIMER_POLL, 33, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), TIMER_POLL);
            }
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let logical = |lp: LPARAM| -> (f32, f32) {
        app::with_app(|a| {
            let s = a.platform.scale;
            (lo(lp) as f32 / s, hi(lp) as f32 / s)
        })
        .unwrap_or((lo(lp) as f32, hi(lp) as f32))
    };

    match msg {
        WM_MOUSEMOVE => {
            let (x, y) = logical(lp);
            app::with_app(|a| a.on_mouse_move(x, y));
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = logical(lp);
            app::with_app(|a| a.on_mouse_down(x, y));
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = logical(lp);
            app::with_app(|a| a.on_mouse_up(x, y));
            LRESULT(0)
        }
        WM_RBUTTONDOWN => LRESULT(0),
        WM_MOUSEWHEEL => {
            let delta = ((wp.0 >> 16) & 0xffff) as i16 as f32 / 120.0;
            app::with_app(|a| a.on_wheel(delta));
            LRESULT(0)
        }
        WM_CHAR => {
            if let Some(c) = char::from_u32(wp.0 as u32) {
                app::with_app(|a| a.on_char(c));
            }
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let vk = wp.0 as u32;
            let handled = app::with_app(|a| a.on_key(vk)).unwrap_or(false);
            if handled { LRESULT(0) } else { DefWindowProcW(hwnd, msg, wp, lp) }
        }
        WM_KILLFOCUS => {
            app::with_app(|a| a.on_focus_lost());
            LRESULT(0)
        }
        WM_TIMER => {
            let id = wp.0;
            app::with_app(|a| a.on_timer(id));
            LRESULT(0)
        }
        WM_COUCOU_EVENT => {
            app::with_app(|a| a.on_events());
            LRESULT(0)
        }
        WM_TRAY => {
            app::with_app(|a| a.on_tray_message(lp.0 as u32));
            LRESULT(0)
        }
        WM_OPEN => {
            app::with_app(|a| a.on_open_request());
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            app::with_app(|a| a.on_display_change());
            LRESULT(0)
        }
        WM_DESTROY => {
            windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

