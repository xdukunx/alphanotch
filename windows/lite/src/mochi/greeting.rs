// The launch "coucou" — port of mochi/greeting.ts (GreetingCanvasView.swift).
// Everything is laid out in the same 640×150 reference space as on macOS.

use std::f32::consts::{PI, TAU};

use crate::anim::{clamp, ease, lerp, now, seg};
use crate::gfx::{hex, rgba, with_alpha, Gfx};
use crate::layout::{COMPACT_W, NOTCH_H, NOTCH_W};
use crate::mochi::character::{
    draw_back, draw_cheeks, draw_front, face_plate, fill_helmet, fill_plate, BackOpts, FrontOpts,
};
use crate::sound;

// ── Timing (mirrors greeting-v2.html `T`) ─────────────────────────────────────

struct T;
#[allow(dead_code)]
impl T {
    const GROW: f32 = 0.45;
    const SQUINT0: f32 = 0.6;
    const SQUINT1: f32 = 0.82;
    const DIP0: f32 = 1.25;
    const DIP1: f32 = 1.4;
    const POP0: f32 = 1.36;
    const POP1: f32 = 1.52;
    const CONTENT0: f32 = 2.45;
    const CONTENT1: f32 = 2.58;
    const TUCK0: f32 = 2.58;
    const TUCK1: f32 = 2.8;
    const BADGE: f32 = 2.72;
    const DOWN0: f32 = 2.85;
    const DOWN1: f32 = 3.2;
    const BLINK2: f32 = 3.8;
    const TINT0: f32 = 3.85;
    const TINT1: f32 = 4.15;
    const END: f32 = 4.6;
    const AUTO_LEAVE: f32 = 4.9;
    const COLLAPSE: f32 = 0.34;
}

// ── Geometry (640×150) ────────────────────────────────────────────────────────

const C0: (f32, f32) = (320.0, 90.0);
const HB: f32 = 58.0;
const ASP: f32 = 1.08;
const EAR_X: f32 = 26.0;
const EAR_Y: f32 = 17.5;
const EAR_HB: f32 = 24.0;
const CARD: (f32, f32, f32, f32) = (10.0, 36.0, 620.0, 104.0);
const CARD_R: f32 = 20.0;
const SMALL_W: f32 = COMPACT_W;

#[derive(Clone, Copy, PartialEq)]
enum EyeType {
    Dot,
    Happy,
    Content,
}

#[derive(Clone, Copy)]
struct Pose {
    hb: f32,
    x: f32,
    y: f32,
    sx: f32,
    sy: f32,
    tilt: f32,
    eye: EyeType,
    open: f32,
    eye_roll: f32,
    look_x: f32,
    look_y: f32,
    hand_l: f32,
    hand_r: f32,
    wave: f32,
    badge: f32,
    tint: f32,
    halo: f32,
    halo_blue: f32,
    minis: f32,
    fx: f32,
    card: f32,
}

