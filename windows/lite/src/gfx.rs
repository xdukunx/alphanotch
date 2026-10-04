// A small Canvas-2D-flavoured drawing context over tiny-skia.
//
// The character, greeting and drop sequence were written against the browser's
// Canvas 2D. Keeping the same vocabulary here (save/restore, translate/rotate/
// scale, path building, fill/stroke/clip, linear and radial gradients) lets that
// code move over nearly line for line, instead of being re-derived.

use std::f32::consts::{PI, TAU};
use std::rc::Rc;

use tiny_skia::{
    Color, FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Mask, Paint, Path,
    PathBuilder, Pixmap, Point, RadialGradient, Shader, SpreadMode, Stroke, Transform,
};

pub fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba8(r, g, b, 255)
}

pub fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, (a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// `#RRGGBB` or `#RRGGBBAA`.
pub fn hex(s: &str) -> Color {
    let h = s.trim_start_matches('#');
    let v = u32::from_str_radix(h, 16).unwrap_or(0);
    if h.len() == 8 {
        Color::from_rgba8((v >> 24) as u8, (v >> 16) as u8, (v >> 8) as u8, v as u8)
    } else {
        Color::from_rgba8((v >> 16) as u8, (v >> 8) as u8, v as u8, 255)
    }
}

/// Colour with 0…1 float components, like the engine's `RGB`.
pub fn frgb(c: [f32; 3], a: f32) -> Color {
    Color::from_rgba(c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0), a.clamp(0.0, 1.0))
        .unwrap_or(Color::BLACK)
}

pub fn with_alpha(c: Color, a: f32) -> Color {
    Color::from_rgba(c.red(), c.green(), c.blue(), (c.alpha() * a).clamp(0.0, 1.0)).unwrap_or(c)
}

#[derive(Clone)]
pub enum Style {
    Solid(Color),
    Shader(Shader<'static>),
}

impl From<Color> for Style {
    fn from(c: Color) -> Self {
        Style::Solid(c)
    }
}

impl Style {
    fn paint(&self, alpha: f32) -> Paint<'static> {
        let mut p = Paint::default();
        p.anti_alias = true;
        match self {
            Style::Solid(c) => p.set_color(with_alpha(*c, alpha)),
            Style::Shader(s) => {
                let mut s = s.clone();
                if alpha < 0.999 {
                    s.apply_opacity(alpha);
                }
                p.shader = s;
            }
        }
        p
    }
}

#[derive(Clone)]
struct State {
    tf: Transform,
    clip: Option<Rc<Mask>>,
    alpha: f32,
    fill: Style,
    stroke: Style,
    line_width: f32,
    cap: LineCap,
    join: LineJoin,
}

pub struct Gfx {
    pub pm: Pixmap,
    st: State,
    stack: Vec<State>,
    path: PathBuilder,
    cur: Option<Point>,
    start: Option<Point>,
    base: Transform,
}

impl Gfx {
    /// `w`×`h` device pixels; `scale` maps the logical coordinates drawn into them.
    pub fn new(w: u32, h: u32, scale: f32) -> Self {
        let base = Transform::from_scale(scale, scale);
        Self {
            pm: Pixmap::new(w.max(1), h.max(1)).expect("pixmap"),
            st: State {
                tf: base,
                clip: None,
                alpha: 1.0,
                fill: Style::Solid(Color::BLACK),
                stroke: Style::Solid(Color::BLACK),
                line_width: 1.0,
                cap: LineCap::Butt,
                join: LineJoin::Miter,
            },
            stack: Vec::new(),
            path: PathBuilder::new(),
            cur: None,
            start: None,
            base,
        }
    }

    pub fn width(&self) -> u32 {
        self.pm.width()
    }
    pub fn height(&self) -> u32 {
        self.pm.height()
    }
    pub fn scale(&self) -> f32 {
        self.base.sx
    }

    pub fn clear(&mut self) {
        self.pm.fill(Color::TRANSPARENT);
        self.stack.clear();
        self.st.tf = self.base;
        self.st.clip = None;
        self.st.alpha = 1.0;
    }

