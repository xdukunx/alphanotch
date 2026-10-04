// A single-line text field: caret, selection, clipboard, password masking.
// Typed text arrives as WM_CHAR (so IME-composed text works), editing keys as
// WM_KEYDOWN.

use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_BACK, VK_DELETE, VK_END, VK_HOME, VK_LEFT, VK_RETURN, VK_RIGHT,
};

use crate::anim::now;
use crate::gfx::{hex, rgba, Gfx};
use crate::platform::{ctrl_down, shift_down};
use crate::text::{self, Align, Face};
use crate::ui::{pal, Rect};

const CF_UNICODETEXT: u32 = 13;

pub struct TextField {
    chars: Vec<char>,
    caret: usize,
    anchor: Option<usize>,
    scroll: f32,
    pub masked: bool,
    pub placeholder: String,
    pub focused: bool,
    blink_from: f32,
    /// Set when Enter was pressed; the owner takes it with `take_submit`.
    submit: bool,
    last_rect: Rect,
}

impl TextField {
    pub fn new(placeholder: &str, masked: bool) -> Self {
        Self {
            chars: Vec::new(),
            caret: 0,
            anchor: None,
            scroll: 0.0,
            masked,
            placeholder: placeholder.to_string(),
            focused: false,
            blink_from: 0.0,
            submit: false,
            last_rect: Rect::default(),
        }
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    pub fn clear(&mut self) {
        self.chars.clear();
        self.caret = 0;
        self.anchor = None;
        self.scroll = 0.0;
    }

    #[allow(dead_code)]
    pub fn set_text(&mut self, s: &str) {
        self.chars = s.chars().collect();
        self.caret = self.chars.len();
        self.anchor = None;
    }

    pub fn take_submit(&mut self) -> bool {
        std::mem::take(&mut self.submit)
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.chars.len();
    }

    fn touch(&mut self) {
        self.blink_from = now();
    }

    fn selection(&self) -> Option<(usize, usize)> {
        self.anchor.filter(|a| *a != self.caret).map(|a| (a.min(self.caret), a.max(self.caret)))
    }

    fn delete_selection(&mut self) -> bool {
        if let Some((a, b)) = self.selection() {
            self.chars.drain(a..b);
            self.caret = a;
            self.anchor = None;
            true
        } else {
            self.anchor = None;
            false
        }
    }

    pub fn insert_str(&mut self, s: &str) {
        self.delete_selection();
        for c in s.chars().filter(|c| !c.is_control()) {
            self.chars.insert(self.caret, c);
            self.caret += 1;
        }
        self.touch();
    }

    pub fn on_char(&mut self, c: char) {
        if c.is_control() {
            return;
        }
        self.insert_str(&c.to_string());
    }

    /// Returns true when the key was an editing key and has been consumed.
    pub fn on_key(&mut self, vk: u32) -> bool {
        let ctrl = ctrl_down();
        let shift = shift_down();
        self.touch();
        let mv = |me: &mut Self, to: usize| {
            if shift {
                if me.anchor.is_none() {
                    me.anchor = Some(me.caret);
                }
            } else {
                me.anchor = None;
            }
            me.caret = to;
        };
        match vk {
            v if v == VK_BACK.0 as u32 => {
                if !self.delete_selection() && self.caret > 0 {
                    self.caret -= 1;
                    self.chars.remove(self.caret);
                }
                true
            }
            v if v == VK_DELETE.0 as u32 => {
                if !self.delete_selection() && self.caret < self.chars.len() {
                    self.chars.remove(self.caret);
                }
                true
            }
            v if v == VK_LEFT.0 as u32 => {
                let to = self.caret.saturating_sub(1);
                if !shift && self.selection().is_some() {
                    let (a, _) = self.selection().unwrap();
                    self.anchor = None;
                    self.caret = a;
                } else {
                    mv(self, to);
                }
                true
            }
            v if v == VK_RIGHT.0 as u32 => {
                let to = (self.caret + 1).min(self.chars.len());
                if !shift && self.selection().is_some() {
                    let (_, b) = self.selection().unwrap();
                    self.anchor = None;
                    self.caret = b;
                } else {
                    mv(self, to);
                }
                true
            }
            v if v == VK_HOME.0 as u32 => {
                mv(self, 0);
                true
            }
            v if v == VK_END.0 as u32 => {
                let n = self.chars.len();
                mv(self, n);
                true
            }
            v if v == VK_RETURN.0 as u32 => {
                self.submit = true;
                true
            }
            0x41 if ctrl => {
                self.select_all();
                true
            }
            0x43 if ctrl => {
                self.copy();
                true
            }
            0x58 if ctrl => {
                self.copy();
                self.delete_selection();
                true
            }
            0x56 if ctrl => {
                if let Some(t) = clipboard_text() {
                    self.insert_str(&t.replace(['\r', '\n'], " "));
                }
                true
            }
            _ => false,
        }
    }

    fn copy(&self) {
        if self.masked {
            return;
        }
        if let Some((a, b)) = self.selection() {
            set_clipboard_text(&self.chars[a..b].iter().collect::<String>());
        }
    }

    fn shown(&self) -> String {
        if self.masked { "•".repeat(self.chars.len()) } else { self.text() }
    }

    fn x_of(&self, idx: usize, size: f32) -> f32 {
        let s = self.shown();
        let upto: String = s.chars().take(idx).collect();
        text::measure(&upto, Face::Regular, size)
    }

    /// Places the caret from a click at `x` (field-local, scroll included).
    pub fn click_at(&mut self, mx: f32, size: f32, extend: bool) {
        let target = mx - self.last_rect.x + self.scroll;
        let mut best = 0;
        let mut best_d = f32::MAX;
        for i in 0..=self.chars.len() {
            let d = (self.x_of(i, size) - target).abs();
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.caret);
            }
        } else {
            self.anchor = None;
        }
        self.caret = best;
        self.touch();
    }

    #[allow(dead_code)]
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.last_rect.contains(x, y)
    }

    pub fn draw(&mut self, g: &mut Gfx, r: Rect, size: f32, color: &str) {
        self.last_rect = r;
        let cy = r.cy();
        g.save();
        g.clip_round_rect(r.x, r.y - 4.0, r.w, r.h + 8.0, 4.0);

        // Keep the caret visible.
        let cx = self.x_of(self.caret, size);
        if cx - self.scroll > r.w - 4.0 {
            self.scroll = cx - r.w + 4.0;
        }
        if cx - self.scroll < 0.0 {
            self.scroll = cx;
        }
        if self.x_of(self.chars.len(), size) <= r.w {
            self.scroll = 0.0;
        }

        if let Some((a, b)) = self.selection() {
            g.fill_style(rgba(99, 140, 255, 0.45));
            let (xa, xb) = (self.x_of(a, size), self.x_of(b, size));
            g.fill_rect(r.x + xa - self.scroll, cy - size * 0.75, xb - xa, size * 1.5);
        }

        if self.chars.is_empty() && !self.focused {
            text::draw(g, &self.placeholder, r.x, cy, Face::Regular, size, hex(pal::DIM3), Align::Left);
        } else if self.chars.is_empty() {
            text::draw(g, &self.placeholder, r.x, cy, Face::Regular, size, hex(pal::DIM3), Align::Left);
        } else {
            text::draw(g, &self.shown(), r.x - self.scroll, cy, Face::Regular, size, hex(color), Align::Left);
        }

        if self.focused && ((now() - self.blink_from) * 1.9) as i32 % 2 == 0 {
            g.fill_style(hex(color));
            g.fill_rect(r.x + cx - self.scroll, cy - size * 0.7, 1.2, size * 1.4);
        }
        g.restore();
    }
}

fn clipboard_text() -> Option<String> {
    unsafe {
        OpenClipboard(None).ok()?;
        let out = (|| {
            let h = GetClipboardData(CF_UNICODETEXT).ok()?;
            let ptr = GlobalLock(HGLOBAL(h.0)) as *const u16;
            if ptr.is_null() {
                return None;
            }
            let mut n = 0;
            while *ptr.add(n) != 0 {
                n += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, n));
            let _ = GlobalUnlock(HGLOBAL(h.0));
            Some(s)
        })();
        let _ = CloseClipboard();
        out
    }
}

fn set_clipboard_text(s: &str) {
    unsafe {
        if OpenClipboard(None).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        if let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
            let ptr = GlobalLock(h) as *mut u16;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
                let _ = GlobalUnlock(h);
                let _ = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(h.0)));
            }
        }
        let _ = CloseClipboard();
    }
}