fn greet_pose(t: f32) -> Pose {
    let gg = ease::back(seg(t, 0.02, T::GROW));
    let hb = lerp(3.0, HB, gg);
    let mut x = C0.0;
    let mut y = lerp(16.0, C0.1, ease::out(seg(t, 0.02, T::GROW)));
    let (mut sx, mut sy, mut tilt) = (1.0, 1.0, 0.0);

    if t >= T::DIP0 && t < T::POP1 {
        let k = (PI * seg(t, T::DIP0, T::POP1)).sin();
        y += hb * 0.22 * k;
        sy = 1.0 - 0.06 * k;
        sx = 1.0 + 0.04 * k;
    }
    if t >= T::POP1 && t < T::TUCK1 {
        let w = t - T::POP1;
        let fade = 1.0 - seg(t, T::TUCK0, T::TUCK1);
        x += (w * TAU * 0.9).sin() * hb * ASP * 0.05 * fade;
        tilt = (w * TAU * 0.9 + 0.6).sin() * 0.05 * fade;
        y += (w * TAU * 1.8).sin() * 0.8 * fade;
    }
    if t >= T::TUCK0 && t < T::DOWN1 {
        y += hb * 0.12 * (PI * seg(t, T::TUCK0, T::DOWN1)).sin();
    }

    let mut eye = EyeType::Dot;
    if t >= T::SQUINT0 && t < T::SQUINT1 {
        eye = EyeType::Happy;
    }
    if t >= T::CONTENT0 && t < T::CONTENT1 {
        eye = EyeType::Content;
    }
    if t >= T::DOWN0 && t < T::DOWN1 {
        eye = EyeType::Content;
    }
    let mut eye_roll = 0.0;
    if t >= T::DIP0 && t < T::POP1 {
        eye_roll = (PI * seg(t, T::DIP0, T::POP1)).sin();
    }
    let blink = |tb: f32| -> f32 {
        let k = seg(t, tb, tb + 0.12);
        if k > 0.0 && k < 1.0 { 1.0 - (PI * k).sin() * 0.94 } else { 1.0 }
    };
    let open = blink(1.95).min(blink(T::BLINK2));

    let (mut look_x, mut look_y) = (0.0, 0.0);
    if t >= T::SQUINT1 && t < T::DIP0 {
        look_y = -0.2;
    }
    if t >= T::POP1 && t < T::CONTENT0 {
        look_x = 0.55;
        look_y = -0.45;
    }
    if t >= T::CONTENT0 && t < T::DOWN1 {
        look_x = -0.3;
        look_y = 0.6;
    }
    if t >= T::DOWN1 {
        let k = ease::in_out(seg(t, T::DOWN1, T::DOWN1 + 0.35));
        look_x = lerp(-0.3, 0.0, k);
        look_y = lerp(0.6, 0.0, k);
    }

    let hand_l = if t < T::TUCK0 {
        ease::back(seg(t, 0.8 * 0.0 + 1.36, 1.36 + 0.14))
    } else {
        1.0 - ease::ease_in(seg(t, T::TUCK0, T::TUCK1 - 0.03))
    };
    let hand_r = if t < T::TUCK0 {
        ease::back(seg(t, 1.36 + 0.04, 1.36 + 0.18))
    } else {
        1.0 - ease::ease_in(seg(t, T::TUCK0 + 0.03, T::TUCK1))
    };
    let wave = if t >= T::POP1 && t < T::TUCK0 { t - T::POP1 } else { -1.0 };

    Pose {
        hb, x, y, sx, sy, tilt, eye, open, eye_roll, look_x, look_y,
        hand_l, hand_r, wave,
        badge: ease::back(seg(t, T::BADGE, T::BADGE + 0.28)),
        tint: 0.6 * ease::in_out(seg(t, T::TINT0, T::TINT1)),
        halo: ease::out(seg(t, 0.3, 0.7)),
        halo_blue: seg(t, T::TINT0, T::TINT1),
        minis: 0.0,
        fx: 1.0,
        card: seg(t, 0.18, 0.45),
    }
}

fn small_pose() -> Pose {
    Pose {
        hb: EAR_HB,
        x: 320.0 - SMALL_W / 2.0 + EAR_X,
        y: EAR_Y,
        sx: 1.0, sy: 1.0, tilt: 0.0,
        eye: EyeType::Dot, open: 1.0, eye_roll: 0.0,
        look_x: 0.0, look_y: 0.0,
        hand_l: 0.0, hand_r: 0.0, wave: -1.0,
        badge: 1.0, tint: 0.6, halo: 0.6, halo_blue: 1.0,
        minis: 1.0, fx: 1.0, card: 0.0,
    }
}

