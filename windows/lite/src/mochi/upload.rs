// The drop choreography — port of upload/sequence.ts + upload/canvas.ts
// (UploadSequenceEngine.swift, itself design/prototype/upload-sequence.html).
//
// The sequence is pure arithmetic: it takes the cursor and a drop time and hands
// `frame()` back everything the canvas needs. All coordinates are island points
// (the island is 640 × 176), so every constant is the macOS constant unchanged.

use std::f32::consts::{PI, TAU};

use crate::anim::{lerp, now, seg};
use crate::app::App;
use crate::gfx::{hex, rgb, rgba, rounded_rect_path, with_alpha, Gfx};
use crate::layout::View;
use crate::mochi::character::{
    draw_back, draw_cheeks, draw_front, face_plate, fill_helmet, fill_plate, BackOpts, FrontOpts,
};
use crate::text::{self, Align, Face};
use crate::ui::{id_of, Rect};

pub struct Usc {
    pub w: f32,
    pub card_x: f32,
    pub card_y: f32,
    pub card_w: f32,
    pub card_h: f32,
    pub card_r: f32,
    pub rest_x: f32,
    pub rest_y: f32,
    pub d_box: f32,
    pub follow_min: f32,
    pub follow_max: f32,
    pub text_x: f32,
    pub text_y: f32,
    pub bar_x0: f32,
    pub bar_x1: f32,
    pub bar_y: f32,
    pub choose_x: f32,
    pub choose_y: f32,
    pub choose_d: f32,
    pub lock_in: f32,
    pub lock_out: f32,
    pub mouth_ajar: f32,
    pub mouth_open: f32,
    pub mouth_max: f32,
    pub t_drop: f32,
    pub t_suck_start: f32,
    pub t_suck_end: f32,
    pub t_close_end: f32,
    pub t_chew1: f32,
    pub t_chew_end: f32,
    pub t_shrink_end: f32,
    pub t_bar_in: f32,
    pub t_prog_start: f32,
    pub dt: f32,
    pub entry_t_ref: f32,
}

/// Constants — exact mirror of USC in UploadSequenceEngine.swift.
pub const USC: Usc = Usc {
    w: 640.0,
    card_x: 10.0,
    card_y: 42.0,
    card_w: 620.0,
    card_h: 124.0,
    card_r: 20.0,
    rest_x: 140.0,
    rest_y: 104.0,
    d_box: 62.0,
    follow_min: 60.0,
    follow_max: 580.0,
    text_x: 196.0,
    text_y: 94.0,
    bar_x0: 46.0,
    bar_x1: 520.0,
    bar_y: 118.0,
    choose_x: 60.0,
    choose_y: 101.0,
    choose_d: 62.0,
    lock_in: 60.0,
    lock_out: 90.0,
    mouth_ajar: 0.2,
    mouth_open: 0.42,
    mouth_max: 0.5,
    t_drop: 1.95,
    t_suck_start: 2.03,
    t_suck_end: 2.33,
    t_close_end: 2.42,
    t_chew1: 2.6,
    t_chew_end: 2.88,
    t_shrink_end: 3.23,
    t_bar_in: 3.0,
    t_prog_start: 3.25,
    dt: 1.0 / 240.0,
    entry_t_ref: 1.95 - 0.4,
};

// ── Easing ────────────────────────────────────────────────────────────────────

fn e_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}
fn e_in(t: f32) -> f32 {
    t * t * t
}
fn e_in_out(t: f32) -> f32 {
    if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 }
}
fn e_back(t: f32) -> f32 {
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

fn squeeze_y(t: f32) -> f32 {
    let (t0, t1, t2, t3) = (USC.t_suck_end, USC.t_suck_end + 0.07, USC.t_suck_end + 0.2, USC.t_chew1);
    if t <= t0 { 1.06 }
    else if t <= t1 { lerp(1.06, 0.82, e_out(seg(t, t0, t1))) }
    else if t <= t2 { lerp(0.82, 1.1, e_out(seg(t, t1, t2))) }
    else if t <= t3 { lerp(1.1, 1.0, e_in_out(seg(t, t2, t3))) }
    else { 1.0 }
}

fn squeeze_x(t: f32) -> f32 {
    let (t0, t1, t2, t3) = (USC.t_suck_end, USC.t_suck_end + 0.07, USC.t_suck_end + 0.2, USC.t_chew1);
    if t <= t0 { 0.97 }
    else if t <= t1 { lerp(0.97, 1.14, e_out(seg(t, t0, t1))) }
    else if t <= t2 { lerp(1.14, 0.95, e_out(seg(t, t1, t2))) }
    else if t <= t3 { lerp(0.95, 1.0, e_in_out(seg(t, t2, t3))) }
    else { 1.0 }
}

/// Progress curve of the upload bar: quick to 60 %, an unhurried middle, then a
/// last push. A plain ease-out reads as a different animation entirely.
pub fn upload_progress_curve(u: f32) -> f32 {
    if u < 0.4 {
        0.6 * e_out(u / 0.4)
    } else if u < 0.85 {
        0.6 + 0.32 * e_in_out((u - 0.4) / 0.45)
    } else {
        0.92 + 0.08 * e_in((u - 0.85) / 0.15)
    }
}

pub fn progress_at(t: f32, start: f32, end: f32) -> f32 {
    if t < start { 0.0 } else { upload_progress_curve(seg(t, start, end)) }
}

/// The reference `spring(s, target, response, damping, dt)`, integrated by hand.
#[derive(Clone, Copy)]
struct Sp {
    v: f32,
    vel: f32,
}

impl Sp {
    fn new(v: f32) -> Self {
        Self { v, vel: 0.0 }
    }
    fn step(&mut self, target: f32, response: f32, damping: f32, dt: f32) {
        let k = (TAU / response).powi(2);
        let c = 2.0 * damping * k.sqrt();
        let a = k * (target - self.v) - c * self.vel;
        self.vel += a * dt;
        self.v += self.vel * dt;
    }
}

// ── Frame ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum UEye {
    Pill,
    Cup,
    Content,
}

