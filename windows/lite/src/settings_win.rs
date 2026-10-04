// The settings window — the one place where anything that writes to disk is
// confirmed: Claude Code hooks (with the exact diff), the API key, integrations
// and general preferences. Drawn with the same toolkit as the island, in an
// ordinary resizable window.

use std::cell::RefCell;
use std::collections::HashMap;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetDC, InvalidateRect, ReleaseDC, StretchDIBits, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, PAINTSTRUCT, SRCCOPY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, KillTimer, LoadCursorW, RegisterClassExW,
    SetForegroundWindow, SetTimer, ShowWindow, CS_DBLCLKS, CW_USEDEFAULT, IDC_ARROW, SW_HIDE,
    SW_RESTORE, SW_SHOW, WM_CHAR, WM_CLOSE, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_SIZE, WM_TIMER, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};

use crate::app;
use crate::gfx::{hex, rgb, rgba, with_alpha, Gfx};
use crate::hooks::{self, HookStatus};
use crate::secrets;
use crate::settings::{self, Settings};
use crate::text::{self, Align, Face};
use crate::textfield::TextField;
use crate::ui::{self, id_of, pal, Rect, Ui};

const BG: &str = "#0B0C0E";

const MODELS: [(&str, &str); 3] = [
    ("claude-opus-5", "Claude Opus 5"),
    ("claude-sonnet-5", "Claude Sonnet 5"),
    ("claude-haiku-4-5", "Claude Haiku 4.5"),
];

struct Field {
    key: &'static str,
    label: &'static str,
    placeholder: &'static str,
    secret: bool,
}

struct Integration {
    id: &'static str,
    name: &'static str,
    color: &'static str,
    fields: &'static [Field],
}

const INTEGRATIONS: &[Integration] = &[
    Integration { id: "integration_stripe", name: "Stripe", color: "#0570DE", fields: &[Field { key: "stripe-api-key", label: "Secret key", placeholder: "sk_live_…", secret: true }] },
    Integration { id: "integration_github", name: "GitHub", color: "#F4505E", fields: &[Field { key: "github-token", label: "Token", placeholder: "ghp_…", secret: true }] },
    Integration { id: "integration_vercel", name: "Vercel", color: "#7C5CFF", fields: &[Field { key: "vercel-token", label: "Token", placeholder: "…", secret: true }] },
    Integration {
        id: "integration_n8n", name: "n8n", color: "#F29B38",
        fields: &[
            Field { key: "n8n-url", label: "Instance URL", placeholder: "https://n8n.example.com", secret: false },
            Field { key: "n8n-api-key", label: "API key", placeholder: "…", secret: true },
        ],
    },
    Integration { id: "integration_resend", name: "Resend", color: "#22C55E", fields: &[Field { key: "resend-api-key", label: "API key", placeholder: "re_…", secret: true }] },
    Integration { id: "integration_notion", name: "Notion", color: "#8C8C8C", fields: &[Field { key: "notion-api-key", label: "Integration token", placeholder: "ntn_…", secret: true }] },
    Integration { id: "integration_calcom", name: "Cal.com", color: "#C9956A", fields: &[Field { key: "calcom-api-key", label: "API key", placeholder: "cal_…", secret: true }] },
];

const MAX_ACTIVE: usize = 4;

enum HooksMode {
    Normal,
    Preview { install: bool, diff: String, backup: String, fingerprint: String },
    Notice { text: String, ok: bool, at: f32 },
}

struct Win {
    hwnd: HWND,
    gfx: Gfx,
    scale: f32,
    cw: i32,
    ch: i32,
    ui: Ui,
    scroll: f32,
    content_h: f32,
    fields: HashMap<&'static str, TextField>,
    focused: Option<&'static str>,
    present: HashMap<&'static str, bool>,
    notes: HashMap<&'static str, (String, bool)>,
    status: HookStatus,
    mode: HooksMode,
    settings: Settings,
    drag_slider: bool,
    timer: bool,
}

thread_local! {
    static WIN: RefCell<Option<Win>> = const { RefCell::new(None) };
}

fn with_win<R>(f: impl FnOnce(&mut Win) -> R) -> Option<R> {
    WIN.with(|w| {
        let mut b = w.try_borrow_mut().ok()?;
        b.as_mut().map(f)
    })
}

