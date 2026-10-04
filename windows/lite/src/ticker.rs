// Overview task ticker — port of views/ticker.ts (TickerView V2 from
// IslandViewContent.swift).
//
// Three rows: completed (A), current → completed (B), incoming (C). Every row
// position is recomputed from a single clock, driven by the island's frame loop.
// Steps are queued, so a burst scrolls past rather than vanishing.

use crate::anim::{clamp, cubic_bezier, lerp};
use crate::gfx::{hex, rgba, with_alpha, Gfx};
use crate::icons::{self, Icon};
use crate::state::Task;
use crate::text::{self, Align, Face};
use crate::ui::pal;

const ROW_H: f32 = 22.0;
/// One step transition, seconds.
const DURATION: f32 = 0.38;
/// Beyond this many queued steps we stop trying to show them all.
const MAX_QUEUE: usize = 4;
const COMPLETED_SCALE: f32 = 11.5 / 13.0;
pub const HEIGHT: f32 = 44.0;

#[derive(Clone, Default)]
struct Row {
    text: String,
    y: f32,
    phase: f32,
    opacity: f32,
}

pub struct Ticker {
    a: Row,
    b: Row,
    c: Row,
    queue: Vec<String>,
    start: Option<f32>,
    display_index: i32,
}

impl Default for Ticker {
    fn default() -> Self {
        Self::new()
    }
}

impl Ticker {
    pub fn new() -> Self {
        let mut t = Ticker {
            a: Row::default(),
            b: Row::default(),
            c: Row::default(),
            queue: Vec::new(),
            start: None,
            display_index: -1,
        };
        t.rest();
        t
    }

    /// The state between transitions: completed on top, current below.
    fn rest(&mut self) {
        place(&mut self.a, 0.0, 1.0, 1.0);
        place(&mut self.b, ROW_H, 0.0, 1.0);
        place(&mut self.c, ROW_H * 2.0, 0.0, 0.0);
    }

    pub fn animating(&self) -> bool {
        self.start.is_some() || !self.queue.is_empty()
    }

    /// Feeds the focused task in and advances the transition clock.
    pub fn tick(&mut self, now: f32, task: Option<&Task>) {
        self.sync(task);
        if self.start.is_none() {
            if self.queue.is_empty() {
                return;
            }
            self.c.text = self.queue[0].clone();
            place(&mut self.c, ROW_H * 2.0, 0.0, 0.0);
            self.start = Some(now);
        }
        let p = clamp((now - self.start.unwrap()) / DURATION, 0.0, 1.0);
        let e = cubic_bezier(0.4, 0.0, 0.2, 1.0, p);

        // A leaves upwards and fades a little faster than it moves, as on macOS.
        place(&mut self.a, lerp(0.0, -ROW_H, e), 1.0, clamp(1.0 - p * 1.35, 0.0, 1.0));
        place(&mut self.b, lerp(ROW_H, 0.0, e), e, 1.0);
        place(&mut self.c, lerp(ROW_H * 2.0, ROW_H, e), 0.0, e);

        if p < 1.0 {
            return;
        }
        // Commit: texts move, rows stay put — no reordering, no overlap.
        self.a.text = self.b.text.clone();
        self.b.text = self.c.text.clone();
        if !self.queue.is_empty() {
            self.queue.remove(0);
        }
        self.start = None;
        self.rest();
    }

    fn sync(&mut self, task: Option<&Task>) {
        let dots = ["…".to_string()];
        let steps: &[String] = match task {
            Some(t) if !t.steps.is_empty() => &t.steps,
            _ => &dots,
        };
        let idx = task.map(|t| t.step_index.min(steps.len() - 1) as i32).unwrap_or(-1);

        let seed = |me: &mut Ticker, idx: i32| {
            me.display_index = idx;
            me.a.text = if idx > 0 { steps[idx as usize - 1].clone() } else { "…".into() };
            me.b.text = steps[idx.max(0) as usize].clone();
            me.rest();
        };

        // First render: drop straight into place, no animation.
        if self.display_index < 0 {
            seed(self, idx);
            return;
        }
        // The session restarted (steps were cleared): re-seed rather than scroll.
        if idx < self.display_index {
            self.queue.clear();
            self.start = None;
            seed(self, idx);
            return;
        }
        for i in (self.display_index + 1)..=idx {
            self.queue.push(steps[i as usize].clone());
        }
        self.display_index = idx;
        if self.queue.len() > MAX_QUEUE {
            let cut = self.queue.len() - MAX_QUEUE;
            self.queue.drain(..cut);
        }
    }