    // ── state ────────────────────────────────────────────────────────────────

    pub fn save(&mut self) {
        self.stack.push(self.st.clone());
    }

    pub fn restore(&mut self) {
        if let Some(s) = self.stack.pop() {
            self.st = s;
        }
    }

    pub fn translate(&mut self, x: f32, y: f32) {
        self.st.tf = self.st.tf.pre_translate(x, y);
    }
    pub fn rotate(&mut self, a: f32) {
        self.st.tf = self.st.tf.pre_concat(Transform::from_rotate(a.to_degrees()));
    }
    pub fn scale_xy(&mut self, x: f32, y: f32) {
        self.st.tf = self.st.tf.pre_scale(x, y);
    }
    pub fn transform(&self) -> Transform {
        self.st.tf
    }

    pub fn set_alpha(&mut self, a: f32) {
        self.st.alpha = a.clamp(0.0, 1.0);
    }
    /// Multiplies the global alpha (Canvas `globalAlpha *= a`).
    pub fn mul_alpha(&mut self, a: f32) {
        self.st.alpha = (self.st.alpha * a).clamp(0.0, 1.0);
    }
    pub fn alpha(&self) -> f32 {
        self.st.alpha
    }
    pub fn fill_style(&mut self, s: impl Into<Style>) {
        self.st.fill = s.into();
    }
    pub fn stroke_style(&mut self, s: impl Into<Style>) {
        self.st.stroke = s.into();
    }
    pub fn line_width(&mut self, w: f32) {
        self.st.line_width = w;
    }
    pub fn line_cap_round(&mut self) {
        self.st.cap = LineCap::Round;
    }
    pub fn line_join_round(&mut self) {
        self.st.join = LineJoin::Round;
    }

    // ── gradients ────────────────────────────────────────────────────────────

    fn stops(stops: &[(f32, Color)]) -> Vec<GradientStop> {
        stops.iter().map(|(o, c)| GradientStop::new(o.clamp(0.0, 1.0), *c)).collect()
    }

    /// Gradient positions are in the *current* user space, like Canvas.
    pub fn linear(&self, x0: f32, y0: f32, x1: f32, y1: f32, stops: &[(f32, Color)]) -> Style {
        LinearGradient::new(
            Point::from_xy(x0, y0),
            Point::from_xy(x1, y1),
            Self::stops(stops),
            SpreadMode::Pad,
            self.st.tf,
        )
        .map(Style::Shader)
        .unwrap_or_else(|| Style::Solid(stops.first().map(|s| s.1).unwrap_or(Color::BLACK)))
    }

    /// Concentric radial gradient (`createRadialGradient(cx,cy,r0, cx,cy,r1)`).
    pub fn radial(&self, cx: f32, cy: f32, r0: f32, r1: f32, stops: &[(f32, Color)]) -> Style {
        let first = stops.first().map(|s| s.1).unwrap_or(Color::BLACK);
        if r1 <= 0.0 {
            return Style::Solid(first);
        }
        let k = (r0 / r1).clamp(0.0, 0.999);
        let mut mapped: Vec<(f32, Color)> = Vec::with_capacity(stops.len() + 1);
        if k > 0.0 {
            mapped.push((0.0, first));
        }
        for (o, c) in stops {
            mapped.push((k + o * (1.0 - k), *c));
        }
        RadialGradient::new(
            Point::from_xy(cx, cy),
            Point::from_xy(cx, cy),
            r1,
            Self::stops(&mapped),
            SpreadMode::Pad,
            self.st.tf,
        )
        .map(Style::Shader)
        .unwrap_or(Style::Solid(first))
    }

    // ── path building ────────────────────────────────────────────────────────

    pub fn begin_path(&mut self) {
        self.path = PathBuilder::new();
        self.cur = None;
        self.start = None;
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(x, y);
        self.cur = Some(Point::from_xy(x, y));
        self.start = self.cur;
    }