/// Opens (or raises) the settings window.
pub fn show() {
    let existing = with_win(|w| {
        w.reload();
        unsafe {
            let _ = ShowWindow(w.hwnd, SW_RESTORE);
            let _ = ShowWindow(w.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(w.hwnd);
        }
        w.invalidate();
    });
    if existing.is_some() {
        return;
    }
    create();
}

/// Something changed elsewhere (a click in the island): redraw if we are open.
pub fn refresh() {
    with_win(|w| {
        w.settings = app::with_app(|a| a.st.settings.clone()).unwrap_or_else(|| w.settings.clone());
        w.invalidate();
    });
}

fn create() {
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("CoucouSettings");
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
        let created = CreateWindowExW(
            Default::default(),
            class,
            w!("Settings — Coucou"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT, CW_USEDEFAULT, 620, 760,
            None, None, Some(hinst.into()), None,
        );
        let hwnd = match created {
            Ok(h) => h,
            Err(e) => {
                crate::log::line(format!("settings window failed: {e}"));
                return;
            }
        };
        let dark: i32 = 1;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as *const _, 4);
        let dpi = GetDpiForWindow(hwnd).max(96);
        let scale = dpi as f32 / 96.0;
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let (cw, ch) = ((rc.right - rc.left).max(1), (rc.bottom - rc.top).max(1));

        let mut win = Win {
            hwnd,
            gfx: Gfx::new(cw as u32, ch as u32, scale),
            scale,
            cw,
            ch,
            ui: Ui::new(),
            scroll: 0.0,
            content_h: 0.0,
            fields: HashMap::new(),
            focused: None,
            present: HashMap::new(),
            notes: HashMap::new(),
            status: hooks::status(),
            mode: HooksMode::Normal,
            settings: Settings::default(),
            drag_slider: false,
            timer: false,
        };
        win.build_fields();
        win.reload();
        WIN.with(|w| *w.borrow_mut() = Some(win));
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
    }
}

impl Win {
    fn build_fields(&mut self) {
        self.fields.insert("anthropic-api-key", TextField::new("sk-ant-…", true));
        for i in INTEGRATIONS {
            for f in i.fields {
                self.fields.insert(f.key, TextField::new(f.placeholder, f.secret));
            }
        }
    }

    /// Re-reads everything that can change outside this window.
    fn reload(&mut self) {
        self.status = hooks::status();
        self.settings = app::with_app(|a| a.st.settings.clone()).unwrap_or_else(settings::load);
        self.present.clear();
        self.present.insert("anthropic-api-key", secrets::present("anthropic-api-key"));
        for i in INTEGRATIONS {
            for f in i.fields {
                self.present.insert(f.key, secrets::present(f.key));
            }
        }
    }

    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn resize(&mut self) {
        unsafe {
            let mut rc = RECT::default();
            let _ = GetClientRect(self.hwnd, &mut rc);
            self.cw = (rc.right - rc.left).max(1);
            self.ch = (rc.bottom - rc.top).max(1);
        }
        self.gfx = Gfx::new(self.cw as u32, self.ch as u32, self.scale);
    }

