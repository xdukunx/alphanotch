// A tiny immediate-mode toolkit: every frame the views draw themselves and ask
// the widgets whether they were clicked. There is no widget tree to keep in sync
// with the state, which is most of what the DOM version spent its lines on.

use tiny_skia::Color;

use crate::anim::lerp;
use crate::gfx::{hex, rgba, with_alpha, Gfx};
use crate::icons::{self, Icon};
use crate::layout::{wash_rgba, Wash};
use crate::text::{self, Align, Face};

// ── Palette (copied from style.css) ───────────────────────────────────────────

pub mod pal {
    pub const INK: &str = "#F5F6F8";
    pub const INK2: &str = "#F1F2F4";
    pub const DIM: &str = "#9398A1";
    pub const DIM2: &str = "#8E939C";
    pub const DIM3: &str = "#6B7079";
    pub const DIM4: &str = "#5F646D";
    pub const CARD: &str = "#141518";
    pub const CARD_FLAT: &str = "#0E0F11";
    pub const TAB_ON: &str = "#1D1F23";
    pub const RED: &str = "#F4505E";
    pub const RED_TEXT: &str = "#FF8D97";
    pub const GREEN: &str = "#22C55E";
    pub const GREEN2: &str = "#34D399";
    pub const AMBER: &str = "#F5A524";
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
    pub fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(self.x + d, self.y + d, (self.w - 2.0 * d).max(0.0), (self.h - 2.0 * d).max(0.0))
    }
}

/// Mouse state for one frame. Edges (`pressed`, `released`) are cleared after
/// every frame so a click is seen exactly once.
#[derive(Default, Clone, Copy)]
pub struct Input {
    /// Island-local logical coordinates.
    pub mouse: (f32, f32),
    pub inside: bool,
    pub down: bool,
    pub pressed: bool,
    pub released: bool,
    pub wheel: f32,
}

#[derive(Default)]
pub struct Ui {
    pub input: Input,
    /// Widget that received the mouse-down and is waiting for the release.
    pub active: Option<u64>,
    /// Anything under the cursor this frame (used to pick the cursor shape).
    pub hovering_button: bool,
    pub hovering_text: bool,
}

pub fn id_of(label: &str, salt: u64) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ salt;
    for b in label.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

impl Ui {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin_frame(&mut self) {
        self.hovering_button = false;
        self.hovering_text = false;
    }

    pub fn end_frame(&mut self) {
        self.input.pressed = false;
        self.input.released = false;
        self.input.wheel = 0.0;
        if !self.input.down {
            self.active = None;
        }
    }

    fn hover(&self, r: Rect) -> bool {
        self.input.inside && r.contains(self.input.mouse.0, self.input.mouse.1)
    }

    /// Registers a clickable region. Returns true on the frame the click lands.
    pub fn click_region(&mut self, id: u64, r: Rect) -> (bool, bool, bool) {
        let hover = self.hover(r);
        if hover {
            self.hovering_button = true;
        }
        if self.input.pressed && hover {
            self.active = Some(id);
        }
        let pressed = self.active == Some(id) && self.input.down;
        let clicked = self.input.released && hover && self.active == Some(id);
        (clicked, hover, pressed)
    }

    // ── Buttons ───────────────────────────────────────────────────────────────

    /// Pill button (`.btn`). Returns (clicked, width).
    pub fn button(&mut self, g: &mut Gfx, x: f32, y: f32, label: &str, primary: bool, kbd: Option<&str>) -> (bool, f32) {
        let size = 12.5;
        let face = Face::Medium;
        let tw = text::measure(label, face, size);
        let kw = kbd.map(|k| text::measure(k, Face::Regular, 10.5) + 8.0).unwrap_or(0.0);
        let w = 13.0 * 2.0 + tw + if kbd.is_some() { 7.0 + kw } else { 0.0 };
        let h = 30.0;
        let r = Rect::new(x, y, w, h);
        let (clicked, hover, pressed) = self.click_region(id_of(label, 7), r);

        g.save();
        let k = if pressed { 0.94 } else { 1.0 };
        g.translate(r.cx(), r.cy());
        g.scale_xy(k, k);
        g.translate(-r.cx(), -r.cy());

        let (bg, fg) = if primary {
            (hex(pal::INK), hex("#0B0C0E"))
        } else {
            (rgba(255, 255, 255, if hover { 0.15 } else { 0.09 }), hex(pal::INK2))
        };
        g.fill_style(bg);
        g.fill_round_rect(x, y, w, h, h / 2.0);
        text::draw(g, label, x + 13.0, y + h / 2.0, face, size, fg, Align::Left);
        if let Some(k) = kbd {
            let kx = x + 13.0 + tw + 7.0;
            let kr = Rect::new(kx, y + h / 2.0 - 8.0, kw, 16.0);
            g.stroke_style(with_alpha(if primary { rgba(0, 0, 0, 0.4) } else { rgba(255, 255, 255, 0.4) }, 0.55));
            g.line_width(1.0);
            g.round_rect(kr.x, kr.y, kr.w, kr.h, 4.0);
            g.stroke();
            text::draw(g, k, kr.cx(), kr.cy(), Face::Regular, 10.5, with_alpha(fg, 0.55), Align::Center);
        }
        g.restore();
        (clicked, w)
    }