    pub fn line_to(&mut self, x: f32, y: f32) {
        if self.cur.is_none() {
            self.move_to(x, y);
        } else {
            self.path.line_to(x, y);
            self.cur = Some(Point::from_xy(x, y));
        }
    }

    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        if self.cur.is_none() {
            self.move_to(cx, cy);
        }
        self.path.quad_to(cx, cy, x, y);
        self.cur = Some(Point::from_xy(x, y));
    }

    pub fn cubic_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        if self.cur.is_none() {
            self.move_to(c1x, c1y);
        }
        self.path.cubic_to(c1x, c1y, c2x, c2y, x, y);
        self.cur = Some(Point::from_xy(x, y));
    }

    pub fn close_path(&mut self) {
        if self.cur.is_some() {
            self.path.close();
            self.cur = self.start;
        }
    }

    /// Canvas `ellipse()` with the same angle convention (clockwise on screen).
    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, rot: f32, a0: f32, a1: f32, ccw: bool) {
        let (a0, mut a1) = (a0, a1);
        if !ccw && a1 < a0 {
            a1 += ((a0 - a1) / TAU).ceil() * TAU;
        }
        if ccw && a1 > a0 {
            a1 -= ((a1 - a0) / TAU).ceil() * TAU;
        }
        if !ccw && a1 - a0 > TAU {
            a1 = a0 + TAU;
        }
        if ccw && a0 - a1 > TAU {
            a1 = a0 - TAU;
        }
        let (sr, cr) = rot.sin_cos();
        let pt = |a: f32| -> (f32, f32) {
            let (s, c) = a.sin_cos();
            let (px, py) = (rx * c, ry * s);
            (cx + px * cr - py * sr, cy + px * sr + py * cr)
        };
        let (sx, sy) = pt(a0);
        self.line_to(sx, sy);
        let total = a1 - a0;
        let n = (total.abs() / (PI / 2.0)).ceil().max(1.0) as i32;
        let da = total / n as f32;
        let k = 4.0 / 3.0 * (da / 4.0).tan();
        for i in 0..n {
            let t0 = a0 + da * i as f32;
            let t1 = t0 + da;
            let (s0, c0) = t0.sin_cos();
            let (s1, c1) = t1.sin_cos();
            let rotp = |px: f32, py: f32| -> (f32, f32) {
                (cx + px * cr - py * sr, cy + px * sr + py * cr)
            };
            let p1 = rotp(rx * (c0 - k * s0), ry * (s0 + k * c0));
            let p2 = rotp(rx * (c1 + k * s1), ry * (s1 - k * c1));
            let p3 = rotp(rx * c1, ry * s1);
            self.cubic_to(p1.0, p1.1, p2.0, p2.1, p3.0, p3.1);
        }
    }

    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32, ccw: bool) {
        self.ellipse(cx, cy, r, r, 0.0, a0, a1, ccw);
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32) {
        self.begin_path();
        self.arc(cx, cy, r, 0.0, TAU, false);
        self.close_path();
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.move_to(x, y);
        self.line_to(x + w, y);
        self.line_to(x + w, y + h);
        self.line_to(x, y + h);
        self.close_path();
    }

    /// Rounded rectangle with independent corner radii (tl, tr, br, bl).
    pub fn round_rect4(&mut self, x: f32, y: f32, w: f32, h: f32, r: [f32; 4]) {
        let m = (w / 2.0).min(h / 2.0).max(0.0);
        let r = r.map(|v| v.clamp(0.0, m));
        let k = 0.552_284_75;
        self.move_to(x + r[0], y);
        self.line_to(x + w - r[1], y);
        self.cubic_to(x + w - r[1] * (1.0 - k), y, x + w, y + r[1] * (1.0 - k), x + w, y + r[1]);
        self.line_to(x + w, y + h - r[2]);
        self.cubic_to(x + w, y + h - r[2] * (1.0 - k), x + w - r[2] * (1.0 - k), y + h, x + w - r[2], y + h);
        self.line_to(x + r[3], y + h);
        self.cubic_to(x + r[3] * (1.0 - k), y + h, x, y + h - r[3] * (1.0 - k), x, y + h - r[3]);
        self.line_to(x, y + r[0]);
        self.cubic_to(x, y + r[0] * (1.0 - k), x + r[0] * (1.0 - k), y, x + r[0], y);
        self.close_path();
    }

    pub fn round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) {
        self.begin_path();
        self.round_rect4(x, y, w, h, [r; 4]);
    }

    /// Takes the path built so far, leaving the builder empty.
    pub fn take_path(&mut self) -> Option<Path> {
        let b = std::mem::replace(&mut self.path, PathBuilder::new());
        self.cur = None;
        self.start = None;
        b.finish()
    }

    // ── painting ─────────────────────────────────────────────────────────────

    pub fn fill(&mut self) {
        if let Some(p) = self.take_path() {
            self.fill_path(&p);
        }
    }

    pub fn fill_path(&mut self, p: &Path) {
        let paint = self.st.fill.paint(self.st.alpha);
        self.pm.fill_path(p, &paint, FillRule::Winding, self.st.tf, self.st.clip.as_deref());
    }

    pub fn stroke(&mut self) {
        if let Some(p) = self.take_path() {
            self.stroke_path(&p);
        }
    }

    pub fn stroke_path(&mut self, p: &Path) {
        let paint = self.st.stroke.paint(self.st.alpha);
        let stroke = Stroke {
            width: self.st.line_width,
            line_cap: self.st.cap,
            line_join: self.st.join,
            ..Stroke::default()
        };
        self.pm.stroke_path(p, &paint, &stroke, self.st.tf, self.st.clip.as_deref());
    }

    pub fn stroke_path_with(&mut self, p: &Path, stroke: &Stroke) {
        let paint = self.st.stroke.paint(self.st.alpha);
        self.pm.stroke_path(p, &paint, stroke, self.st.tf, self.st.clip.as_deref());
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.begin_path();
        self.rect(x, y, w, h);
        self.fill();
    }

    /// Fills a rounded rect with a square premultiplied-RGBA image (`side` px) stretched over it.
    pub fn fill_image_round_rect(&mut self, rgba: &[u8], side: u32, x: f32, y: f32, w: f32, h: f32, r: f32) {
        let Some(img) = tiny_skia::PixmapRef::from_bytes(rgba, side, side) else { return };
        let Some(path) = rounded_rect_path(x, y, w, h, r) else { return };
        let (sx, sy) = (w / side as f32, h / side as f32);
        let pattern = tiny_skia::Pattern::new(
            img,
            SpreadMode::Pad,
            tiny_skia::FilterQuality::Bilinear,
            self.st.alpha,
            Transform::from_row(sx, 0.0, 0.0, sy, x, y),
        );
        let paint = Paint { shader: pattern, anti_alias: true, ..Paint::default() };
        self.pm.fill_path(&path, &paint, FillRule::Winding, self.st.tf, self.st.clip.as_deref());
    }

    pub fn fill_round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) {
        self.round_rect(x, y, w, h, r);
        self.fill();
    }

    /// Intersects the clip with `p` (in the current transform).
    pub fn clip_path(&mut self, p: &Path) {
        let (w, h) = (self.pm.width(), self.pm.height());
        let mask = match self.st.clip.as_deref() {
            Some(m) => {
                let mut m = m.clone();
                m.intersect_path(p, FillRule::Winding, true, self.st.tf);
                m
            }
            None => {
                let Some(mut m) = Mask::new(w, h) else { return };
                m.fill_path(p, FillRule::Winding, true, self.st.tf);
                m
            }
        };
        self.st.clip = Some(Rc::new(mask));
    }

    pub fn clip(&mut self) {
        if let Some(p) = self.take_path() {
            self.clip_path(&p);
        }
    }

    pub fn clip_rect_xywh(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.begin_path();
        self.rect(x, y, w, h);
        self.clip();
    }

    pub fn clip_round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) {
        self.round_rect(x, y, w, h, r);
        self.clip();
    }

    /// Pixel buffer plus clip mask, borrowed together for the text blitter.
    pub(crate) fn pixels_and_mask(&mut self) -> (&mut [u8], u32, u32, Option<&Mask>) {
        let (w, h) = (self.pm.width(), self.pm.height());
        (self.pm.data_mut(), w, h, self.st.clip.as_deref())
    }
}