#[derive(Clone, Copy, Default)]
pub struct MouthRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy)]
pub struct Frame {
    pub t: f32,
    pub cursor_x: f32,
    pub cursor_y: f32,
    pub morph: f32,
    pub x: f32,
    pub y: f32,
    pub d: f32,
    pub sx: f32,
    pub sy: f32,
    pub tilt: f32,
    pub hop: f32,
    pub mouth: f32,
    pub mouth_rect: MouthRect,
    pub eye: UEye,
    pub look_x: f32,
    pub look_y: f32,
    pub file_visible: bool,
    pub suck: f32,
    pub zone_over: bool,
    pub zone_alpha: f32,
    pub text_alpha: f32,
    pub bar_reveal: f32,
    pub bar_alpha: f32,
    pub progress: f32,
    pub flash: f32,
    pub check: f32,
    pub green_wash: f32,
    pub choose_alpha: f32,
    pub prog_end: f32,
    pub grow_start: f32,
    pub grow_end: f32,
}

fn rest_frame() -> Frame {
    Frame {
        t: 0.0,
        cursor_x: 600.0,
        cursor_y: 280.0,
        morph: 0.0,
        x: USC.rest_x,
        y: USC.rest_y,
        d: USC.d_box,
        sx: 1.0,
        sy: 1.0,
        tilt: 0.0,
        hop: 0.0,
        mouth: 0.0,
        mouth_rect: MouthRect::default(),
        eye: UEye::Pill,
        look_x: 0.0,
        look_y: 0.0,
        file_visible: true,
        suck: 0.0,
        zone_over: false,
        zone_alpha: 1.0,
        text_alpha: 1.0,
        bar_reveal: 0.0,
        bar_alpha: 0.0,
        progress: 0.0,
        flash: 0.0,
        check: 0.0,
        green_wash: 0.0,
        choose_alpha: 0.0,
        prog_end: USC.t_prog_start + 2.4,
        grow_start: USC.t_prog_start + 2.4 + 0.25,
        grow_end: USC.t_prog_start + 2.4 + 0.7,
    }
}

// ── Engine ────────────────────────────────────────────────────────────────────

pub struct UploadSeq {
    pub upload_duration: f32,
    active: bool,
    cursor_x: f32,
    cursor_y: f32,
    entry_wall: f32,
    drop_wall: Option<f32>,
    t: f32,
    bx: Sp,
    by: Sp,
    tilt: f32,
    mouth: Sp,
    locked: bool,
    lock_at: f32,
    entered: f32,
    prev_x: f32,
    prev_y: f32,
    prev_t: f32,
    speed: f32,
}

impl Default for UploadSeq {
    fn default() -> Self {
        Self::new()
    }
}

impl UploadSeq {
    pub fn new() -> Self {
        Self {
            upload_duration: 2.4,
            active: false,
            cursor_x: 600.0,
            cursor_y: 280.0,
            entry_wall: 0.0,
            drop_wall: None,
            t: 0.0,
            bx: Sp::new(USC.rest_x),
            by: Sp::new(USC.rest_y),
            tilt: 0.0,
            mouth: Sp::new(0.0),
            locked: false,
            lock_at: -9.0,
            entered: -9.0,
            prev_x: 600.0,
            prev_y: 280.0,
            prev_t: 0.0,
            speed: 0.0,
        }
    }