fn pose(t: f32, tc: f32) -> Pose {
    if t < tc {
        return greet_pose(t.min(T::END + 10.0));
    }
    let a = greet_pose(tc);
    let b = small_pose();
    let e = ease::in_out(seg(t, tc, tc + T::COLLAPSE));
    let mut p = a;
    p.x = lerp(a.x, b.x, e);
    p.y = lerp(a.y, b.y, e);
    p.hb = lerp(a.hb, b.hb, e);
    p.badge = lerp(a.badge, b.badge, e);
    p.tint = lerp(a.tint, b.tint, e);
    p.halo = lerp(a.halo, b.halo, e);
    p.halo_blue = lerp(a.halo_blue, b.halo_blue, e);
    p.card = a.card * (1.0 - seg(t, tc, tc + 0.18));
    p.hand_l = a.hand_l * (1.0 - seg(t, tc, tc + 0.15));
    p.hand_r = a.hand_r * (1.0 - seg(t, tc, tc + 0.15));
    p.tilt = a.tilt * (1.0 - e);
    p.sx = lerp(a.sx, 1.0, e);
    p.sy = lerp(a.sy, 1.0, e);
    p.eye_roll = a.eye_roll * (1.0 - e);
    let bk = seg(t, tc + 0.14, tc + 0.26);
    p.eye = EyeType::Dot;
    p.open = if bk > 0.0 && bk < 1.0 { 1.0 - (PI * bk).sin() * 0.94 } else { 1.0 };
    p.look_x = a.look_x * (1.0 - e);
    p.look_y = a.look_y * (1.0 - e);
    p.minis = ease::back(seg(t, tc + 0.24, tc + 0.42));
    p.fx = 1.0 - seg(t, tc, tc + 0.2);
    p
}

// ── Particles (seeded LCG, seed = 7, identical sequence to the Swift version) ──

struct RingDot {
    a: f32,
    j: f32,
    s: f32,
    al: f32,
}
struct Ring {
    t0: f32,
    dots: Vec<RingDot>,
}
struct Streak {
    a: f32,
    sp: f32,
    len: f32,
    t0: f32,
    col: &'static str,
}

fn particles() -> (Vec<Ring>, Vec<Streak>) {
    let mut seed: i32 = 7;
    let mut rnd = move || -> f32 {
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345) & 0x7fff_ffff;
        seed as f32 / 0x7fff_ffff as f32
    };
    let rings = [0.1f32, 0.2, 0.3, 0.45, 0.6]
        .iter()
        .map(|&t0| Ring {
            t0,
            dots: (0..170)
                .map(|_| RingDot {
                    a: rnd() * TAU,
                    j: (rnd() - 0.5) * 0.22,
                    s: 0.7 + rnd() * 0.9,
                    al: 0.45 + rnd() * 0.55,
                })
                .collect(),
        })
        .collect();
    let cols = ["#3B9EFF", "#F29B38", "#FF5A4E", "#2EC4A0", "#A78BFA"];
    let streaks = (0..16)
        .map(|i| Streak {
            a: i as f32 / 16.0 * TAU + (rnd() - 0.5) * 0.3,
            sp: 230.0 + rnd() * 260.0,
            len: 6.0 + rnd() * 9.0,
            t0: 0.08 + rnd() * 0.14,
            col: cols[i % 5],
        })
        .collect();
    (rings, streaks)
}

// ── Drawing ───────────────────────────────────────────────────────────────────

fn mochi_path(hw: f32, hh: f32) -> tiny_skia::Path {
    let n = 3.2;
    let mut pb = tiny_skia::PathBuilder::new();
    for i in 0..=96 {
        let a = i as f32 / 96.0 * TAU;
        let (sa, ca) = a.sin_cos();
        let px = hw * ca.signum() * ca.abs().powf(2.0 / n);
        let py = hh * sa.signum() * sa.abs().powf(2.0 / n);
        if i == 0 { pb.move_to(px, py) } else { pb.line_to(px, py) }
    }
    pb.close();
    pb.finish().expect("mochi path")
}

