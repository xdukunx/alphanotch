// Icons: the same 24×24 SVG path data the web island used, parsed once into
// tiny-skia paths. A ~100-line SVG path reader is far lighter than an SVG crate.

use std::cell::RefCell;
use std::collections::HashMap;

use tiny_skia::{Color, Path, PathBuilder};

use crate::gfx::Gfx;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Icon {
    House,
    Bubble,
    Plus,
    Gear,
    GearFill,
    SpeakerOn,
    SpeakerOff,
    ArrowUpRight,
    ChevronRight,
    ChevronLeft,
    Check,
    ArrowUp,
    Bang,
    Xmark,
    Timer,
    Ellipsis,
    Star,
    Stack,
    Doc,
}

fn data(i: Icon) -> &'static str {
    match i {
        Icon::House => "M12 3.2 2.8 10.6V21h6.6v-5.4h5.2V21h6.6V10.6L12 3.2z",
        Icon::Bubble => "M12 3.6c-5 0-9 3.3-9 7.4 0 2.3 1.3 4.4 3.3 5.7-.2 1.2-.8 2.4-1.7 3.4 1.9-.2 3.6-.9 4.9-1.9 .8.2 1.6.3 2.5.3 5 0 9-3.3 9-7.5s-4-7.4-9-7.4z",
        Icon::Plus => "M11 4h2v7h7v2h-7v7h-2v-7H4v-2h7V4z",
        Icon::Gear => "M12 8.6a3.4 3.4 0 1 0 0 6.8 3.4 3.4 0 0 0 0-6.8zm0 1.8a1.6 1.6 0 1 1 0 3.2 1.6 1.6 0 0 1 0-3.2zM10.9 2h2.2l.35 2.1c.6.17 1.16.4 1.67.71l1.9-1 1.55 1.55-1 1.9c.3.5.54 1.07.7 1.67l2.13.35v2.2l-2.12.35c-.17.6-.4 1.16-.71 1.67l1 1.9-1.55 1.55-1.9-1c-.5.3-1.07.54-1.67.7L13.1 22h-2.2l-.35-2.12c-.6-.17-1.16-.4-1.67-.71l-1.9 1L5.43 18.6l1-1.9c-.3-.5-.54-1.07-.7-1.67L3.6 14.7v-2.2l2.12-.35c.17-.6.4-1.16.71-1.67l-1-1.9 1.55-1.55 1.9 1c.5-.3 1.07-.54 1.67-.7L10.9 2z",
        Icon::GearFill => "M10.9 2h2.2l.35 2.1c.6.17 1.16.4 1.67.71l1.9-1 1.55 1.55-1 1.9c.3.5.54 1.07.7 1.67l2.13.35v2.2l-2.12.35c-.17.6-.4 1.16-.71 1.67l1 1.9-1.55 1.55-1.9-1c-.5.3-1.07.54-1.67.7L13.1 22h-2.2l-.35-2.12c-.6-.17-1.16-.4-1.67-.71l-1.9 1L5.43 18.6l1-1.9c-.3-.5-.54-1.07-.7-1.67L3.6 14.7v-2.2l2.12-.35c.17-.6.4-1.16.71-1.67l-1-1.9 1.55-1.55 1.9 1c.5-.3 1.07-.54 1.67-.7L10.9 2zM12 8.2a3.8 3.8 0 1 0 0 7.6 3.8 3.8 0 0 0 0-7.6z",
        Icon::SpeakerOn => "M11 4.5 6.5 8.2H3.4v7.6h3.1L11 19.5v-15zm3.2 3a5.3 5.3 0 0 1 0 9 .9.9 0 0 0 .9 1.55 7.1 7.1 0 0 0 0-12.1.9.9 0 0 0-.9 1.55zm2.6-3.1a8.9 8.9 0 0 1 0 15.2.9.9 0 0 0 .92 1.55 10.7 10.7 0 0 0 0-18.3.9.9 0 0 0-.92 1.55z",
        Icon::SpeakerOff => "M11 4.5 6.5 8.2H3.4v7.6h3.1L11 19.5v-15zm3.6 4.1 1.27-1.27 2.33 2.33 2.33-2.33 1.27 1.27L19.47 11l2.33 2.33-1.27 1.27-2.33-2.33-2.33 2.33-1.27-1.27L16.93 11 14.6 8.6z",
        Icon::ArrowUpRight => "M8.5 7h8.5v8.5h-2V10.4l-7.1 7.1-1.4-1.4 7.1-7.1H8.5V7z",
        Icon::ChevronRight => "M9 5.5 15.5 12 9 18.5",
        Icon::ChevronLeft => "M15 5.5 8.5 12 15 18.5",
        Icon::Check => "M5 12.5 9.5 17 19 7.5",
        Icon::ArrowUp => "M12 4.5 5.5 11l1.5 1.5 4-4V19.5h2V8.5l4 4L18.5 11 12 4.5z",
        Icon::Bang => "M11 4h2v10h-2V4zm0 12.2h2v2.2h-2v-2.2z",
        Icon::Xmark => "M6.4 5 12 10.6 17.6 5 19 6.4 13.4 12 19 17.6 17.6 19 12 13.4 6.4 19 5 17.6 10.6 12 5 6.4 6.4 5z",
        Icon::Timer => "M12 4.2a7.8 7.8 0 1 0 0 15.6 7.8 7.8 0 0 0 0-15.6zm0 1.9a5.9 5.9 0 1 1 0 11.8 5.9 5.9 0 0 1 0-11.8zm-.95 2.3v4.2l3.3 2 .95-1.55-2.4-1.45V8.4h-1.85zM9.2 2h5.6v1.7H9.2V2z",
        Icon::Ellipsis => "M6 10.4a1.6 1.6 0 1 0 0 3.2 1.6 1.6 0 0 0 0-3.2zm6 0a1.6 1.6 0 1 0 0 3.2 1.6 1.6 0 0 0 0-3.2zm6 0a1.6 1.6 0 1 0 0 3.2 1.6 1.6 0 0 0 0-3.2z",
        Icon::Star => "M12 3.2l2.6 5.55 5.9.82-4.3 4.3 1.05 6.13L12 17.1l-5.25 2.9L7.8 13.87 3.5 9.57l5.9-.82L12 3.2z",
        Icon::Stack => "M5 8h14v11.5H5V8zm1.8-3h10.4v1.6H6.8V5zm1.6-2.6h7.2V4H8.4V2.4z",
        Icon::Doc => "M6.5 2.6h7l4 4v14.8h-11V2.6zm6.6 1.6v3.3h3.3l-3.3-3.3zM8.6 11h6.8v1.5H8.6V11zm0 3.4h6.8v1.5H8.6v-1.5z",
    }
}