    /// Draws into the region (x, y, w, 44), top-left origin.
    pub fn draw(&self, g: &mut Gfx, x: f32, y: f32, w: f32, now: f32) {
        g.save();
        g.clip_rect_xywh(x, y, w, HEIGHT);
        for row in [&self.a, &self.b, &self.c] {
            draw_row(g, row, x, y, w, now);
        }
        g.restore();
    }
}

fn place(row: &mut Row, y: f32, phase: f32, opacity: f32) {
    row.y = y;
    row.phase = phase;
    row.opacity = opacity;
}

/// Vertical fade at both ends of the ticker (`mask-image`).
fn mask(yc: f32) -> f32 {
    let f = yc / HEIGHT;
    if f < 0.12 {
        (f / 0.12).max(0.0)
    } else if f > 0.85 {
        ((1.0 - f) / 0.15).max(0.0)
    } else {
        1.0
    }
}

fn draw_row(g: &mut Gfx, row: &Row, x: f32, y: f32, w: f32, now: f32) {
    if row.text.is_empty() || row.opacity <= 0.01 {
        return;
    }
    let yc = row.y + ROW_H / 2.0;
    let m = mask(yc);
    let alpha = row.opacity * m;
    if alpha <= 0.01 {
        return;
    }
    let scale = 1.0 - row.phase * (1.0 - COMPLETED_SCALE);
    g.save();
    g.translate(x - row.phase * 10.0, y + yc);
    g.scale_xy(scale, scale);
    g.mul_alpha(alpha);

    // Icon: chevron while current, check once completed.
    let chev = clamp(1.0 - row.phase * 2.0, 0.0, 1.0);
    let check = clamp(row.phase * 2.0 - 1.0, 0.0, 1.0);
    if chev > 0.0 {
        g.save();
        g.mul_alpha(chev);
        icons::stroke(g, Icon::ChevronRight, 6.0, 0.0, 9.0, 2.4, hex(pal::DIM2));
        g.restore();
    }
    if check > 0.0 {
        g.save();
        g.mul_alpha(check);
        icons::stroke(g, Icon::Check, 6.0, 0.0, 8.0, 2.2, hex("#454850"));
        g.restore();
    }

    let tx = 18.0;
    let max_w = (w - 18.0) / scale;
    let line = text::ellipsize(&row.text, Face::Medium, 13.0, max_w);

    let shimmer_a = clamp(1.0 - row.phase * 1.6, 0.0, 1.0);
    let dim_a = clamp(row.phase * 2.0 - 0.4, 0.0, 1.0);
    if shimmer_a > 0.0 {
        g.save();
        g.mul_alpha(shimmer_a);
        let progress = (now / 2.2).fract();
        let p = 1.3 - 2.6 * progress; // background-position, 130% → -130%
        text::draw_with(g, &line, tx, 0.0, Face::Medium, 13.0, Align::Left, |f| {
            let s = (f + 1.6 * p) / 2.6;
            let (a, b) = (hex("#7C818A"), hex("#F2F3F5"));
            let k = if s <= 0.0 {
                0.0
            } else if s < 0.4 {
                s / 0.4
            } else if s < 0.7 {
                1.0 - (s - 0.4) / 0.3
            } else {
                0.0
            };
            tiny_skia::Color::from_rgba(
                lerp(a.red(), b.red(), k),
                lerp(a.green(), b.green(), k),
                lerp(a.blue(), b.blue(), k),
                1.0,
            )
            .unwrap_or(a)
        });
        g.restore();
    }
    if dim_a > 0.0 {
        g.save();
        g.mul_alpha(dim_a);
        text::draw(g, &line, tx, 0.0, Face::Medium, 13.0, with_alpha(hex("#6B7079"), 1.0), Align::Left);
        g.restore();
    }
    let _ = rgba(0, 0, 0, 0.0);
    g.restore();
}