thread_local! {
    static LAYERS: std::cell::RefCell<std::collections::HashMap<u64, Pixmap>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Hashes anything that identifies a cached layer (sizes, colours, scale…).
pub fn layer_key(parts: &[f32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        h ^= p.to_bits() as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

impl Gfx {
    /// Draws a layer that never changes between frames (a card's glow, say) from a
    /// bitmap cache instead of re-running its gradient. `build` paints the layer into
    /// a scratch canvas of `w`×`h` logical px; `(x, y)` is where it lands.
    pub fn cached_layer(&mut self, key: u64, w: f32, h: f32, x: f32, y: f32, build: impl FnOnce(&mut Gfx)) {
        let scale = self.scale();
        let (pw, ph) = ((w * scale).ceil().max(1.0) as u32, (h * scale).ceil().max(1.0) as u32);
        let key = key ^ ((pw as u64) << 20) ^ ((ph as u64) << 40);
        let have = LAYERS.with(|l| l.borrow().contains_key(&key));
        if !have {
            let mut g2 = Gfx::new(pw, ph, scale);
            build(&mut g2);
            LAYERS.with(|l| {
                let mut l = l.borrow_mut();
                if l.len() > 48 {
                    l.clear();
                }
                l.insert(key, g2.pm);
            });
        }
        let tf = self.st.tf;
        let (dx, dy) = ((x * tf.sx + tf.tx).round() as i32, (y * tf.sy + tf.ty).round() as i32);
        let opacity = (self.st.alpha * 256.0).round() as u32;
        let (dw, dh) = (self.pm.width() as i32, self.pm.height() as i32);
        let mask = self.st.clip.clone();
        LAYERS.with(|l| {
            let l = l.borrow();
            let Some(src) = l.get(&key) else { return };
            let (sw, sh) = (src.width() as i32, src.height() as i32);
            let sdata = src.data();
            let ddata = self.pm.data_mut();
            // Premultiplied src-over by hand: the generic pipeline is ~10× slower on big layers.
            for row in 0..sh {
                let y = dy + row;
                if y < 0 || y >= dh {
                    continue;
                }
                let x0 = (-dx).max(0);
                let x1 = sw.min(dw - dx);
                for col in x0..x1 {
                    let si = ((row * sw + col) * 4) as usize;
                    let sa = sdata[si + 3] as u32;
                    if sa == 0 {
                        continue;
                    }
                    let mut k = opacity;
                    if let Some(m) = &mask {
                        k = k * m.data()[(y * dw + dx + col) as usize] as u32 / 255;
                    }
                    let a = (sa * k) >> 8;
                    let di = ((y * dw + dx + col) * 4) as usize;
                    let inv = 255 - a;
                    for c in 0..3 {
                        let v = (sdata[si + c] as u32 * k >> 8) + ddata[di + c] as u32 * inv / 255;
                        ddata[di + c] = v.min(255) as u8;
                    }
                    ddata[di + 3] = (a + ddata[di + 3] as u32 * inv / 255).min(255) as u8;
                }
            }
        });
    }
}

/// Path helpers that do not need a canvas.
pub fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let mut g = PathBuilder::new();
    let m = (w / 2.0).min(h / 2.0).max(0.0);
    let r = r.clamp(0.0, m);
    let k = 0.552_284_75;
    g.move_to(x + r, y);
    g.line_to(x + w - r, y);
    g.cubic_to(x + w - r * (1.0 - k), y, x + w, y + r * (1.0 - k), x + w, y + r);
    g.line_to(x + w, y + h - r);
    g.cubic_to(x + w, y + h - r * (1.0 - k), x + w - r * (1.0 - k), y + h, x + w - r, y + h);
    g.line_to(x + r, y + h);
    g.cubic_to(x + r * (1.0 - k), y + h, x, y + h - r * (1.0 - k), x, y + h - r);
    g.line_to(x, y + r);
    g.cubic_to(x, y + r * (1.0 - k), x + r * (1.0 - k), y, x + r, y);
    g.close();
    g.finish()
}