thread_local! {
    static CACHE: RefCell<HashMap<Icon, Path>> = RefCell::new(HashMap::new());
}

fn path(i: Icon) -> Path {
    CACHE.with(|c| c.borrow_mut().entry(i).or_insert_with(|| parse(data(i))).clone())
}

/// Fills `icon` centred on (cx, cy), `size` logical px wide.
pub fn fill(g: &mut Gfx, icon: Icon, cx: f32, cy: f32, size: f32, color: Color) {
    let p = path(icon);
    g.save();
    g.translate(cx - size / 2.0, cy - size / 2.0);
    g.scale_xy(size / 24.0, size / 24.0);
    g.fill_style(color);
    g.fill_path(&p);
    g.restore();
}

/// Strokes `icon` (open paths such as chevrons and the check mark).
pub fn stroke(g: &mut Gfx, icon: Icon, cx: f32, cy: f32, size: f32, width: f32, color: Color) {
    let p = path(icon);
    g.save();
    g.translate(cx - size / 2.0, cy - size / 2.0);
    g.scale_xy(size / 24.0, size / 24.0);
    g.stroke_style(color);
    g.line_width(width);
    g.line_cap_round();
    g.line_join_round();
    g.stroke_path(&p);
    g.restore();
}

// ── SVG path reader ───────────────────────────────────────────────────────────

struct Reader<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn skip(&mut self) {
        while self.i < self.s.len() && (self.s[self.i] == b' ' || self.s[self.i] == b',' || self.s[self.i] == b'\n') {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.s.get(self.i).copied()
    }

    fn has_number(&mut self) -> bool {
        matches!(self.peek(), Some(c) if c == b'-' || c == b'+' || c == b'.' || c.is_ascii_digit())
    }

    fn number(&mut self) -> f32 {
        self.skip();
        let start = self.i;
        if matches!(self.s.get(self.i), Some(b'-') | Some(b'+')) {
            self.i += 1;
        }
        let mut dot = false;
        while let Some(&c) = self.s.get(self.i) {
            if c.is_ascii_digit() {
                self.i += 1;
            } else if c == b'.' && !dot {
                dot = true;
                self.i += 1;
            } else {
                break;
            }
        }
        std::str::from_utf8(&self.s[start..self.i]).ok().and_then(|t| t.parse().ok()).unwrap_or(0.0)
    }

    fn flag(&mut self) -> bool {
        self.skip();
        let c = self.s.get(self.i).copied().unwrap_or(b'0');
        self.i += 1;
        c == b'1'
    }
}

