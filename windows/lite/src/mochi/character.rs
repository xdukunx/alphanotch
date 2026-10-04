// Mochi's look — one drawing kit shared by the live island, the launch greeting
// and the drop sequence, so the three can never drift apart.
//
// The character: a plum helmet, a cream face plate with two ink eyes and pink
// cheeks, teal headphones, a striped scarf, a graduation cap and a gold crown
// whose five medallions ("dots") double as a live status strip for Claude Code:
//
//     0 honeycomb → searching        3 brain      → thinking
//     1 bolt      → working          4 trend/✓    → finished · rate limit
//     2 centre    → needs you: ! (approval)  ? (question)  × (error)
//
// Everything is in units of R (half the helmet height). All helpers draw around
// the origin; the caller has already translated / rotated / squashed the canvas.

use std::f32::consts::{PI, TAU};

use tiny_skia::{Color, Path};

use crate::anim::lerp;
use crate::gfx::{frgb, hex, rgba, rounded_rect_path, Gfx};

pub type Rgb = [f32; 3];

mod pal {
    pub const HELMET_TOP: &str = "#A74B88";
    pub const HELMET_BOTTOM: &str = "#7E2C63";
    pub const PLATE_TOP: &str = "#FFFAEF";
    pub const PLATE_BOTTOM: &str = "#F4EAD6";
    pub const PHONE_DARK: &str = "#3D7893";
    pub const PHONE_LIGHT: &str = "#86BACB";
    pub const GOLD_TOP: &str = "#FAD35E";
    pub const GOLD_BOTTOM: &str = "#EDB33C";
    pub const GOLD_RIM: &str = "#C98F27";
    pub const MEDAL_IDLE: &str = "#F2C14E";
    pub const CREAM: &str = "#FFF6E0";
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)]
}

// ── What the five medallions show ─────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Glyph {
    Bang,
    Question,
    Cross,
    Check,
}

#[derive(Clone, Copy)]
pub struct MedalSpec {
    /// Which medallion lights up, 0…4, or -1 for none.
    pub slot: i32,
    pub glyph: Option<Glyph>,
    /// Idle glow of the other four (0…1).
    pub rest: f32,
}

/// One table to tweak if the mapping above should change.
pub fn medal_for_state(state: &str) -> MedalSpec {
    let m = |slot, glyph, rest| MedalSpec { slot, glyph, rest };
    match state {
        "searching" => m(0, None, 0.1),
        "working" => m(1, None, 0.1),
        "approval" => m(2, Some(Glyph::Bang), 0.1),
        "question" => m(2, Some(Glyph::Question), 0.1),
        "error" => m(2, Some(Glyph::Cross), 0.1),
        "thinking" => m(3, None, 0.1),
        "finished" => m(4, Some(Glyph::Check), 0.28),
        "ratelimit" => m(4, None, 0.1),
        _ => m(-1, None, 0.0),
    }
}

/// Where the crown's five medallions sit (x, y, radius), in R.
const MEDALS: [(f32, f32, f32); 5] = [
    (-0.66, -0.55, 0.155),
    (-0.34, -0.6, 0.155),
    (0.0, -0.64, 0.2),
    (0.34, -0.6, 0.155),
    (0.66, -0.55, 0.155),
];

// ── Helmet + face ─────────────────────────────────────────────────────────────

pub fn fill_helmet(g: &mut Gfx, body: &Path, r: f32, rx: f32, ry: f32, solid: Option<Rgb>) {
    let grad = match solid {
        Some(s) => g.linear(
            rx * 0.7, -ry * 0.85, -rx * 0.8, ry * 0.9,
            &[(0.0, frgb(mix(s, [1.0; 3], 0.18), 1.0)), (1.0, frgb(mix(s, [0.0; 3], 0.12), 1.0))],
        ),
        None => g.linear(
            -rx * 0.5, -ry, rx * 0.6, ry,
            &[(0.0, hex(pal::HELMET_TOP)), (1.0, hex(pal::HELMET_BOTTOM))],
        ),
    };
    g.fill_style(grad);
    g.fill_path(body);
    // Mini bots stay flat: no shading, no glint (and four fewer fills per pill).
    if solid.is_some() {
        return;
    }

    // Soft form shading + a glint, so the helmet reads as a glossy shell.
    let sh = g.radial(
        0.0, 0.0, r * 0.3, r * 1.3,
        &[(0.0, rgba(0, 0, 0, 0.0)), (0.65, rgba(0, 0, 0, 0.0)), (1.0, rgba(30, 0, 20, 0.28))],
    );
    g.fill_style(sh);
    g.fill_path(body);

    let hl = g.radial(
        -rx * 0.5, -ry * 0.55, 0.0, r * 0.5,
        &[(0.0, rgba(255, 255, 255, 0.28)), (1.0, rgba(255, 255, 255, 0.0))],
    );
    g.fill_style(hl);
    g.fill_path(body);
}