    fn prog_end(&self) -> f32 {
        USC.t_prog_start + self.upload_duration
    }
    fn grow_start(&self) -> f32 {
        self.prog_end() + 0.25
    }
    fn grow_end(&self) -> f32 {
        self.prog_end() + 0.7
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// True once the file has been dropped (the post-drop timeline is running).
    pub fn dropped(&self) -> bool {
        self.drop_wall.is_some()
    }

    /// Seconds since the drop, or None while still dragging.
    pub fn since_drop(&self) -> Option<f32> {
        self.drop_wall.map(|d| now() - d)
    }

    /// A file entered the island. Coordinates are island points.
    pub fn enter_zone(&mut self, x: f32, y: f32) {
        let n = now();
        self.cursor_x = x;
        self.cursor_y = y;
        self.prev_x = x;
        self.prev_y = y;
        self.prev_t = n;
        self.speed = 0.0;

        self.t = USC.entry_t_ref;
        self.entered = USC.entry_t_ref;
        self.bx = Sp::new(USC.rest_x);
        self.by = Sp::new(USC.rest_y);
        self.tilt = 0.0;
        self.mouth = Sp::new(0.0);
        self.locked = false;
        self.lock_at = -9.0;

        self.entry_wall = n;
        self.drop_wall = None;
        self.active = true;
    }

    pub fn update_cursor(&mut self, x: f32, y: f32) {
        let n = now();
        let dt = n - self.prev_t;
        if dt > 0.001 {
            self.speed = ((x - self.prev_x).powi(2) + (y - self.prev_y).powi(2)).sqrt() / dt;
        }
        self.prev_x = x;
        self.prev_y = y;
        self.prev_t = n;
        self.cursor_x = x;
        self.cursor_y = y;
    }

    /// The island stays open per the spec, so leaving the zone changes nothing.
    pub fn exit_zone(&mut self) {}

    pub fn perform_drop(&mut self, duration: f32) {
        self.upload_duration = duration;
        self.drop_wall = Some(now());
        // Restart the canonical post-drop timeline however long the user hovered.
        // Spring state (position and velocity) is deliberately preserved.
        self.t = USC.t_drop;
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.drop_wall = None;
    }

    fn t_ref(&self) -> f32 {
        if !self.active {
            return 0.0;
        }
        let n = now();
        if let Some(d) = self.drop_wall {
            return USC.t_drop + (n - d).max(0.0);
        }
        USC.entry_t_ref + (n - self.entry_wall).max(0.0)
    }

    pub fn frame(&mut self) -> Frame {
        if !self.active {
            return rest_frame();
        }
        let t = self.t_ref();
        self.simulate_to(t);
        self.compute_frame(t)
    }

    fn simulate_to(&mut self, target: f32) {
        // A long stall must not spin here.
        if target - self.t > 2.0 {
            self.t = target - 2.0;
        }
        // f32 near t≈8 s has ~1e-6 resolution, so stop a hair short instead of
        // chasing the target; the cap is a belt-and-braces guard.
        let mut guard = 0;
        while self.t < target - 1e-4 && guard < 4_000 {
            let dt = USC.dt.min(target - self.t);
            self.step_once(dt);
            self.t += dt;
            guard += 1;
        }
    }

    fn step_once(&mut self, dt: f32) {
        let dragging = self.drop_wall.is_none();

        // Horizontal follow and lock, only while dragging inside the zone.
        if dragging && self.entered >= 0.0 {
            let dist = ((self.cursor_x - self.bx.v).powi(2) + (self.cursor_y + 14.0 - self.by.v).powi(2)).sqrt();
            if !self.locked && dist < USC.lock_in && self.speed < 180.0 {
                self.locked = true;
                self.lock_at = self.t;
            }
            if self.locked && dist > USC.lock_out {
                self.locked = false;
            }
            let tx = self.cursor_x.clamp(USC.follow_min, USC.follow_max);
            let response = if self.locked { 0.18 } else { 0.35 };
            let damping = if self.locked { 0.75 } else { 0.7 };
            self.bx.step(tx, response, damping, dt);
            self.by.step(USC.rest_y, response, damping, dt);
        }

        // Tilt follows how fast Mochi is sliding.
        let tilt_target = if dragging { (self.bx.vel * 0.0015).clamp(-0.18, 0.18) } else { 0.0 };
        self.tilt = lerp(self.tilt, tilt_target, 1.0 - 0.0005f32.powf(dt));

        // Mouth. The post-drop close only runs once the drop has actually happened.
        let t = self.t;
        if !dragging && t >= USC.t_suck_end {
            self.mouth.v = lerp(USC.mouth_max, 0.0, e_in(seg(t, USC.t_suck_end, USC.t_close_end))).max(0.0);
        } else {
            let mut mt = 0.0;
            if self.entered >= 0.0 {
                mt = if self.locked || !dragging { USC.mouth_open } else { USC.mouth_ajar };
            }
            if !dragging && t < USC.t_suck_end {
                mt = USC.mouth_max;
            }
            self.mouth.step(mt, 0.25, 0.6, dt);
            if self.mouth.v < 0.0 {
                self.mouth.v = 0.0;
            }
        }
    }

    fn compute_frame(&self, t: f32) -> Frame {
        let mut f = rest_frame();
        f.t = t;
        f.cursor_x = self.cursor_x;
        f.cursor_y = self.cursor_y;
        let prog_end = self.prog_end();
        let grow_start = self.grow_start();
        let grow_end = self.grow_end();
        f.prog_end = prog_end;
        f.grow_start = grow_start;
        f.grow_end = grow_end;

        let entered = if self.entered >= 0.0 { self.entered } else { 1e9 };
        let dragging = self.drop_wall.is_none();
        // Clamp the phase clock to just before the drop while still dragging, so a
        // long hover never trips the post-drop visuals.
        let pt = if dragging { t.min(USC.t_drop - USC.dt) } else { t };

        // Morph: 0→1 on entry, 1→0 shrinking to a ball, 0→1 growing back at choose.
        let morph = if pt < USC.t_chew_end {
            e_back(seg(pt, entered, entered + 0.38))
        } else if pt < grow_start {
            1.0 - e_out(seg(pt, USC.t_chew_end, USC.t_shrink_end))
        } else {
            e_back(seg(pt, grow_start, grow_end))
        };
        f.morph = morph.clamp(0.0, 1.08);

        // Position and diameter.
        let mut x = self.bx.v;
        let mut y = self.by.v;
        let mut d = USC.d_box;
        if pt >= USC.t_chew_end && pt < USC.t_prog_start {
            let k = e_in_out(seg(pt, USC.t_chew_end, USC.t_shrink_end));
            x = lerp(self.bx.v, USC.bar_x0, k);
            y = lerp(self.by.v, USC.bar_y, k);
            d = lerp(USC.d_box, 14.0, k);
        }
        if pt >= USC.t_prog_start {
            let p = progress_at(pt, USC.t_prog_start, prog_end);
            x = lerp(USC.bar_x0, USC.bar_x1, p);
            y = USC.bar_y;
            d = 14.0;
        }
        if pt >= prog_end {
            x = USC.bar_x1;
            y = USC.bar_y - 8.0 * (PI * seg(pt, prog_end, prog_end + 0.2)).sin();
        }
        if pt >= grow_start {
            let k = e_in_out(seg(pt, grow_start, grow_end));
            x = lerp(USC.bar_x1, USC.choose_x, k);
            y = lerp(USC.bar_y, USC.choose_y, k);
            d = lerp(14.0, USC.choose_d, e_back(seg(pt, grow_start, grow_end)));
        }
        f.x = x;
        f.y = y;
        f.d = d;

        // Squeeze.
        let (mut sx, mut sy) = (1.0f32, 1.0f32);
        if pt >= USC.t_drop && pt < USC.t_suck_start {
            let k = e_out(seg(pt, USC.t_drop, USC.t_suck_start));
            sy = lerp(1.0, 0.92, k);
            sx = lerp(1.0, 1.06, k);
        }
        if pt >= USC.t_suck_start && pt < USC.t_suck_end {
            let k = e_in_out(seg(pt, USC.t_suck_start, USC.t_suck_end));
            sy = lerp(0.92, 1.06, k);
            sx = lerp(1.06, 0.97, k);
        }
        if pt >= USC.t_suck_end && pt < USC.t_chew1 {
            sy = squeeze_y(pt);
            sx = squeeze_x(pt);
        }
        if pt >= USC.t_chew1 && pt < USC.t_chew_end {
            let k = ((pt - USC.t_chew1) % 0.14) / 0.14;
            sy = 1.0 - 0.05 * (PI * k).sin();
            sx = 1.0 + 0.03 * (PI * k).sin();
        }
        if pt >= USC.t_chew_end && pt < USC.t_shrink_end {
            let k = seg(pt, USC.t_chew_end, USC.t_shrink_end);
            sy = 1.0 + 0.12 * (PI * k).sin();
            sx = 1.0 - 0.06 * (PI * k).sin();
        }
        if pt >= USC.t_prog_start && pt < prog_end {
            let v = (progress_at(pt + 0.01, USC.t_prog_start, prog_end) - progress_at(pt, USC.t_prog_start, prog_end)) / 0.01;
            let st = (v * 0.18).clamp(0.0, 1.0);
            sx = 1.0 + 0.25 * st;
            sy = 1.0 - 0.15 * st;
        }
        if pt >= grow_start && pt < grow_end {
            sy = 1.0 + 0.06 * (PI * seg(pt, grow_start, grow_end)).sin();
        }
        f.sx = sx;
        f.sy = sy;
        f.tilt = self.tilt;
        f.hop = if self.lock_at > 0.0 && dragging {
            -5.0 * (PI * seg(pt, self.lock_at, self.lock_at + 0.15)).sin()
        } else {
            0.0
        };

        f.mouth = self.mouth.v;

        // Eyes.
        let mut eye = UEye::Pill;
        if self.locked && pt < USC.t_suck_end {
            eye = UEye::Cup;
        }
        if pt >= USC.t_suck_end && pt < USC.t_chew_end + 0.1 {
            eye = UEye::Content;
        }
        if pt >= prog_end && pt < grow_end + 0.3 {
            eye = UEye::Content;
        }
        f.eye = eye;

        let lkx = if pt < USC.t_suck_end { self.cursor_x - x } else if pt < USC.t_prog_start { 0.0 } else { 40.0 };
        let lky = if pt < USC.t_suck_end { self.cursor_y + 10.0 - y } else { 0.0 };
        f.look_x = (lkx / 200.0).clamp(-1.0, 1.0);
        f.look_y = (lky / 150.0).clamp(-1.0, 1.0);

        f.file_visible = pt < USC.t_suck_end;
        f.suck = seg(pt, USC.t_suck_start, USC.t_suck_end);

        // Content alphas.
        f.zone_over = self.entered >= 0.0 && pt < USC.t_chew_end;
        f.zone_alpha = 1.0 - seg(pt, USC.t_chew_end, USC.t_chew_end + 0.2);
        f.text_alpha = f.zone_alpha * if x > USC.text_x - 40.0 && dragging { 0.25 } else { 1.0 };
        f.bar_reveal = e_out(seg(pt, USC.t_bar_in, USC.t_bar_in + 0.25)) * (1.0 - seg(pt, grow_start, grow_start + 0.2));
        f.bar_alpha = seg(pt, USC.t_bar_in + 0.05, USC.t_bar_in + 0.25) * (1.0 - seg(pt, grow_start, grow_start + 0.2));
        f.progress = progress_at(pt, USC.t_prog_start, prog_end);
        f.flash = if pt >= prog_end { (PI * seg(pt, prog_end, prog_end + 0.3)).sin() } else { 0.0 };
        f.check = if pt >= prog_end { e_back(seg(pt, prog_end, prog_end + 0.25)) } else { 0.0 };

        let hover_green: f32 = if f.zone_over { 0.22 } else { 0.0 };
        let mut upload_green = 0.0;
        if pt >= USC.t_prog_start {
            let base = f.progress * 0.5;
            let extra = if pt >= prog_end { 0.2 * (PI * seg(pt, prog_end, prog_end + 0.4)).sin() } else { 0.0 };
            let fade_out = 1.0 - seg(pt, grow_end, grow_end + 0.6);
            upload_green = (base + extra) * fade_out;
        }
        f.green_wash = hover_green.max(upload_green);
        f.choose_alpha = seg(pt, grow_start + 0.15, grow_end);

        // Mouth rect in island coordinates — the file is clipped against it.
        let r = f.d / 2.0 / 1.04;
        let mc = f.morph.clamp(0.0, 1.0);
        let rx = r * (1.04 - 0.04 * mc);
        let ry = r * (0.97 - 0.03 * mc);
        let mh = f.mouth * r * mc;
        let mw = 2.0 * rx - 0.24 * r;
        f.mouth_rect = MouthRect {
            x: x + (-mw / 2.0) * sx,
            y: y + f.hop + (-ry + 0.1 * r) * sy,
            w: mw * sx,
            h: mh * sy,
        };
        f
    }
}

// ── Canvas ────────────────────────────────────────────────────────────────────

const INK: &str = "#0E0F12";

impl App {
    /// Draws the whole drop sequence into the 640×176 island body.
    pub fn draw_upload_canvas(&mut self, g: &mut Gfx, wall: f32) {
        let f = self.upload.frame();

        // Card.
        g.save();
        g.clip_round_rect(USC.card_x, USC.card_y, USC.card_w, USC.card_h, USC.card_r);
        g.fill_style(hex("#0D0E10"));
        g.fill_rect(USC.card_x, USC.card_y, USC.card_w, USC.card_h);

        // Green glow, fanning up from the bottom edge of the card.
        if f.green_wash > 0.0 {
            let gx = USC.card_x + USC.card_w / 2.0;
            let gy = USC.card_y + USC.card_h;
            let grad = g.radial(gx, gy, 0.0, USC.card_h * 1.5, &[
                (0.0, rgba(40, 212, 130, f.green_wash * 0.9)),
                (0.55, rgba(40, 212, 130, f.green_wash * 0.3)),
                (1.0, rgba(40, 212, 130, 0.0)),
            ]);
            g.fill_style(grad);
            g.fill_rect(USC.card_x, USC.card_y, USC.card_w, USC.card_h);
        }
        g.restore();

        // Dashed border, marching left to right at ~20 pt/s.
        if f.zone_alpha > 0.0 {
            g.save();
            g.mul_alpha(f.zone_alpha);
            g.stroke_style(if f.zone_over { rgba(52, 212, 153, 0.55) } else { rgba(255, 255, 255, 0.14) });
            if let Some(p) = rounded_rect_path(
                USC.card_x + 0.75, USC.card_y + 0.75, USC.card_w - 1.5, USC.card_h - 1.5, USC.card_r - 0.5,
            ) {
                let sw = tiny_skia::Stroke {
                    width: 1.5,
                    dash: tiny_skia::StrokeDash::new(vec![6.0, 5.0], (-wall * 20.0).rem_euclid(11.0)),
                    ..tiny_skia::Stroke::default()
                };
                g.stroke_path_with(&p, &sw);
            }
            g.restore();
        }

        if f.zone_alpha > 0.0 && f.text_alpha > 0.0 {
            draw_drop_text(g, &f);
        }
        if f.bar_alpha > 0.0 || f.bar_reveal > 0.0 {
            self.draw_progress_bar(g, &f);
        }
        if f.choose_alpha > 0.0 {
            self.draw_choose(g, &f);
        }
        draw_mochi(g, &f, wall);
        if f.file_visible {
            draw_file(g, &f);
        }
    }