fn cream(g: &mut Gfx, x0: f32, y0: f32, x1: f32, y1: f32) -> crate::gfx::Style {
    g.linear(x0, y0, x1, y1, &[(0.0, hex("#FFFAEF")), (1.0, hex("#EFE2C8"))])
}

fn draw_hand_l(g: &mut Gfx, hw: f32, hh: f32, p: &Pose) {
    let k = p.hand_l;
    if k <= 0.01 {
        return;
    }
    let hb = hh * 2.0;
    let r = hb * 0.15 * k;
    let rx = lerp(-hw * 0.35, -hw - hb * 0.22, k);
    let mut ry = lerp(hh * 0.85, hh * 0.62, k);
    if p.wave >= 0.0 {
        ry += (p.wave * 6.0).sin() * hb * 0.02;
    }
    g.save();
    g.translate(rx, ry);
    let c = cream(g, r, -r, -r, r);
    g.fill_style(c);
    g.circle(0.0, 0.0, r);
    g.fill();
    g.stroke_style(rgba(0, 0, 0, 0.08));
    g.line_width(0.8);
    g.circle(0.0, 0.0, r);
    g.stroke();
    g.restore();
}

fn draw_hand_r(g: &mut Gfx, hw: f32, hh: f32, p: &Pose) {
    let k = p.hand_r;
    if k <= 0.01 {
        return;
    }
    let hb = hh * 2.0;
    let l = hb * 0.4 * k;
    let th = hb * 0.22 * k;
    let mut rx = lerp(hw * 0.35, hw + hb * 0.2, k);
    let mut ry = lerp(hh * 0.85, hh * 0.2, k);
    let mut ang = -0.61;
    if p.wave >= 0.0 {
        let w = p.wave * TAU * 2.5;
        ang += w.sin() * 0.21;
        ry += (w + 0.8).sin() * hb * 0.04;
        rx += w.cos() * hb * 0.015;
    }
    g.save();
    g.translate(rx, ry);
    g.rotate(ang);
    let c = cream(g, l / 2.0, -th / 2.0, -l / 2.0, th / 2.0);
    g.fill_style(c);
    g.round_rect(-l / 2.0, -th / 2.0, l, th, th / 2.0);
    g.fill();
    g.stroke_style(rgba(0, 0, 0, 0.08));
    g.line_width(0.8);
    g.round_rect(-l / 2.0, -th / 2.0, l, th, th / 2.0);
    g.stroke();
    g.restore();
}