/// The cream plate the eyes live on: most of the face, with a thin plum border.
pub fn face_plate(r: f32, yaw: f32, pitch: f32, morph: f32) -> Path {
    let shift_x = yaw.sin() * r * 0.08 * (1.0 - morph);
    let shift_y = -pitch.sin() * r * 0.06 * (1.0 - morph);
    let hw = lerp(0.93, 0.88, morph) * r;
    let hh = lerp(0.68, 0.82, morph) * r;
    let cy = lerp(0.2, 0.08, morph) * r;
    rounded_rect_path(shift_x - hw, cy + shift_y - hh, hw * 2.0, hh * 2.0, lerp(0.42, 0.3, morph) * r)
        .expect("plate path")
}

pub fn fill_plate(g: &mut Gfx, plate: &Path, r: f32, solid: Option<Rgb>) {
    let grad = g.linear(0.0, -r * 0.4, 0.0, r * 0.8, &[(0.0, hex(pal::PLATE_TOP)), (1.0, hex(pal::PLATE_BOTTOM))]);
    g.fill_style(grad);
    g.fill_path(plate);
    g.line_width((r * 0.025).max(0.6));
    g.stroke_style(match solid {
        Some(s) => frgb(mix(s, [0.0; 3], 0.25), 0.5),
        None => rgba(60, 14, 44, 0.55),
    });
    g.stroke_path(plate);
}

pub fn draw_cheeks(g: &mut Gfx, r: f32, yaw: f32, amount: f32, morph: f32) {
    if amount < 0.01 {
        return;
    }
    let a = amount * (1.0 - morph * 0.8);
    let shift = yaw.sin() * r * 0.14;
    g.fill_style(rgba(246, 168, 170, 0.85 * a));
    for sd in [-1.0f32, 1.0] {
        // Kept inside the plate by geometry instead of by a clip mask.
        let cx = (sd * r * 0.62 + shift).clamp(-r * 0.7, r * 0.7);
        g.begin_path();
        g.ellipse(cx, r * lerp(0.4, 0.46, morph), r * 0.22, r * 0.14, 0.0, 0.0, TAU, false);
        g.fill();
    }
}

// ── Behind the helmet: headphones ─────────────────────────────────────────────

pub struct BackOpts {
    /// 0…1 visibility (fades out while Mochi becomes a mailbox).
    pub vis: f32,
    /// 0…1 beat pulse for the ear cups.
    pub beat: f32,
    pub yaw: f32,
}

/// A real headset: a thin band hugging the top of the head and two ear cups at
/// the sides, both tucked *behind* the helmet so only their outer half shows.
pub fn draw_back(g: &mut Gfx, r: f32, rx: f32, ry: f32, o: &BackOpts) {
    if o.vis < 0.02 {
        return;
    }
    g.save();
    g.set_alpha(g.alpha() * o.vis);

    // Headband: an arc just outside the helmet outline, running between the cups.
    g.stroke_style(hex(pal::PHONE_DARK));
    g.line_width(r * 0.13);
    g.line_cap_round();
    g.begin_path();
    g.ellipse(0.0, r * 0.1, rx * 1.075, ry * 1.07, 0.0, PI * 1.02, PI * 1.98, false);
    g.stroke();

    // Ear cups.
    let sx = 1.0 + o.beat * 0.09;
    for sd in [-1.0f32, 1.0] {
        // The cup on the side the head turns away from slides further behind.
        let depth = 1.0 - 0.2 * (-sd * o.yaw.sin()).max(0.0);
        g.save();
        g.translate(sd * (rx + r * 0.07) + o.yaw.sin() * r * 0.05, r * 0.26);
        g.scale_xy(sx * depth, 1.0 + o.beat * 0.04);
        // Outer shell (light) with the cushion (dark) towards the face.
        g.begin_path();
        g.ellipse(0.0, 0.0, r * 0.27, r * 0.46, 0.0, 0.0, TAU, false);
        g.fill_style(hex(pal::PHONE_LIGHT));
        g.fill();
        g.begin_path();
        g.ellipse(sd * r * 0.075, 0.0, r * 0.2, r * 0.38, 0.0, 0.0, TAU, false);
        g.fill_style(hex(pal::PHONE_DARK));
        g.fill();
        g.restore();
    }
    g.restore();
}

// ── In front: the crown ───────────────────────────────────────────────────────

pub struct FrontOpts {
    /// Crown rises by this many R while Mochi is a mailbox, so it never covers the slot.
    pub lift: f32,
    pub yaw: f32,
    /// Lit amount for each of the five medallions, 0…1.
    pub lit: [f32; 5],
    pub glyph: Option<Glyph>,
    /// State colour of the lit medallion.
    pub color: Rgb,
    pub t: f32,
    /// Mini bots get a plain three-point crown.
    pub mini: bool,
    /// Extra crown wobble (rad) for approval jingles etc.
    pub wobble: f32,
}