    fn draw_progress_bar(&self, g: &mut Gfx, f: &Frame) {
        g.save();
        g.mul_alpha(f.bar_alpha.max(0.001));
        let (x0, x1, by) = (USC.bar_x0, USC.bar_x1, USC.bar_y);
        let bar_len = (x1 - x0) * f.bar_reveal;
        let name = self.st.dropped_file.as_ref().map(|d| d.name.clone()).unwrap_or_else(|| "file".into());
        let name = text::ellipsize(&name, Face::Medium, 12.5, 380.0);
        text::draw(g, &format!("Uploading {name}"), x0, by - 30.0, Face::Medium, 12.5, hex("#A9ADB5"), Align::Left);

        if f.check > 0.0 {
            g.save();
            g.translate(x1 - 8.0, by - 30.0);
            g.scale_xy(f.check, f.check);
            g.fill_style(hex("#34D399"));
            g.circle(0.0, 0.0, 8.0);
            g.fill();
            g.begin_path();
            g.move_to(-3.6, 0.2);
            g.line_to(-1.0, 2.8);
            g.line_to(3.8, -2.6);
            g.stroke_style(hex("#07130E"));
            g.line_width(2.0);
            g.line_cap_round();
            g.line_join_round();
            g.stroke();
            g.restore();
        } else {
            text::draw(g, &format!("{} %", (f.progress * 100.0).round() as i32), x1, by - 30.0, Face::Medium, 12.5, hex("#A9ADB5"), Align::Right);
        }

        // Track.
        if bar_len > 0.0 {
            g.fill_style(rgba(255, 255, 255, 0.08));
            g.fill_round_rect(x0, by - 3.0, bar_len, 6.0, 3.0);
        }

        // Fill.
        let fx = lerp(x0, x1, f.progress);
        if fx > x0 + 1.0 {
            let flash = rgb(
                lerp(52.0, 110.0, f.flash).round() as u8,
                lerp(211.0, 231.0, f.flash).round() as u8,
                lerp(153.0, 183.0, f.flash).round() as u8,
            );
            let grad = g.linear(x0, 0.0, fx, 0.0, &[(0.0, hex("#1FA87A")), (1.0, flash)]);
            g.fill_style(grad);
            g.fill_round_rect(x0, by - 3.0, fx - x0, 6.0, 3.0);
        }

        // Glow trail, its length driven by how fast the bar is moving.
        if f.progress > 0.01 && f.progress < 1.0 {
            let v = (progress_at(f.t + 0.01, USC.t_prog_start, f.prog_end) - progress_at(f.t, USC.t_prog_start, f.prog_end)) / 0.01;
            let tl = (8.0 + v * 40.0).clamp(8.0, 34.0);
            let grad = g.linear(fx - tl, 0.0, fx, 0.0, &[(0.0, rgba(52, 212, 153, 0.0)), (1.0, rgba(110, 231, 183, 0.6))]);
            g.fill_style(grad);
            g.fill_round_rect(fx - tl, by - 4.0, tl, 8.0, 4.0);
        }
        g.restore();
    }