fn draw_mochi(g: &mut Gfx, p: &Pose, t_now: f32) {
    let hh = p.hb / 2.0;
    let hw = hh * ASP;
    if hh <= 0.4 {
        return;
    }

    // Halo: golden → blue, two passes for a soft aura
    if p.halo > 0.0 {
        let bl = p.halo_blue;
        let (cr, cg, cb) = (lerp(232.0, 59.0, bl).round() as u8, lerp(195.0, 158.0, bl).round() as u8, lerp(154.0, 255.0, bl).round() as u8);
        for (rad, alpha) in [(hw * 2.6, 0.18f32), (hw * 4.2, 0.07)] {
            let grad = g.radial(p.x, p.y, 0.0, rad, &[(0.0, rgba(cr, cg, cb, alpha * p.halo)), (1.0, rgba(cr, cg, cb, 0.0))]);
            g.fill_style(grad);
            g.circle(p.x, p.y, rad);
            g.fill();
        }
    }

    g.save();
    g.translate(p.x, p.y);
    g.rotate(p.tilt);
    g.scale_xy(p.sx, p.sy);

    draw_hand_l(g, hw, hh, p);
    draw_hand_r(g, hw, hh, p);

    let r = hh;
    let (rx, ry) = (hw, hh);
    let body = mochi_path(hw, hh);
    let look_yaw = p.look_x * 0.5;
    let look_pitch = -p.look_y * 0.4;

    draw_back(g, r, rx, ry, &BackOpts { vis: 1.0, beat: 0.0, yaw: look_yaw });
    fill_helmet(g, &body, r, rx, ry, None);

    let plate = face_plate(r, look_yaw, look_pitch, 0.0);
    fill_plate(g, &plate, r, None);
    draw_cheeks(g, r, look_yaw, 0.6 + p.tint * 0.4, 0.0);
    g.save();

    // Eyes — ink pills on the plate, same proportions as the live island.
    g.fill_style(hex("#1A1412"));
    g.stroke_style(hex("#1A1412"));
    let ew = r * 0.11;
    let eh = r * 0.15;
    let sp = r * 0.42;
    let lx = p.look_x * r * 0.16;
    let ly = r * 0.12 + p.look_y * r * 0.2 + p.eye_roll * r * 1.1;
    for sd in [-1.0f32, 1.0] {
        g.save();
        g.translate(sd * sp + lx, ly);
        match p.eye {
            EyeType::Happy => {
                g.line_width(ew * 1.1);
                g.line_cap_round();
                g.begin_path();
                g.arc(0.0, ew * 0.6, ew * 1.5, PI * 1.15, PI * 1.85, false);
                g.stroke();
            }
            EyeType::Content => {
                g.line_width(ew * 1.1);
                g.line_cap_round();
                g.begin_path();
                g.arc(0.0, -ew * 0.5, ew * 1.5, PI * 0.15, PI * 0.85, false);
                g.stroke();
            }
            EyeType::Dot => {
                let hh2 = (ew * 0.5).max(eh * p.open);
                g.round_rect(-ew, -hh2, ew * 2.0, hh2 * 2.0, ew);
                g.fill();
            }
        }
        g.restore();
    }
    g.restore();

    // Crown: the bolt medallion lights up with the activity badge, as before.
    draw_front(g, r, &FrontOpts {
        lift: 0.0, yaw: look_yaw,
        lit: [0.0, p.badge.min(1.0), 0.0, 0.0, 0.0], glyph: None, color: [0.231, 0.62, 1.0], t: t_now,
        mini: false, wobble: 0.0,
    });
    g.restore();
}

fn draw_particles(g: &mut Gfx, t: f32, p: &Pose, rings: &[Ring], streaks: &[Streak]) {
    if !(p.card > 0.0 || p.fx < 1.0) {
        return;
    }
    for ring in rings {
        let k = seg(t, ring.t0, ring.t0 + 1.35);
        if k <= 0.0 || k >= 1.0 {
            continue;
        }
        let rx = lerp(14.0, 380.0, ease::out(k));
        let ry = rx * 0.34;
        let fade = (1.0 - k) * if k < 0.08 { k / 0.08 } else { 1.0 } * p.fx * p.card;
        for d in &ring.dots {
            let r = 1.0 + d.j;
            g.fill_style(rgba(255, 255, 255, d.al * fade));
            g.fill_rect(C0.0 + d.a.cos() * rx * r, C0.1 + d.a.sin() * ry * r, d.s, d.s);
        }
    }
    for s in streaks {
        let k = seg(t, s.t0, s.t0 + 0.6);
        if k <= 0.0 || k >= 1.0 {
            continue;
        }
        let dist = s.sp * ease::out(k) * 0.9 + 10.0;
        let alpha = (1.0 - k) * p.fx;
        g.stroke_style(with_alpha(hex(s.col), alpha));
        g.line_width(1.6);
        g.line_cap_round();
        g.begin_path();
        g.move_to(C0.0 + s.a.cos() * (dist - s.len), C0.1 + s.a.sin() * (dist - s.len) * 0.42);
        g.line_to(C0.0 + s.a.cos() * dist, C0.1 + s.a.sin() * dist * 0.42);
        g.stroke();
    }
}

const MINI_COLORS: [&str; 4] = ["#E86A6A", "#3E86E0", "#EFAE5A", "#8C73F2"];