pub fn draw_front(g: &mut Gfx, r: f32, o: &FrontOpts) {
    let par = o.yaw.sin() * r * 0.08;
    g.save();
    g.translate(par * 0.6, -r * o.lift);
    g.translate(0.0, -r * 0.4);
    g.rotate(o.wobble);
    g.translate(0.0, r * 0.4);
    if o.mini {
        draw_mini_crown(g, r);
    } else {
        draw_crown(g, r, o);
    }
    g.restore();
}

fn poly(g: &mut Gfx, pts: &[(f32, f32)], r: f32, dy: f32) {
    g.begin_path();
    for (i, (px, py)) in pts.iter().enumerate() {
        if i == 0 {
            g.move_to(px * r, (py + dy) * r);
        } else {
            g.line_to(px * r, (py + dy) * r);
        }
    }
    g.close_path();
}

const CROWN: [(f32, f32); 11] = [
    (-0.85, -0.38), (-0.9, -1.0), (-0.58, -0.8), (-0.4, -1.04), (-0.17, -0.8),
    (0.0, -1.28), (0.17, -0.8), (0.4, -1.04), (0.58, -0.8), (0.9, -1.0), (0.85, -0.38),
];

fn draw_crown(g: &mut Gfx, r: f32, o: &FrontOpts) {
    g.line_join_round();
    g.line_width(r * 0.1);
    poly(g, &CROWN, r, 0.0);
    let grad = g.linear(0.0, -r * 1.28, 0.0, -r * 0.38, &[(0.0, hex(pal::GOLD_TOP)), (1.0, hex(pal::GOLD_BOTTOM))]);
    g.fill_style(grad);
    g.stroke_style(hex(pal::GOLD_BOTTOM));
    let p = g.take_path().unwrap();
    g.fill_path(&p);
    g.stroke_path(&p);

    // Diamond above the centre medallion.
    if r >= 14.0 {
        g.fill_style(rgba(255, 255, 255, 0.9));
        g.begin_path();
        g.move_to(0.0, -1.12 * r);
        g.line_to(r * 0.06, -1.03 * r);
        g.line_to(0.0, -0.94 * r);
        g.line_to(-r * 0.06, -1.03 * r);
        g.close_path();
        g.fill();
    }

    for (i, (mx, my, mr)) in MEDALS.iter().enumerate() {
        draw_medal(g, r, mx * r, my * r, mr * r, i, o);
    }
}

fn draw_mini_crown(g: &mut Gfx, r: f32) {
    let pts = [(-0.75, -0.4), (-0.8, -1.0), (-0.38, -0.74), (0.0, -1.22), (0.38, -0.74), (0.8, -1.0), (0.75, -0.4)];
    g.line_join_round();
    g.line_width(r * 0.12);
    poly(g, &pts, r, 0.0);
    g.fill_style(hex(pal::GOLD_TOP));
    g.stroke_style(hex(pal::GOLD_TOP));
    let p = g.take_path().unwrap();
    g.fill_path(&p);
    g.stroke_path(&p);
}

fn draw_medal(g: &mut Gfx, r: f32, cx: f32, cy: f32, mr: f32, idx: usize, o: &FrontOpts) {
    let lit = o.lit[idx];
    let pulse = 0.78 + 0.22 * (o.t * 6.0 + idx as f32).sin();

    // Tiny sizes: a plain dot is all that survives, and that is enough.
    if r < 14.0 {
        g.begin_path();
        g.arc(cx, cy, mr * 0.9, 0.0, TAU, false);
        g.fill_style(frgb(mix([0.79, 0.56, 0.15], o.color, lit), 1.0));
        g.fill();
        return;
    }

    if lit > 0.02 {
        let gr = g.radial(cx, cy, mr * 0.4, mr * 2.4, &[(0.0, frgb(o.color, 0.7 * lit * pulse)), (1.0, frgb(o.color, 0.0))]);
        g.fill_style(gr);
        g.circle(cx, cy, mr * 2.4);
        g.fill();
    }

    // Ring + face.
    g.fill_style(hex(pal::GOLD_RIM));
    g.circle(cx, cy, mr);
    g.fill();
    g.fill_style(if lit > 0.02 {
        frgb(mix([0.96, 0.76, 0.31], o.color, (lit * 1.1).min(1.0)), 1.0)
    } else {
        hex(pal::MEDAL_IDLE)
    });
    g.circle(cx, cy, mr * 0.82);
    g.fill();

    g.save();
    g.translate(cx, cy);
    let s = mr * 0.5;
    let col: Color = if lit > 0.3 { Color::WHITE } else { hex(pal::CREAM) };
    g.stroke_style(col);
    g.fill_style(col);
    g.mul_alpha(0.62 + 0.38 * lit);
    g.line_width((s * 0.26).max(0.8));
    g.line_cap_round();
    g.line_join_round();

    let glyph = if idx == 2 && lit > 0.3 {
        o.glyph
    } else if idx == 4 && lit > 0.3 && o.glyph == Some(Glyph::Check) {
        Some(Glyph::Check)
    } else {
        None
    };
    match glyph {
        Some(gl) => draw_glyph(g, gl, s, o.t),
        None => draw_icon(g, idx, s, lit, o.t),
    }
    g.restore();
}