    fn set_timer(&mut self, on: bool) {
        if on == self.timer {
            return;
        }
        self.timer = on;
        unsafe {
            if on {
                SetTimer(Some(self.hwnd), 1, 500, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), 1);
            }
        }
    }

    fn save(&mut self) {
        let s = self.settings.clone();
        // The island owns the live copy; hand it the change so both stay in step.
        let applied = app::with_app(|a| {
            a.apply_settings(s.clone());
            let _ = settings::save(&s);
        });
        if applied.is_none() {
            let _ = settings::save(&s);
        }
    }

    // ── Painting ──────────────────────────────────────────────────────────────

    fn paint(&mut self) {
        let mut g = std::mem::replace(&mut self.gfx, Gfx::new(1, 1, 1.0));
        g.clear();
        g.fill_style(hex(BG));
        g.fill_rect(0.0, 0.0, 100_000.0, 100_000.0);

        let lw = self.cw as f32 / self.scale;
        let lh = self.ch as f32 / self.scale;
        self.ui.begin_frame();
        g.save();
        g.translate(0.0, -self.scroll);
        let total = self.draw(&mut g, lw);
        g.restore();
        self.content_h = total;
        let max_scroll = (total - lh).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max_scroll);
        if max_scroll > 0.0 {
            let track = lh - 8.0;
            let thumb = (lh / total * track).max(30.0);
            let y = 4.0 + self.scroll / max_scroll * (track - thumb);
            g.fill_style(rgba(255, 255, 255, 0.18));
            g.fill_round_rect(lw - 8.0, y, 4.0, thumb, 2.0);
        }
        self.ui.end_frame();
        self.gfx = g;
        self.blit();

        let any_focus = self.focused.is_some();
        self.set_timer(any_focus);
    }

    fn blit(&self) {
        unsafe {
            let hdc = GetDC(Some(self.hwnd));
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: self.cw,
                    biHeight: -self.ch,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            // The background is opaque, so premultiplied RGBA only needs its R and B swapped.
            let mut data = self.gfx.pm.data().to_vec();
            for px in data.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
            StretchDIBits(hdc, 0, 0, self.cw, self.ch, 0, 0, self.cw, self.ch, Some(data.as_ptr().cast()), &bmi, DIB_RGB_COLORS, SRCCOPY);
            ReleaseDC(Some(self.hwnd), hdc);
        }
    }

    fn mouse_content(&self, x: i32, y: i32) -> (f32, f32) {
        (x as f32 / self.scale, y as f32 / self.scale + self.scroll)
    }

    // ── Layout ────────────────────────────────────────────────────────────────

    fn draw(&mut self, g: &mut Gfx, lw: f32) -> f32 {
        let m = 24.0;
        let cw = (lw - 2.0 * m).max(380.0);
        let mut y = 22.0;

        text::draw(g, "Coucou", m, y + 12.0, Face::Bold, 20.0, hex(pal::INK), Align::Left);
        text::draw(g, concat!("Lite ", env!("CARGO_PKG_VERSION")), m + 78.0, y + 15.0, Face::Regular, 11.0, hex(pal::DIM3), Align::Left);
        y += 40.0;

        y = self.claude_code(g, m, y, cw);
        y = self.claude_api(g, m, y, cw);
        y = self.integrations(g, m, y, cw);
        y = self.general(g, m, y, cw);

        text::draw(g, "No telemetry. Network requests only go to the services you configure yourself.", m, y + 8.0, Face::Regular, 11.0, hex(pal::DIM3), Align::Left);
        y + 40.0
    }

    /// A section card; returns the inner x, width and the content top.
    fn card(g: &mut Gfx, x: f32, y: f32, w: f32, h: f32) {
        g.fill_style(hex(pal::CARD));
        g.fill_round_rect(x, y, w, h, 14.0);
        g.stroke_style(rgba(255, 255, 255, 0.05));
        g.line_width(1.0);
        g.round_rect(x + 0.5, y + 0.5, w - 1.0, h - 1.0, 13.5);
        g.stroke();
    }

    fn title(g: &mut Gfx, x: f32, y: f32, ok: Option<bool>, label: &str) {
        let mut tx = x;
        if let Some(ok) = ok {
            ui::dot(g, x + 4.0, y, 8.0, hex(if ok { pal::GREEN } else { pal::RED }));
            tx += 16.0;
        }
        text::draw(g, label, tx, y, Face::Medium, 14.0, hex(pal::INK), Align::Left);
    }

    fn para(g: &mut Gfx, x: f32, y: f32, w: f32, s: &str) -> f32 {
        let lines = text::wrap(s, Face::Regular, 12.0, w);
        for (i, l) in lines.iter().enumerate() {
            text::draw(g, l, x, y + 8.0 + i as f32 * 17.0, Face::Regular, 12.0, hex(pal::DIM), Align::Left);
        }
        lines.len() as f32 * 17.0
    }

    fn notice(g: &mut Gfx, x: f32, y: f32, w: f32, s: &str, ok: bool, warn: bool) -> f32 {
        let col = if ok { "#22C55E" } else if warn { "#F5A524" } else { "#F4505E" };
        let lines = text::wrap(s, Face::Regular, 11.5, w - 24.0);
        let h = lines.len() as f32 * 16.0 + 14.0;
        g.fill_style(with_alpha(hex(col), 0.12));
        g.fill_round_rect(x, y, w, h, 10.0);
        for (i, l) in lines.iter().enumerate() {
            text::draw(g, l, x + 12.0, y + 7.0 + 8.0 + i as f32 * 16.0, Face::Regular, 11.5, with_alpha(hex(col), 1.0), Align::Left);
        }
        h
    }

    fn kv(g: &mut Gfx, x: f32, y: f32, w: f32, label: &str, value: &str, dot: Option<bool>) {
        text::draw(g, label, x, y + 9.0, Face::Regular, 12.0, hex(pal::DIM), Align::Left);
        let room = w - 110.0 - if dot.is_some() { 18.0 } else { 0.0 };
        let v = text::ellipsize(value, Face::Mono, 11.0, room);
        text::draw(g, &v, x + 110.0, y + 9.0, Face::Mono, 11.0, hex("#C5C8CD"), Align::Left);
        if let Some(ok) = dot {
            ui::dot(g, x + w - 6.0, y + 9.0, 8.0, hex(if ok { pal::GREEN } else { pal::RED }));
        }
    }

    // ── Claude Code ───────────────────────────────────────────────────────────

    fn claude_code(&mut self, g: &mut Gfx, m: f32, y0: f32, cw: f32) -> f32 {
        let st = &self.status;
        let (installed, spath, hpath, ready) = (st.installed, st.settings_path.clone(), st.hook_path.clone(), st.hook_ready);
        let pad = 18.0;
        let ix = m + pad;
        let iw = cw - 2.0 * pad;

        // Measure first so the card background can be drawn behind the content.
        let mut probe = Gfx::new(1, 1, 1.0);
        let h = self.claude_code_body(&mut probe, ix, 0.0, iw, installed, &spath, &hpath, ready, false);
        Self::card(g, m, y0, cw, h + 2.0 * pad);
        let ui_ptr: *mut Ui = &mut self.ui;
        let _ = ui_ptr;
        self.claude_code_body(g, ix, y0 + pad, iw, installed, &spath, &hpath, ready, true);
        y0 + h + 2.0 * pad + 16.0
    }

    #[allow(clippy::too_many_arguments)]
    fn claude_code_body(&mut self, g: &mut Gfx, x: f32, y0: f32, w: f32, installed: bool, spath: &str, hpath: &str, ready: bool, live: bool) -> f32 {
        let mut y = y0;
        Self::title(g, x, y + 9.0, Some(installed), "Claude Code");
        y += 28.0;

        match &self.mode {
            HooksMode::Normal => {
                let hint = if installed {
                    "Coucou is hooked into your Claude Code sessions. Tool calls, questions and permission requests show up in the island, and you can answer them there."
                } else {
                    "Install the hooks to see your Claude Code sessions in the island and approve permissions without leaving what you are doing."
                };
                y += Self::para(g, x, y, w, hint) + 8.0;
                Self::kv(g, x, y, w, "settings.json", spath, None);
                y += 22.0;
                Self::kv(g, x, y, w, "Relay", hpath, Some(ready));
                y += 28.0;
                if !ready {
                    y += Self::notice(g, x, y, w, "coucou-hook.exe is not in place yet. Restart Coucou; if it still fails, build it with `cargo build -p coucou-hook`.", false, true) + 10.0;
                }
                let label = if installed { "Reinstall hooks…" } else { "Install hooks…" };
                let (clicked, bw) = if live {
                    let (c, bw) = self.ui.button(g, x, y, label, true, None);
                    (c && ready, bw)
                } else {
                    (false, text::measure(label, Face::Medium, 12.5) + 26.0)
                };
                if clicked {
                    self.open_preview(true);
                }
                if installed && live {
                    let (c, _) = self.ui.button(g, x + bw + 8.0, y, "Uninstall hooks…", false, None);
                    if c {
                        self.open_preview(false);
                    }
                } else if installed {
                    let _ = bw;
                }
                y += 30.0;
            }
            HooksMode::Preview { install, diff, backup, fingerprint } => {
                let (install, diff, backup, fingerprint) = (*install, diff.clone(), backup.clone(), fingerprint.clone());
                let hint = if install {
                    "This is exactly what will change in your settings.json. Your own hooks are left untouched."
                } else {
                    "This removes Coucou's entries only. Your own hooks are left untouched."
                };
                y += Self::para(g, x, y, w, hint) + 8.0;
                // Diff box.
                let lines: Vec<&str> = diff.lines().take(22).collect();
                let bh = lines.len() as f32 * 15.0 + 16.0;
                g.fill_style(rgba(0, 0, 0, 0.35));
                g.fill_round_rect(x, y, w, bh, 10.0);
                for (i, l) in lines.iter().enumerate() {
                    let ly = y + 8.0 + 7.5 + i as f32 * 15.0;
                    let (bg, fg) = if l.starts_with('+') {
                        (Some(rgba(34, 197, 94, 0.14)), "#8FE3AE")
                    } else if l.starts_with('-') {
                        (Some(rgba(244, 80, 94, 0.14)), "#FF9AA3")
                    } else {
                        (None, "#9398A1")
                    };
                    if let Some(bg) = bg {
                        g.fill_style(bg);
                        g.fill_rect(x + 4.0, ly - 7.5, w - 8.0, 15.0);
                    }
                    let t = text::ellipsize(l, Face::Mono, 10.5, w - 20.0);
                    text::draw(g, &t, x + 10.0, ly, Face::Mono, 10.5, hex(fg), Align::Left);
                }
                y += bh + 8.0;
                let b = text::ellipsize(&format!("Backup → {backup}"), Face::Mono, 10.5, w);
                text::draw(g, &b, x, y + 8.0, Face::Mono, 10.5, hex(pal::DIM3), Align::Left);
                y += 24.0;
                if live {
                    let label = if install { "Back up and write" } else { "Back up and remove" };
                    let (c, bw) = self.ui.button(g, x, y, label, true, None);
                    let (cancel, _) = self.ui.button(g, x + bw + 8.0, y, "Cancel", false, None);
                    if c {
                        match hooks::write(install, &fingerprint) {
                            Ok(b) => {
                                self.mode = HooksMode::Notice {
                                    text: format!("Done. Previous settings saved as {b}. Open a new Claude Code session to pick the hooks up."),
                                    ok: true,
                                    at: crate::anim::now(),
                                };
                                self.reload();
                            }
                            Err(e) => {
                                self.mode = HooksMode::Notice { text: format!("Could not write: {e}"), ok: false, at: crate::anim::now() };
                            }
                        }
                    } else if cancel {
                        self.mode = HooksMode::Normal;
                    }
                }
                y += 30.0;
            }
            HooksMode::Notice { text: t, ok, at } => {
                let (t, ok, at) = (t.clone(), *ok, *at);
                y += Self::notice(g, x, y, w, &t, ok, false) + 10.0;
                if live {
                    let (c, _) = self.ui.button(g, x, y, "OK", false, None);
                    if c || (ok && crate::anim::now() - at > 6.0) {
                        self.mode = HooksMode::Normal;
                    }
                }
                y += 30.0;
            }
        }
        y - y0
    }

    fn open_preview(&mut self, install: bool) {
        match hooks::preview(install) {
            Ok(p) => {
                self.mode = HooksMode::Preview { install, diff: p.diff, backup: p.backup, fingerprint: p.fingerprint };
            }
            Err(e) => {
                // An unreadable or invalid settings.json stops here rather than
                // being treated as empty and written over.
                self.mode = HooksMode::Notice { text: e, ok: false, at: crate::anim::now() };
            }
        }
    }

    // ── Claude API ────────────────────────────────────────────────────────────

    fn claude_api(&mut self, g: &mut Gfx, m: f32, y0: f32, cw: f32) -> f32 {
        let pad = 18.0;
        let (ix, iw) = (m + pad, cw - 2.0 * pad);
        let has = *self.present.get("anthropic-api-key").unwrap_or(&false);
        let note = self.notes.get("anthropic-api-key").cloned();
        let mut h = 28.0 + 22.0 + 40.0 + 36.0;
        if note.is_some() {
            h += 36.0;
        }
        Self::card(g, m, y0, cw, h + 2.0 * pad);
        let mut y = y0 + pad;
        Self::title(g, ix, y + 9.0, Some(has), "Claude");
        y += 28.0;
        let hint = if has { "Key saved in the Windows Credential Manager." } else { "No key yet — the chat needs one." };
        text::draw(g, hint, ix, y + 8.0, Face::Regular, 12.0, hex(pal::DIM), Align::Left);
        y += 26.0;

        // API key row.
        text::draw(g, "API key", ix, y + 14.0, Face::Regular, 12.0, hex("#C5C8CD"), Align::Left);
        let fx = ix + 104.0;
        let (save_w, rm_w) = (text::measure("Save key", Face::Medium, 12.5) + 26.0, text::measure("Remove", Face::Medium, 12.5) + 26.0);
        let fw = iw - 104.0 - save_w - 8.0 - if has { rm_w + 8.0 } else { 0.0 };
        self.field(g, "anthropic-api-key", Rect::new(fx, y, fw, 28.0), has);
        let (save, _) = self.ui.button(g, fx + fw + 8.0, y - 1.0, "Save key", true, None);
        let mut remove = false;
        if has {
            let (c, _) = self.ui.button(g, fx + fw + 8.0 + save_w + 8.0, y - 1.0, "Remove", false, None);
            remove = c;
        }
        y += 40.0;
        if save {
            self.save_secret("anthropic-api-key");
        }
        if remove {
            match secrets::clear("anthropic-api-key") {
                Ok(()) => {
                    self.notes.insert("anthropic-api-key", ("Key removed.".into(), true));
                }
                Err(e) => {
                    self.notes.insert("anthropic-api-key", (format!("Could not remove: {e}"), false));
                }
            }
            self.reload();
            app::with_app(|a| a.refresh_flags());
        }

        // Model.
        text::draw(g, "Model", ix, y + 14.0, Face::Regular, 12.0, hex("#C5C8CD"), Align::Left);
        let mut mx = fx;
        let current = self.settings.model.clone();
        let mut options: Vec<(String, String)> = MODELS.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        if !options.iter().any(|(id, _)| *id == current) {
            options.push((current.clone(), current.clone()));
        }
        for (id, label) in options {
            let pw = text::measure(&label, Face::Regular, 11.5) + 18.0;
            let r = Rect::new(mx, y, pw, 28.0);
            let (clicked, hover, _) = self.ui.click_region(id_of(&id, 71), r);
            let on = id == current;
            g.fill_style(if on { hex("#252830") } else { rgba(255, 255, 255, if hover { 0.1 } else { 0.05 }) });
            g.fill_round_rect(r.x, r.y, r.w, r.h, 14.0);
            text::draw(g, &label, r.cx(), r.cy(), Face::Regular, 11.5, hex(if on { pal::INK } else { pal::DIM2 }), Align::Center);
            mx += pw + 5.0;
            if clicked && !on {
                self.settings.model = id.clone();
                self.save();
            }
        }
        y += 38.0;

        if let Some((msg, ok)) = note {
            Self::notice(g, ix, y, iw, &msg, ok, false);
        }
        y0 + h + 2.0 * pad + 16.0
    }

    fn save_secret(&mut self, key: &'static str) {
        let value = self.fields.get(key).map(|f| f.text().trim().to_string()).unwrap_or_default();
        if value.is_empty() && key == "anthropic-api-key" {
            return;
        }
        match secrets::set(key, &value) {
            Ok(()) => {
                if let Some(f) = self.fields.get_mut(key) {
                    f.clear();
                }
                let msg = if key == "anthropic-api-key" { "Saved. It never touches disk." } else { "Saved." };
                self.notes.insert(key, (msg.into(), true));
            }
            Err(e) => {
                self.notes.insert(key, (format!("Could not save: {e}"), false));
            }
        }
        self.reload();
        app::with_app(|a| {
            a.refresh_flags();
        });
    }

    /// A password / text input drawn on a soft pill.
    fn field(&mut self, g: &mut Gfx, key: &'static str, r: Rect, stored: bool) {
        let focused = self.focused == Some(key);
        g.fill_style(rgba(255, 255, 255, 0.07));
        g.fill_round_rect(r.x, r.y, r.w, r.h, 8.0);
        if focused {
            g.stroke_style(rgba(99, 140, 255, 0.7));
            g.line_width(1.2);
            g.round_rect(r.x + 0.6, r.y + 0.6, r.w - 1.2, r.h - 1.2, 7.4);
            g.stroke();
        }
        let hover = r.contains(self.ui.input.mouse.0, self.ui.input.mouse.1);
        if self.ui.input.pressed && hover {
            self.focused = Some(key);
            for (k, f) in self.fields.iter_mut() {
                f.focused = *k == key;
            }
            let shift = crate::platform::shift_down();
            if let Some(f) = self.fields.get_mut(key) {
                f.click_at(self.ui.input.mouse.0, 12.5, shift);
            }
        }
        if let Some(f) = self.fields.get_mut(key) {
            f.focused = self.focused == Some(key);
            if f.is_empty() && stored {
                f.placeholder = "••••••••  (stored)".into();
            } else if f.is_empty() {
                // restore the original hint
                if let Some(p) = Self::placeholder_for(key) {
                    f.placeholder = p.into();
                }
            }
            f.draw(g, Rect::new(r.x + 10.0, r.y, r.w - 20.0, r.h), 12.5, pal::INK);
        }
    }

    fn placeholder_for(key: &str) -> Option<&'static str> {
        if key == "anthropic-api-key" {
            return Some("sk-ant-…");
        }
        INTEGRATIONS.iter().flat_map(|i| i.fields.iter()).find(|f| f.key == key).map(|f| f.placeholder)
    }

    // ── Integrations ──────────────────────────────────────────────────────────

    fn integrations(&mut self, g: &mut Gfx, m: f32, y0: f32, cw: f32) -> f32 {
        let pad = 18.0;
        let (ix, iw) = (m + pad, cw - 2.0 * pad);
        let mut h = 28.0 + 40.0;
        for i in INTEGRATIONS {
            h += (i.fields.len() as f32 * 36.0).max(30.0) + 12.0;
        }
        Self::card(g, m, y0, cw, h + 2.0 * pad);
        let mut y = y0 + pad;
        Self::title(g, ix, y + 9.0, None, "Integrations");
        y += 28.0;
        let used = self.settings.active_integrations.len();
        let note = format!("Pick up to {MAX_ACTIVE} pills to show next to Mochi — {used}/{MAX_ACTIVE} in use. Keys are stored in the Windows Credential Manager, never on disk.");
        y += Self::para(g, ix, y, iw, &note) + 10.0;

        for it in INTEGRATIONS {
            let rows_h = (it.fields.len() as f32 * 36.0).max(30.0);
            let active = self.settings.active_integrations.iter().any(|x| x == it.id);
            if self.ui.switch(g, it.id, ix, y + 3.0, active) {
                if active {
                    self.settings.active_integrations.retain(|x| x != it.id);
                } else if self.settings.active_integrations.len() < MAX_ACTIVE {
                    self.settings.active_integrations.push(it.id.to_string());
                }
                self.save();
            }
            ui::dot(g, ix + 46.0, y + 12.0, 8.0, hex(it.color));
            text::draw(g, it.name, ix + 58.0, y + 12.0, Face::Regular, 12.5, hex(pal::INK), Align::Left);

            let fx0 = ix + 150.0;
            let mut fy = y;
            for f in it.fields {
                text::draw(g, f.label, fx0, fy + 14.0, Face::Regular, 11.5, hex(pal::DIM), Align::Left);
                let stored = *self.present.get(f.key).unwrap_or(&false);
                let lab_w = 104.0;
                let btn_w = text::measure("Save", Face::Medium, 12.5) + 26.0;
                let fw = (iw - (fx0 - ix) - lab_w - btn_w - 8.0 - 18.0).max(80.0);
                self.field(g, f.key, Rect::new(fx0 + lab_w, fy, fw, 28.0), stored);
                let (save, _) = self.ui.button(g, fx0 + lab_w + fw + 8.0, fy - 1.0, "Save", false, None);
                let dot = if self.notes.get(f.key).map(|n| !n.1).unwrap_or(false) { pal::AMBER } else if stored { pal::GREEN } else { pal::RED };
                ui::dot(g, fx0 + lab_w + fw + 8.0 + btn_w + 8.0, fy + 14.0, 8.0, hex(dot));
                if save {
                    self.save_secret(f.key);
                }
                fy += 36.0;
            }
            y += rows_h + 12.0;
        }
        y0 + h + 2.0 * pad + 16.0
    }

    // ── General ───────────────────────────────────────────────────────────────

    fn general(&mut self, g: &mut Gfx, m: f32, y0: f32, cw: f32) -> f32 {
        let pad = 18.0;
        let (ix, iw) = (m + pad, cw - 2.0 * pad);
        let h = 28.0 + 5.0 * 38.0;
        Self::card(g, m, y0, cw, h + 2.0 * pad);
        let mut y = y0 + pad;
        Self::title(g, ix, y + 9.0, None, "General");
        y += 32.0;
        let lab = |g: &mut Gfx, y: f32, s: &str| {
            text::draw(g, s, ix, y + 11.0, Face::Regular, 12.0, hex("#C5C8CD"), Align::Left);
        };
        let cx = ix + 150.0;

        // Sound.
        lab(g, y, "Sound");
        let on = self.settings.sound_enabled;
        if self.ui.switch(g, "set-sound", cx, y + 2.0, on) {
            self.settings.sound_enabled = !on;
            self.save();
        }
        let sx = cx + 48.0;
        let sw = 120.0;
        let sr = Rect::new(sx - 6.0, y, sw + 12.0, 22.0);
        let (_, hover, pressed) = self.ui.click_region(id_of("set-vol", 73), sr);
        if pressed || (self.drag_slider && self.ui.input.down) {
            self.drag_slider = true;
            let k = ((self.ui.input.mouse.0 - sx) / sw).clamp(0.0, 1.0);
            let v = ((k * 0.2 / 0.005).round() * 0.005) as f64;
            if (v - self.settings.sound_volume).abs() > 1e-6 {
                self.settings.sound_volume = v;
                crate::sound::set_volume(v as f32);
            }
        }
        if !self.ui.input.down && self.drag_slider {
            self.drag_slider = false;
            self.save();
            crate::sound::play("blip");
        }
        let k = (self.settings.sound_volume / 0.2) as f32;
        g.fill_style(rgba(255, 255, 255, 0.2));
        g.fill_round_rect(sx, y + 9.5, sw, 3.0, 1.5);
        g.fill_style(rgba(255, 255, 255, 0.55));
        g.fill_round_rect(sx, y + 9.5, sw * k, 3.0, 1.5);
        g.fill_style(rgb(255, 255, 255));
        g.circle(sx + sw * k, y + 11.0, if hover || pressed { 6.5 } else { 5.5 });
        g.fill();
        y += 38.0;

        // Auto-close.
        lab(g, y, "Auto-close");
        let secs = self.settings.auto_close_interval.round() as i32;
        let (minus, _) = self.ui.button(g, cx, y - 4.0, "−", false, None);
        text::draw(g, &format!("{secs}"), cx + 60.0, y + 11.0, Face::Medium, 12.5, hex(pal::INK), Align::Center);
        let (plus, _) = self.ui.button(g, cx + 84.0, y - 4.0, "+", false, None);
        text::draw(g, "seconds after you leave the island", cx + 130.0, y + 11.0, Face::Regular, 11.5, hex(pal::DIM3), Align::Left);
        if minus || plus {
            let n = (secs + if plus { 5 } else { -5 }).clamp(5, 120);
            self.settings.auto_close_interval = n as f64;
            self.save();
        }
        y += 38.0;

        // Screen.
        lab(g, y, "Island lives on");
        let mut px = cx;
        for (id, label) in [("primary", "Main display"), ("cursor", "Display under the cursor")] {
            let pw = text::measure(label, Face::Regular, 11.5) + 22.0;
            let r = Rect::new(px, y - 2.0, pw, 28.0);
            let (clicked, hover, _) = self.ui.click_region(id_of(id, 79), r);
            let on = self.settings.screen == id;
            g.fill_style(if on { hex("#252830") } else { rgba(255, 255, 255, if hover { 0.1 } else { 0.05 }) });
            g.fill_round_rect(r.x, r.y, r.w, r.h, 14.0);
            text::draw(g, label, r.cx(), r.cy(), Face::Regular, 11.5, hex(if on { pal::INK } else { pal::DIM2 }), Align::Center);
            px += pw + 6.0;
            if clicked && !on {
                self.settings.screen = id.to_string();
                self.save();
            }
        }
        y += 38.0;

        // Autostart.
        lab(g, y, "Launch at startup");
        let on = self.settings.autostart;
        if self.ui.switch(g, "set-auto", cx, y + 2.0, on) {
            self.settings.autostart = !on;
            settings::set_autostart(!on);
            self.save();
        }
        y += 38.0;

        // Docked mode.
        lab(g, y, "Stay on screen");
        let on = self.settings.stay_visible;
        if self.ui.switch(g, "set-stay", cx, y + 2.0, on) {
            self.settings.stay_visible = !on;
            self.save();
        }
        text::draw(g, "keep the compact bar visible when idle", cx + 48.0, y + 11.0, Face::Regular, 11.5, hex(pal::DIM3), Align::Left);
        let _ = iw;
        y0 + h + 2.0 * pad + 16.0
    }
}