fn draw_minis(g: &mut Gfx, alpha: f32) {
    if alpha <= 0.01 {
        return;
    }
    let cx = 320.0 + SMALL_W / 2.0 - 27.0;
    let cy = 16.0;
    let sp = 6.0;
    let offsets = [(-sp, -sp), (sp, -sp), (-sp, sp), (sp, sp)];
    for (i, (dx, dy)) in offsets.iter().enumerate() {
        g.save();
        g.translate(cx + dx, cy + dy);
        g.scale_xy(alpha, alpha);
        g.fill_style(hex(MINI_COLORS[i]));
        let p = mochi_path(5.3, 4.0);
        g.fill_path(&p);
        g.restore();
    }
}

// ── Controller ────────────────────────────────────────────────────────────────

/// Runs the greeting animation. `take_complete()` fires once at T.end (or right
/// after the collapse when interrupted) so the FSM can move on.
pub struct Greeting {
    start: f32,
    tc: f32,
    fired: bool,
    reported: bool,
    played_greet: bool,
    played_badge: bool,
    rings: Vec<Ring>,
    streaks: Vec<Streak>,
}

impl Default for Greeting {
    fn default() -> Self {
        Self::new()
    }
}

impl Greeting {
    pub fn new() -> Self {
        let (rings, streaks) = particles();
        Self { start: 0.0, tc: f32::INFINITY, fired: true, reported: true, played_greet: true, played_badge: true, rings, streaks }
    }

    pub fn start(&mut self) {
        self.start = now();
        self.tc = f32::INFINITY;
        self.fired = false;
        self.reported = false;
        self.played_greet = false;
        self.played_badge = false;
    }

    /// Mouse entered the island during the greeting — hold it open.
    pub fn hover(&mut self) {
        if self.tc >= T::AUTO_LEAVE {
            self.tc = f32::INFINITY;
        }
    }

    /// Mouse left — collapse from now.
    pub fn interrupt(&mut self) {
        let t = self.elapsed();
        if !self.tc.is_finite() || self.tc > t {
            self.tc = t;
        }
    }

    pub fn elapsed(&self) -> f32 {
        now() - self.start
    }

    /// Advances the sound cues and the completion flag.
    pub fn update(&mut self, _n: f32) {
        let t = self.elapsed();
        if !self.played_greet && t >= T::POP0 {
            self.played_greet = true;
            sound::play("greet");
        }
        if !self.played_badge && t >= T::BADGE {
            self.played_badge = true;
            sound::play("blip");
        }
        if !self.fired && t >= T::END + 0.05 && self.tc >= T::AUTO_LEAVE {
            self.fired = true;
        }
        if !self.fired && self.tc.is_finite() && t >= self.tc + T::COLLAPSE {
            self.fired = true;
        }
    }

    pub fn take_complete(&mut self) -> bool {
        if self.fired && !self.reported {
            self.reported = true;
            return true;
        }
        false
    }

    pub fn draw(&mut self, g: &mut Gfx) {
        let t = self.elapsed();
        let p = pose(t, self.tc);
        let a = clamp(p.card, 0.0, 1.0);

        if p.card > 0.0 {
            g.save();
            g.mul_alpha(a);
            g.fill_style(hex("#141518"));
            g.fill_round_rect(CARD.0, CARD.1, CARD.2, CARD.3, CARD_R);
            g.restore();

            g.save();
            g.clip_round_rect(CARD.0, CARD.1, CARD.2, CARD.3, CARD_R);
            draw_particles(g, t, &p, &self.rings, &self.streaks);
            g.restore();
        } else if self.tc.is_finite() && t >= self.tc {
            draw_particles(g, t, &p, &self.rings, &self.streaks);
        }

        draw_minis(g, p.minis);
        draw_mochi(g, &p, now());
    }
}

#[allow(dead_code)]
const _: (f32, f32) = (NOTCH_W, NOTCH_H);