fn hexagon(g: &mut Gfx, r: f32) {
    g.begin_path();
    for i in 0..6 {
        let a = PI / 6.0 + i as f32 * PI / 3.0;
        g.line_to(a.cos() * r, a.sin() * r);
    }
    g.close_path();
}

fn draw_icon(g: &mut Gfx, idx: usize, s: f32, lit: f32, t: f32) {
    match idx {
        0 => {
            // honeycomb
            hexagon(g, s * 1.05);
            g.stroke();
            hexagon(g, s * 0.45);
            g.fill();
        }
        1 => {
            // bolt
            let k = 1.0 + 0.12 * lit * (t * 12.0).sin();
            g.scale_xy(k, k);
            g.begin_path();
            g.move_to(s * 0.2, -s * 1.1);
            g.line_to(-s * 0.7, s * 0.15);
            g.line_to(-s * 0.05, s * 0.15);
            g.line_to(-s * 0.25, s * 1.1);
            g.line_to(s * 0.7, -s * 0.2);
            g.line_to(s * 0.05, -s * 0.2);
            g.close_path();
            g.fill();
        }
        2 => {
            // bars
            let hs = [0.9f32, 1.5, 2.0];
            for (i, h) in hs.iter().enumerate() {
                let bx = (i as f32 - 1.0) * s * 0.62;
                let bh = s * h * (1.0 + 0.1 * lit * (t * 5.0 + i as f32 * 1.7).sin());
                g.round_rect(bx - s * 0.2, s * 0.8 - bh, s * 0.4, bh, s * 0.12);
                g.fill();
            }
        }
        3 => {
            // brain
            g.begin_path();
            g.arc(-s * 0.36, 0.0, s * 0.62, PI * 0.55, PI * 1.55, false);
            g.stroke();
            g.begin_path();
            g.arc(s * 0.36, 0.0, s * 0.62, PI * 1.45, PI * 0.45, false);
            g.stroke();
            g.begin_path();
            g.move_to(0.0, -s * 0.55);
            g.line_to(0.0, s * 0.55);
            g.stroke();
            g.begin_path();
            g.move_to(-s * 0.55, -s * 0.05);
            g.quad_to(-s * 0.2, s * 0.3, 0.0, s * 0.05);
            g.stroke();
        }
        _ => {
            // trend
            g.begin_path();
            g.move_to(-s * 0.95, s * 0.65);
            g.line_to(-s * 0.25, -s * 0.05);
            g.line_to(s * 0.2, s * 0.35);
            g.line_to(s * 0.9, -s * 0.5);
            g.stroke();
            g.begin_path();
            g.move_to(s * 0.35, -s * 0.55);
            g.line_to(s * 0.92, -s * 0.55);
            g.line_to(s * 0.92, s * 0.02);
            g.stroke();
        }
    }
}

fn draw_glyph(g: &mut Gfx, glyph: Glyph, s: f32, t: f32) {
    match glyph {
        Glyph::Bang => {
            let k = 1.0 + 0.08 * (t * 9.0).sin();
            g.scale_xy(k, k);
            g.round_rect(-s * 0.18, -s * 1.0, s * 0.36, s * 1.4, s * 0.18);
            g.fill();
            g.circle(0.0, s * 0.8, s * 0.22);
            g.fill();
        }
        Glyph::Question => {
            g.begin_path();
            g.arc(0.0, -s * 0.35, s * 0.55, PI * 1.05, PI * 2.45, false);
            g.quad_to(s * 0.4, s * 0.2, 0.0, s * 0.35);
            g.stroke();
            g.circle(0.0, s * 0.85, s * 0.2);
            g.fill();
        }
        Glyph::Cross => {
            g.begin_path();
            g.move_to(-s * 0.7, -s * 0.7);
            g.line_to(s * 0.7, s * 0.7);
            g.move_to(s * 0.7, -s * 0.7);
            g.line_to(-s * 0.7, s * 0.7);
            g.stroke();
        }
        Glyph::Check => {
            g.begin_path();
            g.move_to(-s * 0.8, 0.0);
            g.line_to(-s * 0.25, s * 0.6);
            g.line_to(s * 0.85, -s * 0.6);
            g.stroke();
        }
    }
}