// ── Window procedure ──────────────────────────────────────────────────────────

fn lo(l: LPARAM) -> i32 {
    (l.0 & 0xffff) as i16 as i32
}
fn hi(l: LPARAM) -> i32 {
    ((l.0 >> 16) & 0xffff) as i16 as i32
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            BeginPaint(hwnd, &mut ps);
            with_win(|w| w.paint());
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_SIZE => {
            with_win(|w| {
                w.resize();
                w.invalidate();
            });
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            with_win(|w| {
                w.ui.input.mouse = w.mouse_content(lo(lp), hi(lp));
                w.ui.input.inside = true;
                w.invalidate();
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let _ = SetFocus(Some(hwnd));
            SetCapture(hwnd);
            with_win(|w| {
                w.ui.input.mouse = w.mouse_content(lo(lp), hi(lp));
                w.ui.input.inside = true;
                w.ui.input.down = true;
                w.ui.input.pressed = true;
                // Clicking away from every field drops the focus.
                w.focused = None;
                for f in w.fields.values_mut() {
                    f.focused = false;
                }
                w.paint();
                w.invalidate();
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            with_win(|w| {
                w.ui.input.mouse = w.mouse_content(lo(lp), hi(lp));
                w.ui.input.down = false;
                w.ui.input.released = true;
                w.paint();
                w.invalidate();
            });
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let d = ((wp.0 >> 16) & 0xffff) as i16 as f32 / 120.0;
            with_win(|w| {
                w.scroll = (w.scroll - d * 48.0).max(0.0);
                w.invalidate();
            });
            LRESULT(0)
        }
        WM_CHAR => {
            if let Some(c) = char::from_u32(wp.0 as u32) {
                with_win(|w| {
                    if let Some(k) = w.focused {
                        if let Some(f) = w.fields.get_mut(k) {
                            f.on_char(c);
                        }
                        w.invalidate();
                    }
                });
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wp.0 as u32;
            let handled = with_win(|w| {
                let Some(k) = w.focused else { return false };
                let Some(f) = w.fields.get_mut(k) else { return false };
                let h = f.on_key(vk);
                if f.take_submit() {
                    w.save_secret(k);
                }
                w.invalidate();
                h
            })
            .unwrap_or(false);
            if handled { LRESULT(0) } else { DefWindowProcW(hwnd, msg, wp, lp) }
        }
        WM_TIMER => {
            with_win(|w| w.invalidate());
            LRESULT(0)
        }
        WM_CLOSE => {
            with_win(|w| {
                w.set_timer(false);
                w.mode = HooksMode::Normal;
            });
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

#[allow(dead_code)]
fn _unused(_: PCWSTR) {}