    /// Text-only button (`.link-btn`). Returns (clicked, width).
    pub fn link(&mut self, g: &mut Gfx, x: f32, y: f32, label: &str, color: Color) -> (bool, f32) {
        let w = text::measure(label, Face::Medium, 11.0);
        let r = Rect::new(x - 2.0, y - 9.0, w + 4.0, 18.0);
        let (clicked, hover, _) = self.click_region(id_of(label, 11), r);
        text::draw(g, label, x, y, Face::Medium, 11.0, if hover { with_alpha(color, 1.0) } else { with_alpha(color, 0.92) }, Align::Left);
        if hover {
            g.fill_style(with_alpha(color, 0.6));
            g.fill_rect(x, y + 7.0, w, 1.0);
        }
        (clicked, w)
    }

    /// Round icon button (`.icon-btn`, 16 px).
    pub fn icon_button(&mut self, g: &mut Gfx, id: &str, cx: f32, cy: f32, d: f32, icon: Icon, size: f32) -> bool {
        let r = Rect::new(cx - d / 2.0, cy - d / 2.0, d, d);
        let (clicked, hover, _) = self.click_region(id_of(id, 3), r);
        g.fill_style(rgba(255, 255, 255, if hover { 0.14 } else { 0.07 }));
        g.circle(cx, cy, d / 2.0);
        g.fill();
        icons::fill(g, icon, cx, cy, size, hex(if hover { pal::DIM } else { pal::DIM4 }));
        clicked
    }

    /// Header icon (tab or action) — no background unless `on`/hovered.
    pub fn header_icon(&mut self, g: &mut Gfx, id: &str, r: Rect, icon: Icon, size: f32, on: bool, pill: bool) -> bool {
        let (clicked, hover, _) = self.click_region(id_of(id, 5), r);
        if pill && (on || hover) {
            g.fill_style(if on { hex(pal::TAB_ON) } else { rgba(255, 255, 255, 0.07) });
            g.fill_round_rect(r.x, r.y, r.w, r.h, r.h / 2.0);
        }
        let col = if on { hex(pal::INK) } else if hover { hex("#B0B5BE") } else { hex(pal::DIM2) };
        icons::fill(g, icon, r.cx(), r.cy(), size, col);
        clicked
    }

    /// 32×18 switch. Returns true when toggled.
    pub fn switch(&mut self, g: &mut Gfx, id: &str, x: f32, y: f32, on: bool) -> bool {
        let r = Rect::new(x, y, 32.0, 18.0);
        let (clicked, _, _) = self.click_region(id_of(id, 13), r);
        g.fill_style(if on { hex(pal::GREEN) } else { rgba(255, 255, 255, 0.15) });
        g.fill_round_rect(x, y, 32.0, 18.0, 9.0);
        g.fill_style(Color::WHITE);
        g.circle(x + 9.0 + if on { 14.0 } else { 0.0 }, y + 9.0, 7.0);
        g.fill();
        clicked
    }
}

// ── Static drawing helpers ────────────────────────────────────────────────────

pub fn dot(g: &mut Gfx, cx: f32, cy: f32, d: f32, color: Color) {
    g.fill_style(color);
    g.circle(cx, cy, d / 2.0);
    g.fill();
}

/// `.card`: rounded panel with an optional glow rising from below (the wash).
pub fn card(g: &mut Gfx, r: Rect, wash: Wash, flat: bool) {
    g.fill_style(hex(if flat { pal::CARD_FLAT } else { pal::CARD }));
    g.fill_round_rect(r.x, r.y, r.w, r.h, 20.0);
    g.stroke_style(rgba(255, 255, 255, 0.035));
    g.line_width(1.0);
    g.round_rect(r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0, 19.5);
    g.stroke();
    if wash != Wash::None {
        let (cr, cg, cb, ca) = wash_rgba(wash);
        glow(g, r, 50.0, 130.0, 280.0, tiny_skia::Color::from_rgba8(cr, cg, cb, (ca * 255.0) as u8));
    }
}

/// Radial glow centred at (`px`%, `py`%) of `r`, confined to the card's shape by the
/// fill path itself. It is static, so it is rendered once into a cached layer.
pub fn glow(g: &mut Gfx, r: Rect, px: f32, py: f32, radius: f32, c: Color) {
    let key = crate::gfx::layer_key(&[r.w, r.h, px, py, radius, c.red(), c.green(), c.blue(), c.alpha(), 1.0]);
    g.cached_layer(key, r.w, r.h, r.x, r.y, |l| {
        let local = Rect::new(0.0, 0.0, r.w, r.h);
        let cx = local.w * px / 100.0;
        let cy = local.h * py / 100.0;
        let grad = l.radial(cx, cy, 0.0, radius, &[(0.0, c), (0.7, with_alpha(c, 0.0)), (1.0, with_alpha(c, 0.0))]);
        l.fill_style(grad);
        l.fill_round_rect(0.0, 0.0, local.w, local.h, 20.0);
    });
}

pub fn lighten(hexs: &str, amount: f32) -> Color {
    let c = hex(hexs);
    Color::from_rgba(
        lerp(c.red(), 1.0, amount).min(1.0),
        lerp(c.green(), 1.0, amount).min(1.0),
        lerp(c.blue(), 1.0, amount).min(1.0),
        c.alpha(),
    )
    .unwrap_or(c)
}
