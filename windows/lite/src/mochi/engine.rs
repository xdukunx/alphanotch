#![allow(dead_code)] // the emote / state API is kept whole, like the TS source it ports

// Mochi — port of src/mochi/engine.ts (itself a port of BotEngine.swift).
// Same constants, same tweens, same easings, same particles.
//
// Two differences from the browser version, both on purpose:
//  * no setTimeout: delayed actions sit in a small queue the update loop drains;
//  * sounds are not played here — they are queued and the app drains them, which
//    keeps the engine free of side effects (and testable offscreen).

use std::collections::HashMap;
use std::f32::consts::{PI, TAU};

use tiny_skia::{Color, Path, PathBuilder};

use crate::anim::{ease, lerp, now, EaseFn};
use crate::gfx::{frgb, hex, rgb, rgba, Gfx};
use crate::mochi::character::{
    self, draw_back, draw_cheeks, draw_front, face_plate, fill_helmet, fill_plate, BackOpts,
    FrontOpts, Rgb,
};
use crate::text::{self, Align, Face};

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EyeShape {
    Pill, Wide, Dot, Line, Flat, Happy, Closed, Spiral, Heart, Star, Tired, Wink, Cup,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum BotState {
    Idle, Working, Thinking, Searching, Approval, Question, Error, Finished, RateLimit, Sleeping, Dizzy,
}

impl BotState {
    pub fn as_str(self) -> &'static str {
        match self {
            BotState::Idle => "idle",
            BotState::Working => "working",
            BotState::Thinking => "thinking",
            BotState::Searching => "searching",
            BotState::Approval => "approval",
            BotState::Question => "question",
            BotState::Error => "error",
            BotState::Finished => "finished",
            BotState::RateLimit => "ratelimit",
            BotState::Sleeping => "sleeping",
            BotState::Dizzy => "dizzy",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "idle" => Self::Idle,
            "working" => Self::Working,
            "thinking" => Self::Thinking,
            "searching" => Self::Searching,
            "approval" => Self::Approval,
            "question" => Self::Question,
            "error" => Self::Error,
            "finished" => Self::Finished,
            "ratelimit" => Self::RateLimit,
            "sleeping" => Self::Sleeping,
            "dizzy" => Self::Dizzy,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Emote {
    Love, Surprised, Proud, Wink, Yawn, Happy, Annoyed,
}

impl Emote {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "love" => Self::Love,
            "surprised" => Self::Surprised,
            "proud" => Self::Proud,
            "wink" => Self::Wink,
            "yawn" => Self::Yawn,
            "happy" => Self::Happy,
            "annoyed" => Self::Annoyed,
            _ => return None,
        })
    }

    fn eye(self) -> EyeShape {
        match self {
            Emote::Love => EyeShape::Heart,
            Emote::Surprised => EyeShape::Dot,
            Emote::Proud => EyeShape::Star,
            Emote::Wink => EyeShape::Wink,
            Emote::Yawn => EyeShape::Tired,
            Emote::Happy => EyeShape::Happy,
            Emote::Annoyed => EyeShape::Line,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum BadgeKind {
    Dots, Bang, Question, Dot,
}

#[derive(Clone, Copy, PartialEq)]
pub struct Badge {
    pub kind: BadgeKind,
    pub color: Rgb,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Prop {
    Yaw, Pitch, Roll, Tilt, Open, Sx, Sy, Oy, Ox, Tint, Morph, Hands, Blush, Es, BadgeS,
}

type Key = (f32, f32, EaseFn); // target, duration (s), ease

struct Tween {
    keys: Vec<Key>,
    index: usize,
    from: f32,
    start: f32,
    on_done: Done,
}

#[derive(Clone, Copy)]
enum Done {
    None,
    ResetRoll,
}

#[derive(Clone, Copy)]
struct StateCfg {
    color: Rgb,
    tint: f32,
    eye: EyeShape,
    badge: Option<Badge>,
    bounces: bool,
    scans: bool,
    breathes: bool,
    zz: bool,
    sweat: bool,
    look: Option<(f32, f32)>,
    tilt: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum PKind {
    Heart, Star, Spark, Sweat, Z,
}

struct Particle {
    kind: PKind,
    x: f32, y: f32, vx: f32, vy: f32,
    age: f32, life: f32, rot: f32, size: f32,
}

/// Things setTimeout used to do.
#[derive(Clone, Copy)]
enum Action {
    EmitSpark(i32),
    CommitBadge(u32),
    SlotClose,
    EndChew,
    Sound(&'static str),
    Blink(u32),
    HandsUp(u32),
    GreetBlink(u32),
    GreetHandsDown(u32),
    GreetHappy(u32),
    YawnClose,
}

// ── Constants ─────────────────────────────────────────────────────────────────

const EYE_W: f32 = 0.2;
const EYE_H: f32 = 0.34;
const EYE_SP: f32 = 0.37;
const EYE_P: f32 = -0.12;
const INK: &str = "#1A1412";
const MINI_INK: &str = "#10131A";

const fn c(r: f32, g: f32, b: f32) -> Rgb {
    [r, g, b]
}

pub mod colors {
    use super::{c, Rgb};
    pub const IDLE: Rgb = c(0.902, 0.914, 0.933);
    pub const WORKING: Rgb = c(0.231, 0.62, 1.0);
    pub const THINKING: Rgb = c(0.545, 0.361, 0.965);
    pub const SEARCHING: Rgb = c(0.388, 0.396, 0.949);
    pub const APPROVAL: Rgb = c(0.961, 0.647, 0.141);
    pub const QUESTION: Rgb = c(0.133, 0.827, 0.933);
    pub const ERROR: Rgb = c(0.957, 0.314, 0.369);
    pub const FINISHED: Rgb = c(0.204, 0.831, 0.6);
    pub const RATELIMIT: Rgb = c(0.984, 0.573, 0.235);
    pub const SLEEPING: Rgb = c(0.58, 0.635, 0.722);
    pub const DIZZY: Rgb = c(0.957, 0.447, 0.714);
}

fn state_cfg(s: BotState) -> StateCfg {
    use colors::*;
    let base = StateCfg {
        color: IDLE, tint: 0.0, eye: EyeShape::Pill, badge: None,
        bounces: false, scans: false, breathes: false, zz: false, sweat: false,
        look: None, tilt: 0.0,
    };
    let badge = |kind, color| Some(Badge { kind, color });
    match s {
        BotState::Idle => StateCfg { tilt: -0.05, ..base },
        BotState::Working => StateCfg { color: WORKING, tint: 0.72, badge: badge(BadgeKind::Dots, WORKING), ..base },
        BotState::Thinking => StateCfg { color: THINKING, tint: 0.72, badge: badge(BadgeKind::Dots, THINKING), look: Some((0.55, 0.55)), ..base },
        BotState::Searching => StateCfg { color: SEARCHING, tint: 0.72, badge: badge(BadgeKind::Dots, SEARCHING), scans: true, ..base },
        BotState::Approval => StateCfg { color: APPROVAL, tint: 0.78, eye: EyeShape::Wide, badge: badge(BadgeKind::Bang, APPROVAL), bounces: true, ..base },
        BotState::Question => StateCfg { color: QUESTION, tint: 0.75, badge: badge(BadgeKind::Question, QUESTION), tilt: 0.17, ..base },
        BotState::Error => StateCfg { color: ERROR, tint: 0.78, eye: EyeShape::Flat, badge: badge(BadgeKind::Dot, ERROR), ..base },
        BotState::Finished => StateCfg { color: FINISHED, tint: 0.35, eye: EyeShape::Happy, badge: badge(BadgeKind::Dot, FINISHED), ..base },
        BotState::RateLimit => StateCfg { color: RATELIMIT, tint: 0.72, eye: EyeShape::Tired, badge: badge(BadgeKind::Dot, RATELIMIT), sweat: true, ..base },
        BotState::Sleeping => StateCfg { color: SLEEPING, tint: 0.32, eye: EyeShape::Closed, breathes: true, zz: true, ..base },
        BotState::Dizzy => StateCfg { color: DIZZY, tint: 0.7, eye: EyeShape::Spiral, ..base },
    }
}

/// State → sound, as in BotStateCfg.sound.
pub fn state_sound(s: BotState) -> Option<&'static str> {
    Some(match s {
        BotState::Working => "work",
        BotState::Thinking => "think",
        BotState::Searching => "search",
        BotState::Approval => "approval",
        BotState::Question => "question",
        BotState::Error => "error",
        BotState::Finished => "finish",
        BotState::RateLimit => "rate",
        BotState::Sleeping => "sleep",
        BotState::Dizzy => "dizzy",
        _ => return None,
    })
}

fn mix3(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)]
}

pub fn hex_to_rgb(h: &str) -> Rgb {
    let v = u32::from_str_radix(h.trim_start_matches('#'), 16).unwrap_or(0);
    [((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0]
}

// ── Engine ────────────────────────────────────────────────────────────────────

pub struct BotEngine {
    pub is_mini: bool,
    /// Solid body colour for mini bots / integration pills (None = Mochi plum).
    pub body_color: Option<Rgb>,

    // Animated state (BotEngine `s`)
    yaw: f32, pitch: f32, roll: f32, tilt: f32, open: f32,
    sx: f32, sy: f32, oy: f32, ox: f32,
    tint: f32, pub morph: f32, hands: f32, blush: f32, es: f32, badge_s: f32,

    // Targets
    tg_yaw: f32, tg_pitch: f32, tg_tilt: f32, tg_sy: f32, tg_sx: f32,
    pub tg_es: f32,

    /// Extra canvas height above the body so hearts can fly out without clipping.
    pub particle_overhang: f32,

    // Mouth spring (fraction of R)
    pub slot_h: f32, pub slot_h_target: f32, slot_h_vel: f32, is_chewing: bool,

    // Crown medallions (the "dots"), tassel spring and headphone beat.
    medal_lit: [f32; 5],
    swing: f32, swing_vel: f32, beat: f32, wobble: f32,
    prev_ox: f32, prev_yaw: f32,

    col: Rgb,
    col_t: Rgb,

    pub state: BotState,
    cfg: StateCfg,

    eye_override: Option<EyeShape>,
    eye_override_until: f32,
    pub permanent_eye: Option<EyeShape>,
    permanent_emote: Option<Emote>,
    mini_next_behavior: f32,

    badge: Option<Badge>,
    badge_key: i64,
    badge_token: u32,
    pending_badge: Option<Option<Badge>>,

    tweens: HashMap<Prop, Tween>,
    particles: Vec<Particle>,
    delayed: Vec<(f32, Action)>,

    pub look_x: f32,
    pub look_y: f32,

    t0: f32,
    next_blink: f32,
    wave_until: f32,
    wave_start: f32,
    greet_token: u32,
    last_ambient: f32,
    slap_times: Vec<f32>,
    mini_look: (f32, f32),
    mini_look_next: f32,
    rng: u64,

    /// Fired when three slaps land inside 1.7 s (→ dizzy + confused view).
    pub dizzy_triggered: bool,
    /// Sounds queued since the last drain.
    sounds: Vec<&'static str>,
}

impl Default for BotEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl BotEngine {
    pub fn new() -> Self {
        let n = now();
        let mut e = Self {
            is_mini: false, body_color: None,
            yaw: 0.0, pitch: 0.0, roll: 0.0, tilt: 0.0, open: 1.0,
            sx: 1.0, sy: 1.0, oy: 0.0, ox: 0.0,
            tint: 0.0, morph: 0.0, hands: 0.0, blush: 0.0, es: 1.0, badge_s: 0.0,
            tg_yaw: 0.0, tg_pitch: 0.0, tg_tilt: 0.0, tg_sy: 1.0, tg_sx: 1.0, tg_es: 1.0,
            particle_overhang: 0.0,
            slot_h: 0.0, slot_h_target: 0.0, slot_h_vel: 0.0, is_chewing: false,
            medal_lit: [0.0; 5], swing: 0.0, swing_vel: 0.0, beat: 0.0, wobble: 0.0,
            prev_ox: 0.0, prev_yaw: 0.0,
            col: colors::IDLE, col_t: colors::IDLE,
            state: BotState::Idle, cfg: state_cfg(BotState::Idle),
            eye_override: None, eye_override_until: 0.0, permanent_eye: None, permanent_emote: None,
            mini_next_behavior: 0.0,
            badge: None, badge_key: -1, badge_token: 0, pending_badge: None,
            tweens: HashMap::new(), particles: Vec::new(), delayed: Vec::new(),
            look_x: 0.0, look_y: 0.0,
            t0: n, next_blink: n + 1.8, wave_until: 0.0, wave_start: 0.0, greet_token: 0,
            last_ambient: 0.0, slap_times: Vec::new(), mini_look: (0.0, 0.0), mini_look_next: 0.0,
            rng: 0x9E37_79B9_7F4A_7C15 ^ (n.to_bits() as u64),
            dizzy_triggered: false, sounds: Vec::new(),
        };
        e.tg_tilt = e.cfg.tilt;
        e.tilt = e.cfg.tilt;
        e
    }

    fn rnd(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        ((self.rng >> 40) as f32) / ((1u64 << 24) as f32)
    }

    fn get(&self, p: Prop) -> f32 {
        match p {
            Prop::Yaw => self.yaw, Prop::Pitch => self.pitch, Prop::Roll => self.roll,
            Prop::Tilt => self.tilt, Prop::Open => self.open, Prop::Sx => self.sx,
            Prop::Sy => self.sy, Prop::Oy => self.oy, Prop::Ox => self.ox,
            Prop::Tint => self.tint, Prop::Morph => self.morph, Prop::Hands => self.hands,
            Prop::Blush => self.blush, Prop::Es => self.es, Prop::BadgeS => self.badge_s,
        }
    }

    fn set(&mut self, p: Prop, v: f32) {
        match p {
            Prop::Yaw => self.yaw = v, Prop::Pitch => self.pitch = v, Prop::Roll => self.roll = v,
            Prop::Tilt => self.tilt = v, Prop::Open => self.open = v, Prop::Sx => self.sx = v,
            Prop::Sy => self.sy = v, Prop::Oy => self.oy = v, Prop::Ox => self.ox = v,
            Prop::Tint => self.tint = v, Prop::Morph => self.morph = v, Prop::Hands => self.hands = v,
            Prop::Blush => self.blush = v, Prop::Es => self.es = v, Prop::BadgeS => self.badge_s = v,
        }
    }

    fn locked(&self, p: Prop) -> bool {
        self.tweens.contains_key(&p)
    }

    fn anim(&mut self, p: Prop, keys: &[(f32, f32, EaseFn)]) {
        self.anim_done(p, keys, Done::None);
    }

    /// Keys carry their duration in milliseconds, like the TS source.
    fn anim_done(&mut self, p: Prop, keys: &[(f32, f32, EaseFn)], done: Done) {
        let keys = keys.iter().map(|(t, ms, e)| (*t, ms / 1000.0, *e)).collect();
        let from = self.get(p);
        self.tweens.insert(p, Tween { keys, index: 0, from, start: now(), on_done: done });
    }

    fn later(&mut self, secs: f32, a: Action) {
        self.delayed.push((now() + secs, a));
    }

    /// A one-off animation (tween, particles, queued action) is playing: worth full frame rate.
    pub fn in_transition(&self) -> bool {
        !self.tweens.is_empty() || !self.particles.is_empty() || !self.delayed.is_empty()
    }

    /// The idle blink is due: the host should run a few frames.
    pub fn blink_due(&self) -> bool {
        now() > self.next_blink
    }

    pub fn take_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sounds)
    }

    fn play(&mut self, name: &'static str) {
        self.sounds.push(name);
    }

    // ── Public API ────────────────────────────────────────────────────────────

    pub fn set_state(&mut self, next: BotState, force: bool) {
        if self.state == next && !force {
            return;
        }
        let prev = self.state;
        self.state = next;
        self.cfg = state_cfg(next);
        self.col_t = self.cfg.color;
        if !self.locked(Prop::Tint) {
            self.tint = self.cfg.tint;
        }
        if !self.locked(Prop::Tilt) {
            self.tg_tilt = self.cfg.tilt;
        }
        self.set_badge(self.cfg.badge);

        match next {
            BotState::Finished => {
                self.do_roll(950.0, 1.0);
                self.later(0.5, Action::EmitSpark(5));
            }
            BotState::Error => self.anim(Prop::Ox, &[
                (0.08, 50.0, ease::out), (-0.08, 70.0, ease::in_out),
                (0.05, 70.0, ease::in_out), (0.0, 90.0, ease::out),
            ]),
            BotState::Approval => self.anim(Prop::Oy, &[(-0.2, 150.0, ease::out), (0.0, 300.0, ease::back)]),
            BotState::Dizzy => self.do_roll(1300.0, 2.0),
            BotState::Question => self.blink(),
            BotState::RateLimit => self.emit(PKind::Sweat, 1),
            _ => {
                if prev != BotState::Idle || next != BotState::Idle {
                    self.blink();
                }
            }
        }
    }

    pub fn set_badge(&mut self, b: Option<Badge>) {
        let key = match b {
            Some(b) => (b.kind as i64) * 1_000_000 + (b.color[0] * 255.0) as i64 * 10_000
                + (b.color[1] * 255.0) as i64 * 100 + (b.color[2] * 255.0) as i64 % 100,
            None => -2,
        };
        if key == self.badge_key {
            return;
        }
        self.badge_key = key;
        self.badge_token += 1;
        self.pending_badge = Some(b);
        self.anim(Prop::BadgeS, &[(0.0, 90.0, ease::in_out)]);
        let tok = self.badge_token;
        self.later(0.1, Action::CommitBadge(tok));
    }

    pub fn blink(&mut self) {
        if self.locked(Prop::Open) {
            return;
        }
        self.anim(Prop::Open, &[(0.06, 70.0, ease::in_out), (1.0, 130.0, ease::out)]);
    }

    pub fn squash(&mut self) {
        self.anim(Prop::Sy, &[(0.78, 70.0, ease::out), (1.1, 130.0, ease::out), (1.0, 170.0, ease::in_out)]);
        self.anim(Prop::Sx, &[(1.16, 70.0, ease::out), (0.95, 130.0, ease::out), (1.0, 170.0, ease::in_out)]);
    }

    /// Mailbox swallow — opens the slot, chews, then closes.
    pub fn gulp(&mut self) {
        self.slot_h_target = 0.42;
        self.later(0.46, Action::SlotClose);
        self.anim(Prop::Sy, &[(0.78, 80.0, ease::out), (1.18, 130.0, ease::out), (1.0, 220.0, ease::back)]);
        self.anim(Prop::Sx, &[(1.28, 80.0, ease::out), (0.92, 130.0, ease::out), (1.0, 220.0, ease::back)]);
        self.blink();
    }

    pub fn slap(&mut self) {
        self.interrupt_greet();
        if self.state == BotState::Dizzy {
            return;
        }
        let t = now();
        self.slap_times.retain(|s| t - s < 1.7);
        self.slap_times.push(t);
        self.play("slap");
        self.squash();
        if self.slap_times.len() >= 3 {
            self.slap_times.clear();
            self.dizzy_triggered = true;
        } else {
            self.eye_override = Some(EyeShape::Line);
            self.eye_override_until = t + 0.8;
            self.later(0.06, Action::Sound("annoyed"));
        }
    }

    pub fn do_roll(&mut self, ms: f32, turns: f32) {
        self.roll = 0.0;
        self.anim_done(Prop::Roll, &[(TAU * turns, ms, ease::in_out)], Done::ResetRoll);
    }

    /// Peek wave — the "coucou". Timings from BotEngine.greet().
    pub fn greet(&mut self) {
        let t = now();
        self.greet_token += 1;
        let tok = self.greet_token;
        self.wave_start = t + 0.45;
        self.wave_until = t + 1.55;

        self.eye_override = Some(EyeShape::Happy);
        self.eye_override_until = t + 2.0;
        self.anim(Prop::Oy, &[(-0.06, 220.0, ease::out), (0.0, 220.0, ease::back)]);

        self.later(0.25, Action::HandsUp(tok));
        self.later(0.55, Action::GreetBlink(tok));
        self.later(1.5, Action::GreetBlink(tok));
        self.later(1.55, Action::GreetHandsDown(tok));
        self.later(1.75, Action::GreetHappy(tok));
    }

    pub fn interrupt_greet(&mut self) {
        if self.hands <= 0.01 && now() >= self.wave_until {
            return;
        }
        self.greet_token += 1;
        self.wave_until = 0.0;
        self.wave_start = 0.0;
        self.anim(Prop::Hands, &[(0.0, 150.0, ease::in_out)]);
    }

    pub fn set_permanent_emote(&mut self, emote: Option<Emote>) {
        self.permanent_emote = emote;
        if emote == Some(Emote::Wink) {
            self.mini_next_behavior = now() + 0.8 + self.rnd() * 1.7;
            return;
        }
        self.permanent_eye = emote.map(|e| e.eye());
        if let Some(eye) = self.permanent_eye {
            self.eye_override = Some(eye);
            self.eye_override_until = f32::INFINITY;
        } else if self.eye_override_until == f32::INFINITY {
            self.eye_override = None;
            self.eye_override_until = 0.0;
        }
        self.mini_next_behavior = now() + 0.8 + self.rnd() * 1.7;
    }

    pub fn set_permanent_eye(&mut self, eye: EyeShape) {
        self.permanent_eye = Some(eye);
        self.eye_override = Some(eye);
        self.eye_override_until = f32::INFINITY;
    }

    pub fn trigger_emote(&mut self, emote: Emote, duration: f32) {
        let t = now();
        self.eye_override = Some(emote.eye());
        self.eye_override_until = t + duration;
        let ms = |secs: f32| (secs * 1000.0).max(0.0);

        match emote {
            Emote::Love => {
                self.anim(Prop::Blush, &[
                    (1.0, 300.0, ease::out), (1.0, ms(duration - 0.6), ease::lin), (0.0, 300.0, ease::in_out),
                ]);
                self.emit(PKind::Heart, 4);
                self.anim(Prop::Oy, &[(-0.1, 160.0, ease::out), (0.0, 300.0, ease::back)]);
            }
            Emote::Surprised => {
                self.anim(Prop::Oy, &[(-0.3, 140.0, ease::out), (0.0, 380.0, ease::back)]);
                self.anim(Prop::Es, &[(1.25, 120.0, ease::out), (1.0, 500.0, ease::in_out)]);
            }
            Emote::Proud => {
                self.emit(PKind::Star, 5);
                self.anim(Prop::Tilt, &[
                    (-0.14, 220.0, ease::out), (-0.14, ms(duration - 0.5), ease::lin), (0.0, 280.0, ease::in_out),
                ]);
                self.anim(Prop::Blush, &[
                    (0.7, 250.0, ease::out), (0.7, ms(duration - 0.5), ease::lin), (0.0, 300.0, ease::in_out),
                ]);
            }
            Emote::Wink => self.anim(Prop::Tilt, &[
                (0.12, 160.0, ease::out), (0.12, ms(duration - 0.4), ease::lin), (0.0, 240.0, ease::in_out),
            ]),
            Emote::Yawn => {
                self.anim(Prop::Sy, &[(1.12, 500.0, ease::in_out), (1.0, 500.0, ease::in_out)]);
                self.anim(Prop::Sx, &[(0.94, 500.0, ease::in_out), (1.0, 500.0, ease::in_out)]);
                self.later(0.7, Action::YawnClose);
            }
            Emote::Happy => self.anim(Prop::Blush, &[(0.6, 200.0, ease::out), (0.0, 600.0, ease::in_out)]),
            Emote::Annoyed => {
                self.eye_override = Some(EyeShape::Line);
                self.eye_override_until = t + 0.8;
                self.later(0.06, Action::Sound("annoyed"));
            }
        }
    }

    fn emit(&mut self, kind: PKind, count: i32) {
        for i in 0..count {
            let is_z = kind == PKind::Z;
            let (a, b, c2, d, e, f, sz) = (self.rnd(), self.rnd(), self.rnd(), self.rnd(), self.rnd(), self.rnd(), self.rnd());
            let y0 = if self.is_mini { -0.7 } else { -1.45 };
            self.particles.push(Particle {
                kind,
                x: (a - 0.5) * 0.9 + if is_z { 0.55 } else { 0.0 },
                y: y0 - b * 0.2,
                vx: (c2 - 0.5) * 0.35 + if is_z { 0.18 } else { 0.0 },
                vy: -(0.45 + d * 0.35),
                age: -(i as f32) * 0.14,
                life: 1.3 + e * 0.5,
                rot: f * TAU,
                size: 0.15 + sz * 0.08,
            });
        }
    }

    pub fn animate_morph(&mut self, target: f32) {
        let dur = if target > 0.5 { 550.0 } else { 650.0 };
        self.anim(Prop::Morph, &[(target, dur, ease::in_out)]);
    }

    pub fn reset_morph(&mut self) {
        self.tweens.remove(&Prop::Morph);
        self.morph = 0.0;
    }

    /// True while anything is still moving — lets the host stop its frame loop.
    pub fn busy(&self) -> bool {
        let cfg = &self.cfg;
        !self.tweens.is_empty()
            || !self.particles.is_empty()
            || !self.delayed.is_empty()
            || cfg.bounces || cfg.scans || cfg.breathes || cfg.zz || cfg.sweat
            || self.is_mini
            || (self.state != BotState::Idle && self.state != BotState::Sleeping)
            || self.medal_lit.iter().enumerate().any(|(i, v)| (v - self.medal_target(i)).abs() > 0.01)
            || (self.tg_yaw - self.yaw).abs() > 0.002
            || (self.tg_pitch - self.pitch).abs() > 0.002
            || (self.tg_tilt - self.tilt).abs() > 0.002
            || (self.tg_sy - self.sy).abs() > 0.002
            || (self.tg_sx - self.sx).abs() > 0.002
            || (self.tg_es - self.es).abs() > 0.002
            || self.slot_h > 0.001 || self.slot_h_vel.abs() > 0.001
            || (0..3).any(|i| (self.col[i] - self.col_t[i]).abs() > 0.003)
    }

    // ── Update ────────────────────────────────────────────────────────────────

    pub fn update(&mut self, dt: f32) {
        let n = now();

        // Tweens
        let props: Vec<Prop> = self.tweens.keys().copied().collect();
        for p in props {
            let Some(tw) = self.tweens.get_mut(&p) else { continue };
            let (target, dur, ez) = tw.keys[tw.index];
            let k = ((n - tw.start) / dur.max(0.0001)).clamp(0.0, 1.0);
            let v = tw.from + (target - tw.from) * ez(k);
            let mut finished = None;
            if k >= 1.0 {
                tw.from = target;
                tw.index += 1;
                tw.start = n;
                if tw.index >= tw.keys.len() {
                    finished = Some(tw.on_done);
                }
            }
            self.set(p, v);
            if let Some(done) = finished {
                self.tweens.remove(&p);
                if let Done::ResetRoll = done {
                    self.roll = 0.0;
                }
            }
        }

        // Delayed actions
        if !self.delayed.is_empty() {
            let due: Vec<Action> = {
                let mut due = Vec::new();
                self.delayed.retain(|(at, a)| {
                    if *at <= n {
                        due.push(*a);
                        false
                    } else {
                        true
                    }
                });
                due
            };
            for a in due {
                self.run(a);
            }
        }

        let t = n - self.t0;
        let mut ty = self.look_x * 0.62;
        let mut tp = self.look_y * 0.5;

        if let Some((lx, ly)) = self.cfg.look {
            ty = ty * 0.35 + lx * 0.55;
            tp = tp * 0.3 + ly * 0.5;
        }
        if self.cfg.scans {
            ty = (t * 2.6).sin() * 0.6;
            tp = -0.06;
        }
        if self.state == BotState::Sleeping {
            ty = 0.0;
            tp = -0.14;
        }
        if self.state == BotState::Dizzy {
            ty = (t * 9.0).sin() * 0.25;
        }

        // Mini bots never follow the mouse — they wander.
        if self.is_mini && self.cfg.look.is_none() && !self.cfg.scans
            && self.state != BotState::Sleeping && self.state != BotState::Dizzy
        {
            if n > self.mini_look_next {
                self.mini_look = (-0.88 + self.rnd() * 1.76, -0.55 + self.rnd() * 1.0);
                self.mini_look_next = n + 0.5 + self.rnd() * 1.5;
            }
            ty = self.mini_look.0 * 0.62;
            tp = self.mini_look.1 * 0.5;
        }

        self.tg_yaw = ty;
        self.tg_pitch = tp;
        self.tg_tilt = self.cfg.tilt;

        if n > self.wave_start && n < self.wave_until {
            let wt = n - self.wave_start;
            self.tg_tilt = -0.06 + (TAU * 1.2 * wt).sin() * 0.07;
        }

        let bounce = if self.cfg.bounces { -(t * 5.2).sin().abs() * 0.07 } else { 0.0 };
        let k_gen = 1.0 - 0.0008f32.powf(dt);
        if !self.locked(Prop::Oy) {
            self.oy += (bounce - self.oy) * k_gen;
        }

        if self.cfg.breathes {
            let amp = if self.is_mini { 0.07 } else { 0.035 };
            self.tg_sy = 1.0 + (t * 1.8).sin() * amp;
            self.tg_sx = 1.0 - (t * 1.8).sin() * amp * 0.57;
        } else if self.is_mini {
            self.tg_sy = 1.0 + (t * 2.2).sin() * 0.04;
            self.tg_sx = 1.0 - (t * 2.2).sin() * 0.02;
        } else {
            self.tg_sy = 1.0;
            self.tg_sx = 1.0;
        }

        if self.is_mini && n > self.mini_next_behavior {
            self.mini_behavior();
        }

        let k_look = 1.0 - 0.0025f32.powf(dt);
        if !self.locked(Prop::Yaw) { self.yaw += (self.tg_yaw - self.yaw) * k_look; }
        if !self.locked(Prop::Pitch) { self.pitch += (self.tg_pitch - self.pitch) * k_look; }
        if !self.locked(Prop::Tilt) { self.tilt += (self.tg_tilt - self.tilt) * k_gen; }
        if !self.locked(Prop::Sy) { self.sy += (self.tg_sy - self.sy) * k_gen; }
        if !self.locked(Prop::Sx) { self.sx += (self.tg_sx - self.sx) * k_gen; }
        if !self.locked(Prop::Es) { self.es += (self.tg_es - self.es) * k_gen; }

        self.col = mix3(self.col, self.col_t, 1.0 - 0.002f32.powf(dt));

        if n > self.next_blink {
            if self.state != BotState::Sleeping && self.state != BotState::Dizzy {
                self.blink();
                if self.rnd() < 0.22 {
                    self.later(0.23, Action::Blink(0));
                }
            }
            self.next_blink = n + 2.2 + self.rnd() * 3.2;
        }

        if self.eye_override.is_some() && n > self.eye_override_until {
            self.eye_override = self.permanent_eye;
            if self.permanent_eye.is_some() {
                self.eye_override_until = f32::INFINITY;
            }
        }

        if n - self.last_ambient > 1.3 {
            self.last_ambient = n;
            if self.cfg.zz {
                self.emit(PKind::Z, 1);
            }
            if !self.is_mini && self.cfg.sweat && self.rnd() < 0.5 {
                self.emit(PKind::Sweat, 1);
            }
        }

        for p in &mut self.particles {
            p.age += dt;
        }
        self.particles.retain(|p| p.age < p.life);

        self.update_character(dt, t);

        // Mouth slot spring — ω₀ = 2π/0.25, ζ = 0.6
        let omega = TAU / 0.25;
        let zeta = 0.6;
        let acc = omega * omega * (self.slot_h_target - self.slot_h) - 2.0 * zeta * omega * self.slot_h_vel;
        self.slot_h_vel += acc * dt;
        self.slot_h = (self.slot_h + self.slot_h_vel * dt).max(0.0);
    }

    fn run(&mut self, a: Action) {
        match a {
            Action::EmitSpark(n) => self.emit(PKind::Spark, n),
            Action::CommitBadge(tok) => {
                if tok == self.badge_token {
                    if let Some(b) = self.pending_badge.take() {
                        self.badge = b;
                        if b.is_some() {
                            self.anim(Prop::BadgeS, &[(1.0, 280.0, ease::back)]);
                        }
                    }
                }
            }
            Action::SlotClose => {
                self.slot_h_target = 0.0;
                self.is_chewing = true;
                self.later(0.8, Action::EndChew);
            }
            Action::EndChew => self.is_chewing = false,
            Action::Sound(s) => self.play(s),
            Action::Blink(_) => self.blink(),
            Action::HandsUp(tok) => {
                if self.greet_token == tok {
                    self.anim(Prop::Hands, &[(1.0, 280.0, ease::out)]);
                    self.anim(Prop::Sy, &[(0.95, 100.0, ease::out), (1.0, 260.0, ease::back)]);
                    self.anim(Prop::Sx, &[(1.04, 100.0, ease::out), (1.0, 260.0, ease::back)]);
                    self.play("greet");
                }
            }
            Action::GreetBlink(tok) => {
                if self.greet_token == tok {
                    self.blink();
                }
            }
            Action::GreetHandsDown(tok) => {
                if self.greet_token == tok {
                    self.wave_until = 0.0;
                    self.anim(Prop::Hands, &[(0.0, 200.0, ease::in_out)]);
                }
            }
            Action::GreetHappy(tok) => {
                if self.greet_token == tok {
                    self.eye_override = Some(EyeShape::Happy);
                    self.eye_override_until = now() + 0.3;
                }
            }
            Action::YawnClose => {
                self.eye_override = Some(EyeShape::Closed);
                self.emit(PKind::Z, 2);
            }
        }
    }

    fn mini_behavior(&mut self) {
        let n = now();
        match self.permanent_emote {
            Some(Emote::Happy) => {
                if self.locked(Prop::Oy) {
                    self.mini_next_behavior = n + 0.4;
                    return;
                }
                self.anim(Prop::Oy, &[(-0.3, 120.0, ease::out), (0.03, 200.0, ease::in_out), (0.0, 160.0, ease::back)]);
                self.anim(Prop::Sy, &[(0.82, 80.0, ease::out), (1.18, 130.0, ease::out), (0.88, 160.0, ease::in_out), (1.0, 200.0, ease::back)]);
                self.anim(Prop::Sx, &[(1.15, 80.0, ease::out), (0.88, 130.0, ease::out), (1.06, 160.0, ease::in_out), (1.0, 200.0, ease::back)]);
                self.mini_next_behavior = n + 2.2 + self.rnd() * 1.2;
            }
            Some(Emote::Annoyed) => {
                if self.locked(Prop::Yaw) {
                    self.mini_next_behavior = n + 0.5;
                    return;
                }
                self.anim(Prop::Yaw, &[
                    (-0.65, 50.0, ease::out), (0.65, 90.0, ease::in_out), (-0.5, 80.0, ease::in_out),
                    (0.4, 75.0, ease::in_out), (-0.2, 70.0, ease::in_out), (0.0, 140.0, ease::out),
                ]);
                self.mini_next_behavior = n + 3.0 + self.rnd() * 2.5;
            }
            Some(Emote::Wink) => {
                self.eye_override = Some(EyeShape::Wink);
                self.eye_override_until = n + 0.55;
                self.anim(Prop::Tilt, &[(0.13, 100.0, ease::out), (0.13, 320.0, ease::lin), (0.0, 200.0, ease::in_out)]);
                self.mini_next_behavior = n + 2.2 + self.rnd() * 2.0;
            }
            Some(Emote::Love) => {
                self.emit(PKind::Heart, 2);
                self.anim(Prop::Tilt, &[(-0.1, 180.0, ease::out), (0.1, 340.0, ease::in_out), (0.0, 220.0, ease::in_out)]);
                self.mini_next_behavior = n + 2.6 + self.rnd() * 1.5;
            }
            _ => self.mini_next_behavior = n + 3.0 + self.rnd() * 2.0,
        }
    }

    fn medal_target(&self, i: usize) -> f32 {
        let spec = character::medal_for_state(self.state.as_str());
        if self.state == BotState::Dizzy {
            return 0.5 + 0.5 * (now() * 8.0 + i as f32 * 1.3).sin();
        }
        if spec.slot == i as i32 { 1.0 } else { spec.rest }
    }

    /// Medallion glow, tassel pendulum, headphone beat and crown jingle.
    fn update_character(&mut self, dt: f32, t: f32) {
        let k_m = 1.0 - 0.0006f32.powf(dt);
        for i in 0..5 {
            let target = self.medal_target(i);
            self.medal_lit[i] += (target - self.medal_lit[i]) * k_m;
        }

        // Headphones pump while Claude works or thinks.
        let beating = matches!(self.state, BotState::Working | BotState::Thinking | BotState::Searching);
        let rate = if self.state == BotState::Thinking { 3.6 } else { 7.5 };
        let bt = (0.5 + 0.5 * (t * rate).sin()).powi(3);
        self.beat += ((if beating { bt } else { 0.0 }) - self.beat) * (1.0 - 0.0005f32.powf(dt));

        // Crown jingle when something needs the user.
        let jingle = matches!(self.state, BotState::Approval | BotState::Question);
        let wt = if jingle { (t * 14.0).sin() * 0.07 * (0.6 + 0.4 * (t * 2.2).sin()) } else { 0.0 };
        self.wobble += (wt - self.wobble) * (1.0 - 0.0003f32.powf(dt));
    }

    // ── Draw ──────────────────────────────────────────────────────────────────

    /// Draws hands, body, face, accessories, badge and particles into a canvas
    /// region of `w`×`h` logical pixels whose origin is the current (0,0).
    pub fn draw(&mut self, g: &mut Gfx, w: f32, h: f32) {
        let r = w * 0.3;
        // A spring can overshoot through zero while the island resizes; nothing to draw then.
        if !(r > 0.4) || !h.is_finite() {
            return;
        }
        let rx = r * 1.06;
        let ry = r;
        let cx = w / 2.0 + self.ox * r;
        let cy = h / 2.0 + self.particle_overhang / 2.0 + self.oy * r + r * 0.06;

        self.draw_hands_behind(g, r, rx, ry, cx, cy);

        g.save();
        g.translate(cx, cy);
        if self.tilt != 0.0 {
            g.rotate(self.tilt);
        }
        g.scale_xy(self.sx, self.sy);

        let dress = 1.0 - (self.morph * 2.2).min(1.0);
        let solid = self.body_color;
        let t = now();

        if !self.is_mini {
            draw_back(g, r, rx, ry, &BackOpts { vis: dress, beat: self.beat, yaw: self.yaw });
        }

        let body = self.body_path(rx, ry, r);
        fill_helmet(g, &body, r, rx, ry, solid);

        let plate = face_plate(r, self.yaw, self.pitch, self.morph);
        fill_plate(g, &plate, r, solid);

        draw_cheeks(g, r, self.yaw, self.blush.max(self.tint * 0.5).max(0.6), self.morph);

        self.draw_eyes(g, r, rx, ry);
        if self.morph > 0.05 {
            self.draw_mouth(g, &body, r);
        }

        // The crown is always there — idle, compact, even as a mailbox (it then
        // perches on top of the box). Cap and ear cups come and go with `dress`.
        let spec = character::medal_for_state(self.state.as_str());
        draw_front(g, r, &FrontOpts {
            lift: self.morph * 0.58,
            yaw: self.yaw,
            lit: self.medal_lit, glyph: spec.glyph,
            color: self.col, t, mini: self.is_mini, wobble: self.wobble,
        });

        g.restore();

        if self.is_mini && self.badge_s > 0.01 && self.morph < 0.25 {
            if let Some(b) = self.badge {
                self.draw_badge(g, b, r, cx, cy);
            }
        }
        self.draw_particles(g, r, cx, cy);
    }

    fn body_path(&self, rx: f32, ry: f32, r: f32) -> Path {
        let n = 72;
        let exp_n = 2.0 / 3.1;
        let (tw, th, tr) = (r, r * 0.94, r * 0.42);
        let m = self.morph;
        let mut pb = PathBuilder::new();
        for i in 0..=n {
            let a = i as f32 / n as f32 * TAU;
            let (sa, ca) = a.sin_cos();
            let px0 = rx * ca.signum() * ca.abs().powf(exp_n);
            let py0 = ry * sa.signum() * sa.abs().powf(exp_n);
            let (mut px, mut py) = (px0, py0);
            if m >= 0.005 {
                let (qx, qy) = rr_point(ca, sa, tw, th, tr);
                px = lerp(px0, qx, m);
                py = lerp(py0, qy, m);
            }
            if i == 0 { pb.move_to(px, py) } else { pb.line_to(px, py) }
        }
        pb.close();
        pb.finish().expect("body path")
    }

    fn draw_eyes(&self, g: &mut Gfx, r: f32, rx: f32, ry: f32) {
        let mut shape = self.eye_override.unwrap_or(self.cfg.eye);
        if self.morph > 0.5 {
            if self.is_chewing {
                shape = EyeShape::Happy;
            } else if self.slot_h_target > 0.05 || self.slot_h > 0.1 {
                shape = EyeShape::Cup;
            }
        }

        g.save();
        let ink = if self.is_mini { hex(MINI_INK) } else { hex(INK) };

        for sd in [-1.0f32, 1.0] {
            let eye_yaw = sd * EYE_SP + self.yaw;
            let mut eye_pitch = EYE_P + self.pitch + self.roll;
            eye_pitch = ((eye_pitch + PI).rem_euclid(TAU)) - PI;
            let cp = eye_pitch.cos();
            if eye_yaw.cos() * cp <= 0.04 {
                continue;
            }
            // The plate is the eyes' world: keep them on it without a clip mask.
            let plate_hw = lerp(0.93, 0.88, self.morph) * r;
            let eye_w = r * EYE_W * self.es * if self.is_mini { 1.9 } else { 1.0 };
            let ex_lim = (plate_hw - eye_w * 0.7 - r * 0.04).max(0.0);
            let ex = (eye_yaw.sin() * cp * rx).clamp(-ex_lim, ex_lim);
            let ey = -eye_pitch.sin() * ry + if self.morph > 0.0 { ry * 0.14 * self.morph } else { 0.0 };
            let fx = lerp(eye_yaw.cos().max(0.18), 1.0, self.morph * 0.7);
            let fy = lerp(cp.max(0.18), 1.0, self.morph * 0.7);
            let eye_mult = if self.is_mini { 1.9 } else { 1.0 };
            let ew = r * EYE_W * self.es * eye_mult;
            let eh = r * EYE_H * self.es * eye_mult;

            g.save();
            g.translate(ex, ey);
            g.scale_xy(fx, fy);
            self.draw_eye_shape(g, shape, ew, eh, sd, ink);
            g.restore();
        }
        g.restore();
    }

    fn draw_eye_shape(&self, g: &mut Gfx, shape: EyeShape, w: f32, h: f32, sd: f32, ink: Color) {
        let t = now();
        g.fill_style(ink);
        g.stroke_style(ink);
        match shape {
            EyeShape::Wide => self.draw_eye_shape(g, EyeShape::Pill, w * 1.16, h * 1.12, sd, ink),
            EyeShape::Pill => {
                let hh = (h * self.open).max(w * 0.3);
                g.round_rect(-w / 2.0, -hh / 2.0, w, hh, (w / 2.0).min(hh / 2.0));
                g.fill();
            }
            EyeShape::Dot => {
                g.circle(0.0, 0.0, w * 0.45);
                g.fill();
            }
            EyeShape::Line => {
                g.rotate(-sd * 0.2);
                g.round_rect(-w * 0.78, -w * 0.21, w * 1.56, w * 0.42, w * 0.21);
                g.fill();
            }
            EyeShape::Flat => {
                g.round_rect(-w * 0.72, -w * 0.2, w * 1.44, w * 0.4, w * 0.2);
                g.fill();
            }
            EyeShape::Happy => self.happy_arc(g, w, h),
            EyeShape::Closed => {
                g.line_width(w * 0.36);
                g.line_cap_round();
                g.begin_path();
                g.arc(0.0, -h * 0.08, w * 0.78, PI * 0.15, PI * 0.85, false);
                g.stroke();
            }
            EyeShape::Spiral => {
                g.line_width(w * 0.22);
                g.line_cap_round();
                g.begin_path();
                let mut a = 0.0f32;
                while a < 4.4 * PI {
                    let rad = w * 0.06 + a * w * 0.058;
                    let aa = a + t * 9.0 * sd;
                    let (px, py) = (aa.cos() * rad, aa.sin() * rad);
                    if a == 0.0 { g.move_to(px, py) } else { g.line_to(px, py) }
                    a += 0.2;
                }
                g.stroke();
            }
            EyeShape::Heart => {
                g.fill_style(hex("#FF4D6D"));
                heart_path(g, w * 1.2);
                g.fill();
            }
            EyeShape::Star => {
                g.fill_style(hex("#F7B32B"));
                g.rotate(t * 1.5 * sd);
                star_path(g, w * 1.05, w * 0.46);
                g.fill();
            }
            EyeShape::Tired => {
                g.round_rect(-w / 2.0, -h * 0.02, w, h * 0.38, w / 2.0);
                g.fill();
                g.round_rect(-w * 0.62, -h * 0.1, w * 1.24, w * 0.22, w * 0.11);
                g.fill();
            }
            EyeShape::Wink => {
                if sd < 0.0 {
                    let hh = (h * self.open).max(w * 0.3);
                    g.round_rect(-w / 2.0, -hh / 2.0, w, hh, (w / 2.0).min(hh / 2.0));
                    g.fill();
                } else {
                    self.happy_arc(g, w, h);
                }
            }
            EyeShape::Cup => {
                // Flat top, rounded bottom corners (U shape) — used while the box is open
                let hh = (h * self.open).max(w * 0.3);
                let cr = (w / 2.0).min(hh / 2.0);
                g.begin_path();
                g.move_to(-w / 2.0, -hh / 2.0);
                g.line_to(w / 2.0, -hh / 2.0);
                g.line_to(w / 2.0, hh / 2.0 - cr);
                g.quad_to(w / 2.0, hh / 2.0, w / 2.0 - cr, hh / 2.0);
                g.line_to(-w / 2.0 + cr, hh / 2.0);
                g.quad_to(-w / 2.0, hh / 2.0, -w / 2.0, hh / 2.0 - cr);
                g.close_path();
                g.fill();
            }
        }
    }

    fn happy_arc(&self, g: &mut Gfx, w: f32, h: f32) {
        g.line_width(w * 0.5);
        g.line_cap_round();
        g.begin_path();
        g.arc(0.0, h * 0.18, w * 0.82, PI * 1.12, PI * 1.88, false);
        g.stroke();
    }

    /// Mailbox slot: dark pill cut into the box face, with rim and lip highlights.
    fn draw_mouth(&self, g: &mut Gfx, body: &Path, r: f32) {
        let m = self.morph;
        let h_w = r * 1.8 * m;
        let h_h = self.slot_h * r * m;
        let h_x = -h_w / 2.0;
        let box_top = -r * (0.88 + 0.06 * m);
        let h_y = box_top + r * 0.08 * m;

        g.save();
        g.clip_path(body);

        g.stroke_style(rgba(255, 255, 255, 0.55 * m));
        g.line_width(1.0);
        g.line_cap_round();
        g.begin_path();
        g.move_to(-r * 0.9 * m, box_top + 1.0);
        g.line_to(r * 0.9 * m, box_top + 1.0);
        g.stroke();

        if h_h > 0.8 {
            let hr = (h_w / 2.0).min(h_h / 2.0);
            let grad = g.linear(0.0, h_y, 0.0, h_y + h_h, &[(0.0, rgb(7, 8, 10)), (1.0, rgb(16, 19, 26))]);
            g.fill_style(grad);
            g.round_rect(h_x, h_y, h_w, h_h, hr);
            g.fill();
            if h_h > 4.0 {
                let lip = hr.min((h_w - 2.0) / 2.0);
                g.stroke_style(rgba(255, 255, 255, 0.28 * m));
                g.begin_path();
                g.move_to(h_x + lip, h_y + h_h - 0.5);
                g.line_to(h_x + h_w - lip, h_y + h_h - 0.5);
                g.stroke();
            }
        }
        g.restore();
    }

    /// Hands sit behind the body — drawn before it, in world coordinates.
    fn draw_hands_behind(&self, g: &mut Gfx, r: f32, rx: f32, ry: f32, cx: f32, cy: f32) {
        if self.hands <= 0.01 || self.is_mini || r <= 14.0 {
            return;
        }
        let n = now();
        let body_h = 2.0 * ry;
        let hew = 0.3 * ry * self.hands;
        let heh = 0.26 * ry * self.hands;
        let hw_b = rx * self.sx;
        let hh_b = ry * self.sy;
        let waving = n >= self.wave_start && self.wave_start > 0.0 && n < self.wave_until;

        for sd in [-1.0f32, 1.0] {
            let (local_x, local_y, hand_rot);
            if sd > 0.0 && waving {
                let wt = n - self.wave_start;
                let rise = (wt / 0.18).min(1.0);
                let rise_e = 1.0 - (1.0 - rise).powi(3);
                let (rest_x, rest_y) = (hw_b * 1.08, hh_b * 0.7);
                let osc_x = (13.0 * wt).cos() * 0.06 * body_h;
                let osc_y = -(13.0 * wt).sin() * 0.14 * body_h;
                let (wave_x, wave_y) = (hw_b * 1.1 + osc_x, -hh_b * 0.15 + osc_y);
                local_x = rest_x + (wave_x - rest_x) * rise_e;
                local_y = rest_y + (wave_y - rest_y) * rise_e;
                hand_rot = (-0.5 + (13.0 * wt).sin() * 0.35) * rise_e;
            } else if sd < 0.0 && waving {
                let wt = n - self.wave_start;
                local_x = -hw_b * 1.08;
                local_y = hh_b * 0.7 + (6.0 * wt).sin() * 0.04 * body_h;
                hand_rot = 0.0;
            } else {
                local_x = sd * hw_b * 1.08;
                local_y = hh_b * 0.7;
                hand_rot = 0.0;
            }

            let (sin_t, cos_t) = self.tilt.sin_cos();
            let world_x = cx + cos_t * local_x - sin_t * local_y;
            let world_y = cy + sin_t * local_x + cos_t * local_y;

            g.save();
            g.translate(world_x, world_y);
            if hand_rot != 0.0 {
                g.rotate(hand_rot);
            }
            let grad = match self.body_color {
                Some(b) => g.linear(hew * 0.7, -heh * 0.85, -hew * 0.8, heh * 0.9, &[
                    (0.0, frgb(mix3(b, [1.0; 3], 0.35), 1.0)), (1.0, frgb(b, 1.0)),
                ]),
                None => g.linear(hew * 0.7, -heh * 0.85, -hew * 0.8, heh * 0.9, &[
                    (0.0, hex("#FFFAEF")), (1.0, hex("#EFE2C8")),
                ]),
            };
            g.fill_style(grad);
            g.begin_path();
            g.ellipse(0.0, 0.0, hew, heh, 0.0, 0.0, TAU, false);
            g.fill();
            g.stroke_style(rgba(0, 0, 0, 0.08));
            g.line_width(1.0);
            g.begin_path();
            g.ellipse(0.0, 0.0, hew, heh, 0.0, 0.0, TAU, false);
            g.stroke();
            g.restore();
        }
    }

    fn draw_badge(&self, g: &mut Gfx, badge: Badge, r: f32, cx: f32, cy: f32) {
        let bs = self.badge_s * if self.is_mini { 1.25 } else { 1.0 };
        let bx = cx - r * 0.72 * self.sx;
        let by = cy - r * 0.72 * self.sy;
        let t = now();

        g.save();
        g.translate(bx, by);
        g.scale_xy(bs, bs);
        let col = frgb(badge.color, 1.0);

        match badge.kind {
            BadgeKind::Dots => {
                if self.is_mini {
                    let phase = (t * 2.4) % 1.0;
                    let dot_r = r * 0.22 * (1.0 + 0.25 * (phase * TAU).sin());
                    g.fill_style(Color::BLACK);
                    g.circle(0.0, 0.0, r * 0.2);
                    g.fill();
                    g.fill_style(col);
                    g.circle(0.0, 0.0, dot_r);
                    g.fill();
                }
            }
            BadgeKind::Bang | BadgeKind::Question => {
                g.fill_style(Color::BLACK);
                g.circle(0.0, 0.0, r * 0.3);
                g.fill();
                g.fill_style(col);
                g.circle(0.0, 0.0, r * 0.23);
                g.fill();
                if !self.is_mini {
                    let s = if badge.kind == BadgeKind::Bang { "!" } else { "?" };
                    text::draw(g, s, 0.0, r * 0.02, Face::Bold, r * 0.32, Color::WHITE, Align::Center);
                }
            }
            BadgeKind::Dot => {
                g.fill_style(Color::BLACK);
                g.circle(0.0, 0.0, r * 0.2);
                g.fill();
                g.fill_style(col);
                g.circle(0.0, 0.0, r * 0.135);
                g.fill();
            }
        }
        g.restore();
    }

    fn draw_particles(&self, g: &mut Gfx, r: f32, cx: f32, cy: f32) {
        for p in &self.particles {
            if p.age <= 0.0 {
                continue;
            }
            let k = p.age / p.life;
            let a = if k < 0.2 { k / 0.2 } else { 1.0 - (k - 0.2) / 0.8 };
            let px = cx + (p.x + p.vx * p.age) * r * 1.3;
            let py = cy + (p.y + p.vy * p.age) * r * 1.3;
            let sz = r * p.size * (1.0 + k * 0.4);

            g.save();
            g.translate(px, py);
            g.set_alpha(a.clamp(0.0, 1.0));
            match p.kind {
                PKind::Heart => {
                    g.rotate((p.age * 6.0).sin() * 0.3);
                    g.fill_style(hex("#FF4D6D"));
                    heart_path(g, sz);
                    g.fill();
                }
                PKind::Star => {
                    g.rotate(p.rot + p.age * 2.0);
                    g.fill_style(hex("#F7B32B"));
                    star_path(g, sz, sz * 0.45);
                    g.fill();
                }
                PKind::Spark => {
                    g.rotate(p.rot);
                    g.fill_style(Color::WHITE);
                    star_path(g, sz * 0.8, sz * 0.18);
                    g.fill();
                }
                PKind::Sweat => {
                    g.fill_style(hex("#7CC7FF"));
                    g.begin_path();
                    g.move_to(0.0, -sz);
                    g.quad_to(sz * 0.8, sz * 0.2, 0.0, sz * 0.6);
                    g.quad_to(-sz * 0.8, sz * 0.2, 0.0, -sz);
                    g.fill();
                }
                PKind::Z => {
                    text::draw(g, "z", 0.0, 0.0, Face::Bold, sz * 1.9, rgb(209, 219, 235), Align::Center);
                }
            }
            g.restore();
        }
    }
}

fn heart_path(g: &mut Gfx, s: f32) {
    g.begin_path();
    g.move_to(0.0, s * 0.38);
    g.cubic_to(-s * 1.05, -s * 0.15, -s * 0.5, -s * 0.95, 0.0, -s * 0.38);
    g.cubic_to(s * 0.5, -s * 0.95, s * 1.05, -s * 0.15, 0.0, s * 0.38);
    g.close_path();
}

fn star_path(g: &mut Gfx, ro: f32, ri: f32) {
    g.begin_path();
    for i in 0..10 {
        let rad = if i % 2 == 1 { ri } else { ro };
        let a = -PI / 2.0 + i as f32 * PI / 5.0;
        g.line_to(a.cos() * rad, a.sin() * rad);
    }
    g.close_path();
}

/// Ray → rounded-rect boundary intersection, for the mailbox morph.
fn rr_point(ca: f32, sa: f32, w: f32, h: f32, cr: f32) -> (f32, f32) {
    let eps = 1e-6;
    let kx = if ca >= 0.0 { 1.0 } else { -1.0 };
    let ky = if sa >= 0.0 { 1.0 } else { -1.0 };
    let cx = kx * (w - cr);
    let cy = ky * (h - cr);

    let dot = ca * cx + sa * cy;
    let disc = dot * dot - (cx * cx + cy * cy - cr * cr);
    if disc >= 0.0 {
        let t = dot + disc.sqrt();
        if t > eps {
            let (px, py) = (ca * t, sa * t);
            if px.abs() >= w - cr - eps && py.abs() >= h - cr - eps {
                return (px, py);
            }
        }
    }
    if sa.abs() > eps {
        let t = ky * h / sa;
        if t > eps {
            let px = ca * t;
            if px.abs() <= w - cr + eps {
                return (px, ky * h);
            }
        }
    }
    if ca.abs() > eps {
        let t = kx * w / ca;
        if t > eps {
            let py = sa * t;
            if py.abs() <= h - cr + eps {
                return (kx * w, py);
            }
        }
    }
    (kx * w, ky * h)
}