    fn draw_choose(&mut self, g: &mut Gfx, f: &Frame) {
        g.save();
        g.mul_alpha(f.choose_alpha);
        g.translate(0.0, (1.0 - f.choose_alpha) * 4.0);

        let name = self.st.dropped_file.as_ref().map(|d| d.name.clone()).unwrap_or_else(|| "file".into());
        let nm = text::ellipsize(&name, Face::Bold, 14.0, 420.0);
        text::draw(g, &format!("{nm} is ready."), 114.0, 80.0, Face::Medium, 14.0, hex("#F5F6F8"), Align::Left);
        text::draw(g, "What do you want to do with it?", 114.0, 100.0, Face::Regular, 12.5, hex("#9398A1"), Align::Left);

        let live = f.choose_alpha > 0.5;
        let ask_r = Rect::new(114.0, 113.0, 168.0, 26.0);
        let can_r = Rect::new(290.0, 113.0, 120.0, 26.0);
        let (ask, ah, _) = if live { self.ui.click_region(id_of("choose-ask", 29), ask_r) } else { (false, false, false) };
        let (cancel, ch, _) = if live { self.ui.click_region(id_of("choose-cancel", 31), can_r) } else { (false, false, false) };

        g.fill_style(if ah { hex("#FFFFFF") } else { hex("#F5F6F8") });
        g.fill_round_rect(ask_r.x, ask_r.y, ask_r.w, ask_r.h, 13.0);
        text::draw(g, "Ask a question about it", 198.0, 126.0, Face::Medium, 12.5, hex("#0B0C0E"), Align::Center);

        g.fill_style(rgba(255, 255, 255, if ch { 0.15 } else { 0.09 }));
        g.fill_round_rect(can_r.x, can_r.y, can_r.w, can_r.h, 13.0);
        text::draw(g, "Cancel", 350.0, 126.0, Face::Medium, 12.5, hex("#F1F2F4"), Align::Center);
        g.restore();

        if ask {
            self.set_view(View::Prompt);
        } else if cancel {
            let v = self.st.default_view();
            self.set_view(v);
        }
    }
}

fn draw_drop_text(g: &mut Gfx, f: &Frame) {
    g.save();
    g.mul_alpha(f.text_alpha);
    text::draw(g, "Drop your files here", USC.text_x, USC.text_y - 4.0, Face::Medium, 13.0, hex("#D5D7DB"), Align::Left);
    let mut cx = USC.text_x;
    for chip in ["PDF", "Images", "Code", "Docs"] {
        // The macOS port measures chips the same rough way, so the row lines up.
        let w = chip.len() as f32 * 6.5 + 16.0;
        g.fill_style(rgba(255, 255, 255, 0.07));
        g.fill_round_rect(cx, USC.text_y + 9.0, w, 18.0, 9.0);
        text::draw(g, chip, cx + 8.0, USC.text_y + 18.0, Face::Medium, 11.0, hex("#B9BDC4"), Align::Left);
        cx += w + 6.0;
    }
    g.restore();
}

/// Superellipse body — port of usBodyPath(m, R).
fn body_path(m: f32, r: f32) -> (tiny_skia::Path, f32, f32) {
    let mc = m.clamp(0.0, 1.0);
    let n = 2.15 + (5.5 - 2.15) * mc;
    let rx = r * (1.04 - 0.04 * mc);
    let ry = r * (0.97 - 0.03 * mc);
    let mut pb = tiny_skia::PathBuilder::new();
    for i in 0..=96 {
        let a = i as f32 / 96.0 * TAU;
        let (sa, ca) = a.sin_cos();
        let px = rx * ca.signum() * ca.abs().powf(2.0 / n);
        let py = ry * sa.signum() * sa.abs().powf(2.0 / n);
        if i == 0 { pb.move_to(px, py) } else { pb.line_to(px, py) }
    }
    pb.close();
    (pb.finish().expect("body"), rx, ry)
}

fn draw_mochi(g: &mut Gfx, f: &Frame, wall: f32) {
    let r = f.d / 2.0 / 1.04;
    if !(r > 0.4) {
        return;
    }
    let mc = f.morph.clamp(0.0, 1.0);

    g.save();
    g.translate(f.x, f.y + f.hop);
    g.rotate(f.tilt);
    g.scale_xy(f.sx, f.sy);

    let (body, rx, ry) = body_path(f.morph, r);
    let dress = 1.0 - (mc * 2.2).min(1.0);
    let yaw = f.look_x * 0.5;
    let pitch = -f.look_y * 0.4;

    draw_back(g, r, rx, ry, &BackOpts { vis: dress, beat: 0.0, yaw });
    fill_helmet(g, &body, r, rx, ry, None);
    let plate = face_plate(r, yaw, pitch, mc);
    fill_plate(g, &plate, r, None);
    draw_cheeks(g, r, yaw, 0.6, mc);

    // Everything below is cut off at the body outline.
    g.save();
    g.clip_path(&body);

    // Top rim, once Mochi is box-shaped enough to have one.
    if mc > 0.3 {
        let a = ((mc - 0.3) / 0.7).clamp(0.0, 1.0);
        g.begin_path();
        g.move_to(-rx * 0.72, -ry + 0.9);
        g.line_to(rx * 0.72, -ry + 0.9);
        g.stroke_style(rgba(255, 255, 255, 0.45 * a));
        g.line_width(1.2);
        g.line_cap_round();
        g.stroke();
    }

    // Mouth hole.
    let mh = f.mouth * r * mc;
    if mh > 0.3 {
        let mw = 2.0 * rx - 0.24 * r;
        let mx = -mw / 2.0;
        let my = -ry + 0.1 * r;
        let grad = g.linear(0.0, my, 0.0, my + mh, &[(0.0, hex("#030304")), (1.0, hex("#101114"))]);
        g.fill_style(grad);
        g.round_rect(mx, my, mw, mh, (mw / 2.0).min(mh / 2.0));
        g.fill();
        if mh > 4.0 {
            let rr = (mw / 2.0).min(mh / 2.0);
            g.begin_path();
            g.move_to(mx + rr, my + mh + 0.5);
            g.line_to(mx + mw - rr, my + mh + 0.5);
            g.stroke_style(rgba(255, 255, 255, 0.55));
            g.line_width(1.0);
            g.line_cap_round();
            g.stroke();
        }
    }

    // Eyes.
    let ew = r * 0.2;
    let eh = r * (0.34 - 0.08 * mc);
    let ey = r * (0.12 + 0.2 * mc);
    let sp = r * 0.42;
    let lx = f.look_x * r * (0.2 - 0.06 * mc);
    let ly = f.look_y * r * (0.1 - 0.05 * mc);
    for sd in [-1.0f32, 1.0] {
        g.save();
        g.translate(sd * sp + lx, ey + ly);
        draw_eye(g, f.eye, ew, eh);
        g.restore();
    }
    g.restore(); // body clip

    draw_front(g, r, &FrontOpts {
        lift: mc * 0.58, yaw,
        lit: [0.0; 5], glyph: None, color: [0.231, 0.62, 1.0], t: wall,
        mini: false, wobble: 0.0,
    });
    g.restore();
}

fn draw_eye(g: &mut Gfx, shape: UEye, w: f32, h: f32) {
    match shape {
        UEye::Pill => {
            g.fill_style(hex(INK));
            g.round_rect(-w / 2.0, -h / 2.0, w, h, w / 2.0);
            g.fill();
        }
        UEye::Cup => {
            // Flat top, semicircular bottom.
            let hh = h * 0.55;
            g.begin_path();
            g.move_to(-w / 2.0, -hh / 2.0);
            g.line_to(w / 2.0, -hh / 2.0);
            g.line_to(w / 2.0, hh / 2.0 - w / 2.0);
            g.arc(0.0, hh / 2.0 - w / 2.0, w / 2.0, 0.0, PI, false);
            g.close_path();
            g.fill_style(hex(INK));
            g.fill();
        }
        UEye::Content => {
            g.begin_path();
            g.arc(0.0, -h * 0.12, w * 0.85, PI * 0.15, PI * 0.85, false);
            g.stroke_style(hex(INK));
            g.line_width(w * 0.5);
            g.line_cap_round();
            g.stroke();
        }
    }
}

/// The generic sheet with a folded corner.
fn draw_doc(g: &mut Gfx, cx: f32, cy: f32, wsc: f32, hsc: f32) {
    let w = 34.0 * wsc;
    let h = 42.0 * hsc;
    let (x, y) = (cx - w / 2.0, cy - h / 2.0);
    let fold = 8.0 * wsc.min(hsc);

    let outline = |g: &mut Gfx, dy: f32| {
        g.begin_path();
        g.move_to(x + 2.0, y + dy);
        g.line_to(x + w - fold, y + dy);
        g.line_to(x + w, y + fold + dy);
        g.line_to(x + w, y + h - 2.0 + dy);
        g.quad_to(x + w, y + h + dy, x + w - 2.0, y + h + dy);
        g.line_to(x + 2.0, y + h + dy);
        g.quad_to(x, y + h + dy, x, y + h - 2.0 + dy);
        g.line_to(x, y + 2.0 + dy);
        g.quad_to(x, y + dy, x + 2.0, y + dy);
        g.close_path();
    };
    // Cheap drop shadow: two soft offset copies.
    for (dy, a) in [(4.0, 0.12), (3.0, 0.16)] {
        outline(g, dy);
        g.fill_style(rgba(0, 0, 0, a));
        g.fill();
    }
    outline(g, 0.0);
    g.fill_style(hex("#F4F4F6"));
    g.fill();

    g.begin_path();
    g.move_to(x + w - fold, y);
    g.line_to(x + w - fold, y + fold);
    g.line_to(x + w, y + fold);
    g.close_path();
    g.fill_style(hex("#D5D6DB"));
    g.fill();

    g.fill_style(hex("#3B82F5"));
    g.fill_round_rect(x + w * 0.18, y + h * 0.58, w * 0.64, h * 0.16, 2.0);
}

fn draw_file(g: &mut Gfx, f: &Frame) {
    let cx = f.cursor_x;
    let cy = f.cursor_y + 14.0;

    if f.suck <= 0.0 {
        g.save();
        g.mul_alpha(0.92);
        draw_doc(g, cx, cy, 1.0, 1.0);
        g.restore();
        return;
    }

    let m = f.mouth_rect;
    let (w0, h0) = (34.0, 42.0);
    let p = e_in(f.suck);
    let top_y = lerp(cy - h0 / 2.0, m.y - 2.0, e_in_out(f.suck));
    let hs = lerp(1.08, 0.55, e_in_out(f.suck));
    let hh_total = h0 * hs;
    let sc = lerp(1.0, 0.55, p);
    let q = e_out(f.suck);
    let f_cx = lerp(cx, m.x + m.w / 2.0, e_out(f.suck));
    let wob = (f.suck * TAU).sin() * 0.1 * (1.0 - p);
    let clip_y = m.y + m.h * 0.5;

    // The sheet is drawn as 28 horizontal strips, each narrowed towards the mouth,
    // so the page appears to funnel in. Everything below the mouth line is clipped
    // away — that is what makes it look swallowed.
    g.save();
    g.clip_rect_xywh(0.0, 0.0, USC.w, clip_y.max(0.0));
    for i in 0..28 {
        let v0 = i as f32 / 28.0;
        let wsc = lerp(1.0, lerp(0.92, 0.22 * m.w / w0, v0.powf(1.2)), q) * sc;
        let yy = top_y + v0 * hh_total;
        let hh = hh_total / 28.0 + 0.6;

        g.save();
        g.translate(f_cx, yy);
        g.rotate(wob);
        g.clip_rect_xywh(-w0 * wsc / 2.0, 0.0, w0 * wsc, hh);
        g.translate(-f_cx, -yy);
        draw_doc(g, f_cx, top_y + hh_total / 2.0, wsc, hs);
        g.restore();
    }
    g.restore();

    // Green crumbs pulled in with the file.
    for i in 0..4 {
        let a = i as f32 / 4.0 * TAU + 0.6;
        let k = ((f.suck - i as f32 * 0.08) / 0.7).clamp(0.0, 1.0);
        if k <= 0.0 || k >= 1.0 {
            continue;
        }
        let (sx0, sy0) = (cx + a.cos() * 24.0, cy + a.sin() * 24.0);
        let (ex, ey) = (m.x + m.w / 2.0, m.y + m.h * 0.3);
        let kk = k.powf(0.7);
        let px = lerp(sx0, ex, kk);
        let py = lerp(sy0, ey, kk) - (PI * k).sin() * 6.0;
        let rad = 2.2 * (1.0 - k * 0.5);
        g.fill_style(with_alpha(rgba(52, 212, 153, 1.0), 1.0 - k));
        g.circle(px, py, rad);
        g.fill();
    }
}


