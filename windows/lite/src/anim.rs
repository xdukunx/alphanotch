// Easing + spring helpers — port of core/anim.ts.
//
// `now()` is a process clock in seconds. The snapshot tool replaces it with a
// manual clock so animations can be rendered at an exact moment.

use std::cell::Cell;
use std::sync::OnceLock;
use std::time::Instant;

pub type EaseFn = fn(f32) -> f32;

pub mod ease {
    pub fn out(t: f32) -> f32 {
        1.0 - (1.0 - t).powi(3)
    }
    pub fn in_out(t: f32) -> f32 {
        if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 }
    }
    pub fn back(t: f32) -> f32 {
        let c1 = 1.7;
        let c3 = c1 + 1.0;
        1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
    }
    pub fn lin(t: f32) -> f32 {
        t
    }
    pub fn ease_in(t: f32) -> f32 {
        t * t * t
    }
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
pub fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    v.max(lo).min(hi)
}
pub fn seg(t: f32, a: f32, b: f32) -> f32 {
    clamp((t - a) / (b - a), 0.0, 1.0)
}

thread_local! {
    static MANUAL: Cell<Option<f64>> = const { Cell::new(None) };
}

static START: OnceLock<Instant> = OnceLock::new();

/// Seconds since the app started (or the snapshot clock).
pub fn now() -> f32 {
    if let Some(t) = MANUAL.with(|m| m.get()) {
        return t as f32;
    }
    START.get_or_init(Instant::now).elapsed().as_secs_f32()
}

#[cfg_attr(not(feature = "snapshot"), allow(dead_code))]
pub fn set_manual_clock(t: Option<f64>) {
    MANUAL.with(|m| m.set(t));
}

/// cubic-bezier(x1,y1,x2,y2) — the 340 ms close curve (.45,0,.2,1).
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    let cx = |t: f32| (1.0 - t).powi(2) * 3.0 * t * x1 + 3.0 * (1.0 - t) * t * t * x2 + t.powi(3);
    let cy = |t: f32| (1.0 - t).powi(2) * 3.0 * t * y1 + 3.0 * (1.0 - t) * t * t * y2 + t.powi(3);
    let (mut lo, mut hi, mut t) = (0.0, 1.0, x);
    for _ in 0..12 {
        if cx(t) < x { lo = t } else { hi = t }
        t = (lo + hi) / 2.0;
    }
    cy(t)
}

pub fn close_curve(x: f32) -> f32 {
    cubic_bezier(0.45, 0.0, 0.2, 1.0, x)
}

/// SwiftUI-equivalent spring: ω₀ = 2π / response, ζ = dampingFraction.
#[derive(Clone)]
pub struct Spring {
    pub value: f32,
    pub target: f32,
    pub velocity: f32,
    omega: f32,
    zeta: f32,
}

impl Spring {
    pub fn new(value: f32) -> Self {
        Self::with(value, 0.5, 0.72)
    }

    pub fn with(value: f32, response: f32, damping: f32) -> Self {
        Self { value, target: value, velocity: 0.0, omega: std::f32::consts::TAU / response, zeta: damping }
    }

    pub fn configure(&mut self, response: f32, damping: f32) {
        self.omega = std::f32::consts::TAU / response;
        self.zeta = damping;
    }

    pub fn set(&mut self, v: f32) {
        self.value = v;
        self.target = v;
        self.velocity = 0.0;
    }

    pub fn settled(&self) -> bool {
        (self.target - self.value).abs() < 0.01 && self.velocity.abs() < 0.05
    }

    pub fn step(&mut self, dt: f32) {
        let steps = (dt / (1.0 / 240.0)).ceil().max(1.0) as i32;
        let h = dt / steps as f32;
        for _ in 0..steps {
            let acc = self.omega * self.omega * (self.target - self.value)
                - 2.0 * self.zeta * self.omega * self.velocity;
            self.velocity += acc * h;
            self.value += self.velocity * h;
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Idle,
    Spring,
    Curve,
}

/// Value driven by a spring when growing and by a timed curve when shrinking.
#[derive(Clone)]
pub struct Tracked {
    spring: Spring,
    from: f32,
    to: f32,
    start: f32,
    dur: f32,
    mode: Mode,
}

impl Tracked {
    pub fn new(v: f32) -> Self {
        Self { spring: Spring::new(v), from: 0.0, to: 0.0, start: 0.0, dur: 0.0, mode: Mode::Idle }
    }

    pub fn value(&self) -> f32 {
        self.spring.value
    }

    pub fn animating(&self) -> bool {
        self.mode != Mode::Idle
    }

    pub fn jump(&mut self, v: f32) {
        self.spring.set(v);
        self.mode = Mode::Idle;
    }

    pub fn spring_to(&mut self, v: f32) {
        self.spring.configure(0.5, 0.72);
        self.spring.target = v;
        self.mode = Mode::Spring;
    }

    pub fn curve_towards(&mut self, v: f32) {
        self.from = self.spring.value;
        self.to = v;
        self.start = now();
        self.dur = 0.34;
        self.spring.target = v;
        self.spring.velocity = 0.0;
        self.mode = Mode::Curve;
    }

    pub fn step(&mut self, dt: f32) {
        match self.mode {
            Mode::Spring => {
                self.spring.step(dt);
                if self.spring.settled() {
                    self.spring.value = self.spring.target;
                    self.spring.velocity = 0.0;
                    self.mode = Mode::Idle;
                }
            }
            Mode::Curve => {
                let p = clamp((now() - self.start) / self.dur, 0.0, 1.0);
                self.spring.value = lerp(self.from, self.to, close_curve(p));
                if p >= 1.0 {
                    self.spring.velocity = 0.0;
                    self.mode = Mode::Idle;
                }
            }
            Mode::Idle => {}
        }
    }
}