fn parse(d: &str) -> Path {
    let mut r = Reader { s: d.as_bytes(), i: 0 };
    let mut pb = PathBuilder::new();
    let (mut cx, mut cy) = (0.0f32, 0.0f32);
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    let mut cmd = b'M';
    while let Some(c) = r.peek() {
        if c.is_ascii_alphabetic() {
            cmd = c;
            r.i += 1;
        } else if !r.has_number() {
            break;
        }
        let rel = cmd.is_ascii_lowercase();
        match cmd.to_ascii_uppercase() {
            b'M' => {
                let (x, y) = (r.number(), r.number());
                (cx, cy) = if rel { (cx + x, cy + y) } else { (x, y) };
                pb.move_to(cx, cy);
                (sx, sy) = (cx, cy);
                cmd = if rel { b'l' } else { b'L' };
            }
            b'L' => {
                let (x, y) = (r.number(), r.number());
                (cx, cy) = if rel { (cx + x, cy + y) } else { (x, y) };
                pb.line_to(cx, cy);
            }
            b'H' => {
                let x = r.number();
                cx = if rel { cx + x } else { x };
                pb.line_to(cx, cy);
            }
            b'V' => {
                let y = r.number();
                cy = if rel { cy + y } else { y };
                pb.line_to(cx, cy);
            }
            b'C' => {
                let v: Vec<f32> = (0..6).map(|_| r.number()).collect();
                let (ox, oy) = if rel { (cx, cy) } else { (0.0, 0.0) };
                pb.cubic_to(ox + v[0], oy + v[1], ox + v[2], oy + v[3], ox + v[4], oy + v[5]);
                (cx, cy) = (ox + v[4], oy + v[5]);
            }
            b'S' => {
                // Smooth cubic: reflect nothing (the icons never rely on it), use the current point.
                let v: Vec<f32> = (0..4).map(|_| r.number()).collect();
                let (ox, oy) = if rel { (cx, cy) } else { (0.0, 0.0) };
                pb.cubic_to(cx, cy, ox + v[0], oy + v[1], ox + v[2], oy + v[3]);
                (cx, cy) = (ox + v[2], oy + v[3]);
            }
            b'A' => {
                let (rx, ry, rot) = (r.number(), r.number(), r.number());
                let (large, sweep) = (r.flag(), r.flag());
                let (x, y) = (r.number(), r.number());
                let (ex, ey) = if rel { (cx + x, cy + y) } else { (x, y) };
                arc(&mut pb, cx, cy, rx, ry, rot, large, sweep, ex, ey);
                (cx, cy) = (ex, ey);
            }
            b'Z' => {
                pb.close();
                (cx, cy) = (sx, sy);
            }
            _ => break,
        }
    }
    pb.finish().expect("icon path")
}

/// SVG elliptical arc → cubic béziers (endpoint → centre parameterisation).
#[allow(clippy::too_many_arguments)]
fn arc(pb: &mut PathBuilder, x1: f32, y1: f32, rx: f32, ry: f32, rot_deg: f32, large: bool, sweep: bool, x2: f32, y2: f32) {
    if (x1 - x2).abs() < 1e-6 && (y1 - y2).abs() < 1e-6 {
        return;
    }
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx < 1e-6 || ry < 1e-6 {
        pb.line_to(x2, y2);
        return;
    }
    let phi = rot_deg.to_radians();
    let (sp, cp) = phi.sin_cos();
    let dx = (x1 - x2) / 2.0;
    let dy = (y1 - y2) / 2.0;
    let x1p = cp * dx + sp * dy;
    let y1p = -sp * dx + cp * dy;
    let lam = x1p * x1p / (rx * rx) + y1p * y1p / (ry * ry);
    if lam > 1.0 {
        let s = lam.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut co = (num / den).max(0.0).sqrt();
    if large == sweep {
        co = -co;
    }
    let cxp = co * rx * y1p / ry;
    let cyp = -co * ry * x1p / rx;
    let cx = cp * cxp - sp * cyp + (x1 + x2) / 2.0;
    let cy = sp * cxp + cp * cyp + (y1 + y2) / 2.0;
    let ang = |ux: f32, uy: f32, vx: f32, vy: f32| -> f32 {
        let dot = ux * vx + uy * vy;
        let len = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
        let mut a = (dot / len).clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 {
            a = -a;
        }
        a
    };
    let th1 = ang(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dth = ang((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
    if !sweep && dth > 0.0 {
        dth -= std::f32::consts::TAU;
    } else if sweep && dth < 0.0 {
        dth += std::f32::consts::TAU;
    }
    let n = (dth.abs() / (std::f32::consts::PI / 2.0)).ceil().max(1.0) as i32;
    let d = dth / n as f32;
    let k = 4.0 / 3.0 * (d / 4.0).tan();
    let pt = |t: f32| -> (f32, f32) {
        let (s, c) = t.sin_cos();
        (cx + rx * c * cp - ry * s * sp, cy + rx * c * sp + ry * s * cp)
    };
    let dpt = |t: f32| -> (f32, f32) {
        let (s, c) = t.sin_cos();
        (-rx * s * cp - ry * c * sp, -rx * s * sp + ry * c * cp)
    };
    for i in 0..n {
        let t0 = th1 + d * i as f32;
        let t1 = t0 + d;
        let (p0, d0) = (pt(t0), dpt(t0));
        let (p1, d1) = (pt(t1), dpt(t1));
        pb.cubic_to(p0.0 + k * d0.0, p0.1 + k * d0.1, p1.0 - k * d1.0, p1.1 - k * d1.1, p1.0, p1.1);
    }
}

#[allow(dead_code)]
pub fn color_of(c: Color) -> Color {
    c
}
