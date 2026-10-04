// The dashboard: what the island shows when you open it and nothing needs you.
// Greeting, Now Playing (artwork, seek bar, transport), a timer with presets and a
// few system gauges. Everything here is read from the system or from `activity`.

use std::cell::Cell;

use tiny_skia::Color;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;

use crate::activity::{self, Transport};
use crate::anim::clamp;
use crate::app::{fading_text, App};
use crate::gfx::{hex, rgba, Gfx};
use crate::text::{self, Align, Face};
use crate::ui::{id_of, pal, Rect};

const LEFT_W: f32 = 372.0;

// ── System gauges ─────────────────────────────────────────────────────────────

thread_local! {
    static CPU_PREV: Cell<(u64, u64)> = const { Cell::new((0, 0)) };
    static CPU_LAST: Cell<f32> = const { Cell::new(0.0) };
}

fn ft(f: windows::Win32::Foundation::FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

/// CPU load since the previous call, 0…1. Called at about 2 Hz while the dashboard is open.
fn cpu_load() -> f32 {
    let (mut idle, mut kernel, mut user) = Default::default();
    unsafe {
        if GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).is_err() {
            return CPU_LAST.get();
        }
    }
    let (i, total) = (ft(idle), ft(kernel) + ft(user));
    let (pi, pt) = CPU_PREV.get();
    CPU_PREV.set((i, total));
    if pt == 0 || total <= pt {
        return CPU_LAST.get();
    }
    let load = 1.0 - (i - pi) as f32 / (total - pt) as f32;
    CPU_LAST.set(load.clamp(0.0, 1.0));
    CPU_LAST.get()
}

fn mem_load() -> f32 {
    let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    unsafe {
        if GlobalMemoryStatusEx(&mut m).is_ok() {
            return m.dwMemoryLoad as f32 / 100.0;
        }
    }
    0.0
}

/// (percent, charging) or None on a desktop without a battery.
fn battery() -> Option<(u8, bool)> {
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe {
        GetSystemPowerStatus(&mut s).ok()?;
    }
    (s.BatteryLifePercent <= 100).then_some((s.BatteryLifePercent, s.ACLineStatus == 1))
}

// ── Greeting ──────────────────────────────────────────────────────────────────

fn greeting() -> (String, String) {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let t = unsafe { GetLocalTime() };
    let hello = match t.wHour {
        4..=10 => "Selamat pagi",
        11..=14 => "Selamat siang",
        15..=17 => "Selamat sore",
        _ => "Selamat malam",
    };
    let name = std::env::var("USERNAME").unwrap_or_default();
    let days = ["Minggu", "Senin", "Selasa", "Rabu", "Kamis", "Jumat", "Sabtu"];
    let months = [
        "Januari", "Februari", "Maret", "April", "Mei", "Juni", "Juli", "Agustus", "September", "Oktober",
        "November", "Desember",
    ];
    let date = format!(
        "{}, {} {} {}",
        days[(t.wDayOfWeek as usize) % 7],
        t.wDay,
        months[(t.wMonth as usize).clamp(1, 12) - 1],
        t.wYear
    );
    (if name.is_empty() { hello.to_string() } else { format!("{hello}, {name}") }, date)
}

// ── Glyphs ────────────────────────────────────────────────────────────────────

pub(crate) fn glyph_play(g: &mut Gfx, cx: f32, cy: f32, s: f32) {
    g.begin_path();
    g.move_to(cx - s * 0.38, cy - s * 0.5);
    g.line_to(cx + s * 0.52, cy);
    g.line_to(cx - s * 0.38, cy + s * 0.5);
    g.close_path();
    g.fill();
}

pub(crate) fn glyph_pause(g: &mut Gfx, cx: f32, cy: f32, s: f32) {
    g.fill_round_rect(cx - s * 0.42, cy - s * 0.5, s * 0.30, s, s * 0.08);
    g.fill_round_rect(cx + s * 0.12, cy - s * 0.5, s * 0.30, s, s * 0.08);
}

/// `dir` = 1 for next, -1 for previous.
fn glyph_skip(g: &mut Gfx, cx: f32, cy: f32, s: f32, dir: f32) {
    for k in [-0.5f32, 0.05] {
        let x = cx + dir * k * s;
        g.begin_path();
        g.move_to(x - dir * s * 0.38, cy - s * 0.42);
        g.line_to(x + dir * s * 0.45, cy);
        g.line_to(x - dir * s * 0.38, cy + s * 0.42);
        g.close_path();
        g.fill();
    }
}

/// Events that have not ended yet (all-day ones always count).
fn upcoming(all: &[crate::gtasks::Event]) -> Vec<crate::gtasks::Event> {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let t = unsafe { GetLocalTime() };
    let now = format!("{:02}:{:02}", t.wHour, t.wMinute);
    all.iter().filter(|e| e.start == "Seharian" || e.end.as_deref().map(|end| end > now.as_str()).unwrap_or(true)).cloned().collect()
}

/// Cuts `s` with an ellipsis so it fits `max_w`.
fn ellipsize(s: &str, face: Face, size: f32, max_w: f32) -> String {
    if text::measure(s, face, size) <= max_w {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars() {
        let mut next = out.clone();
        next.push(c);
        if text::measure(&format!("{next}…"), face, size) > max_w {
            break;
        }
        out = next;
    }
    format!("{out}…")
}

fn mmss(secs: f32) -> String {
    let s = secs.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

impl App {
    pub fn draw_dashboard(&mut self, g: &mut Gfx, v: Rect, n: f32) {
        // Greeting row (Mochi sits at the left, see layout).
        let (hello, date) = greeting();
        text::draw(g, &hello, v.x + 60.0, v.y + 12.0, Face::Bold, 14.5, hex(pal::INK), Align::Left);
        text::draw(g, &date, v.x + 60.0, v.y + 29.0, Face::Regular, 11.0, hex(pal::DIM), Align::Left);
        if let Some(w) = crate::weather::report() {
            let dx = text::measure(&date, Face::Regular, 11.0);
            let label = format!("·  {:.0}° {}", w.now.temp, crate::weather::label_of(w.now.code));
            text::draw(g, &label, v.x + 60.0 + dx + 8.0, v.y + 29.0, Face::Regular, 11.0, hex(pal::DIM2), Align::Left);
        }

        let top = v.y + 44.0;
        let h = (v.h - 44.0).max(0.0);
        let left = Rect::new(v.x, top, LEFT_W, h);
        // The right column has no card: text sits straight on the island, as in OmniNotch.
        let col = Rect::new(v.x + LEFT_W + 20.0, top + 2.0, v.w - LEFT_W - 20.0 - 6.0, h - 2.0);
        crate::ui::card(g, left, crate::layout::Wash::None, false);

        self.dash_now_playing(g, left, n);
        self.dash_column(g, col);
    }

    fn dash_now_playing(&mut self, g: &mut Gfx, r: Rect, n: f32) {
        let art = Rect::new(r.x + 16.0, r.y + (r.h - 96.0) / 2.0, 96.0, 96.0);
        let col = r.x + 128.0;
        let colw = r.w - 128.0 - 18.0;
        let _ = n;

        let Some(m) = activity::media() else {
            g.fill_style(rgba(255, 255, 255, 0.06));
            g.fill_round_rect(art.x, art.y, art.w, art.h, 14.0);
            text::draw(g, "♪", art.cx(), art.cy(), Face::Bold, 30.0, rgba(255, 255, 255, 0.25), Align::Center);
            text::draw(g, "Tidak ada media", col, r.cy() - 8.0, Face::Medium, 14.0, hex(pal::INK), Align::Left);
            text::draw(g, "Putar musik di Spotify atau browser.", col, r.cy() + 12.0, Face::Regular, 11.5, hex(pal::DIM), Align::Left);
            return;
        };

        // Artwork.
        match activity::art() {
            Some(a) => g.fill_image_round_rect(&a.rgba, a.size, art.x, art.y, art.w, art.h, 14.0),
            None => {
                g.fill_style(rgba(255, 255, 255, 0.06));
                g.fill_round_rect(art.x, art.y, art.w, art.h, 14.0);
                text::draw(g, "♪", art.cx(), art.cy(), Face::Bold, 30.0, rgba(255, 255, 255, 0.25), Align::Center);
            }
        }

        fading_text(g, &m.title, col, r.y + 28.0, 14.0, hex(pal::INK), col, col + colw);
        if !m.artist.is_empty() {
            fading_text(g, &m.artist, col, r.y + 47.0, 12.0, hex(pal::DIM), col, col + colw);
        }

        // Seek bar.
        let pos = m.position_now();
        let bar = Rect::new(col, r.y + 66.0, colw, 4.0);
        let hit = Rect::new(col - 4.0, r.y + 58.0, colw + 8.0, 20.0);
        let (clicked, hover, _) = self.ui.click_region(id_of("dash-seek", 91), hit);
        g.fill_style(rgba(255, 255, 255, 0.16));
        g.fill_round_rect(bar.x, bar.y, bar.w, bar.h, 2.0);
        if m.duration > 0.0 {
            let k = clamp(pos / m.duration, 0.0, 1.0);
            g.fill_style(if hover { hex("#FFFFFF") } else { rgba(255, 255, 255, 0.88) });
            g.fill_round_rect(bar.x, bar.y, (bar.w * k).max(4.0), bar.h, 2.0);
            if clicked {
                let f = clamp((self.ui.input.mouse.0 - bar.x) / bar.w, 0.0, 1.0);
                activity::transport(Transport::Seek(f * m.duration));
            }
            text::draw(g, &mmss(pos), col, r.y + 84.0, Face::Regular, 10.5, hex(pal::DIM3), Align::Left);
            text::draw(g, &mmss(m.duration), col + colw, r.y + 84.0, Face::Regular, 10.5, hex(pal::DIM3), Align::Right);
        }

        // Transport.
        let cy = r.y + 108.0;
        let mid = col + colw / 2.0;
        for (id, cx, size, kind) in [("dash-prev", mid - 52.0, 15.0, 0), ("dash-play", mid, 19.0, 1), ("dash-next", mid + 52.0, 15.0, 2)] {
            let hit = Rect::new(cx - 20.0, cy - 16.0, 40.0, 32.0);
            let (clicked, hover, pressed) = self.ui.click_region(id_of(id, 92), hit);
            let tint: Color = if pressed { rgba(255, 255, 255, 0.6) } else if hover { hex("#FFFFFF") } else { rgba(255, 255, 255, 0.86) };
            g.fill_style(tint);
            match kind {
                0 => glyph_skip(g, cx, cy, size, -1.0),
                1 => {
                    if m.playing {
                        glyph_pause(g, cx, cy, size)
                    } else {
                        glyph_play(g, cx, cy, size)
                    }
                }
                _ => glyph_skip(g, cx, cy, size, 1.0),
            }
            if clicked {
                activity::transport(match kind {
                    0 => Transport::Previous,
                    1 => Transport::PlayPause,
                    _ => Transport::Next,
                });
            }
        }
    }

    /// Right column (OmniNotch style): today's agenda on top, the task list under it. The timer is one
    /// small button on the tasks header; it opens 5/15/25 and a gear for a custom length.
    fn dash_column(&mut self, g: &mut Gfx, c: Rect) {
        // ── Today (an accordion: it folds away when there is nothing to show) ──
        let gstatus = crate::gtasks::status();
        let needs = crate::gtasks::calendar_needs_login();
        let connected = gstatus == crate::gtasks::Status::Linked && !needs;
        let events = upcoming(&crate::gtasks::events());
        let shown = events.len().min(2);
        let auto_open = connected && !events.is_empty();
        let open = self.today_force.unwrap_or(auto_open);
        // The body is: the events, or one line saying why there are none / how to connect.
        let target = if !open { 0.0 } else if connected && !events.is_empty() { shown as f32 * 32.0 } else { 26.0 };
        self.today_target = target;
        if (self.today_h - target).abs() > 0.3 {
            self.ensure_running();
        }

        let head = Rect::new(c.x, c.y, c.w, 26.0);
        let (hclick, hhover, _) = self.ui.click_region(id_of("today-head", 97), head);
        text::draw(g, "Hari ini", c.x, c.y + 12.0, Face::Bold, 12.5, if hhover { hex("#FFFFFF") } else { hex(pal::INK) }, Align::Left);
        let hw = text::measure("Hari ini", Face::Bold, 12.5);
        // Chevron: points right when folded, down when open.
        g.stroke_style(hex(pal::DIM));
        g.line_width(1.5);
        g.line_cap_round();
        g.line_join_round();
        g.begin_path();
        let (cx0, cy0) = (c.x + hw + 10.0, c.y + 12.0);
        if open {
            g.move_to(cx0 - 3.0, cy0 - 1.5);
            g.line_to(cx0, cy0 + 1.8);
            g.line_to(cx0 + 3.0, cy0 - 1.5);
        } else {
            g.move_to(cx0 - 1.5, cy0 - 3.0);
            g.line_to(cx0 + 1.8, cy0);
            g.line_to(cx0 - 1.5, cy0 + 3.0);
        }
        g.stroke();
        if hclick {
            crate::sound::play("blip");
            self.today_force = Some(!open);
        }
        if !open {
            // Folded: a one-line summary beside the title.
            let summary = if !connected {
                if needs { "kalender: masuk ulang".to_string() } else { "belum terhubung".to_string() }
            } else if events.is_empty() {
                "tidak ada agenda".to_string()
            } else {
                format!("{} acara · {} {}", events.len(), events[0].start, events[0].title)
            };
            crate::app::fading_text(g, &summary, c.x + hw + 22.0, c.y + 12.0, 10.5, hex(pal::DIM2), c.x + hw + 22.0, c.x + c.w);
        }
        // Body, clipped by its animated height without a clip: rows that do not fit yet are not drawn.
        if self.today_h > 6.0 {
            let fade = clamp(self.today_h / target.max(24.0), 0.0, 1.0);
            g.save();
            g.mul_alpha(fade);
            if !connected {
                let r = Rect::new(c.x, c.y + 26.0, c.w, 24.0);
                let (clicked, hover, _) = self.ui.click_region(id_of("gt-cal", 97), r);
                let hint = if needs { "Kalender: masuk ulang" } else { "Hubungkan Google untuk agenda" };
                text::draw(g, hint, c.x, c.y + 38.0, Face::Medium, 11.5, if hover { hex(pal::INK) } else { hex(pal::AMBER) }, Align::Left);
                if clicked {
                    crate::gtasks::connect();
                }
            } else if events.is_empty() {
                text::draw(g, "Tidak ada agenda lagi hari ini", c.x, c.y + 38.0, Face::Regular, 11.5, hex(pal::DIM2), Align::Left);
            } else {
                for (i, e) in events.iter().take(2).enumerate() {
                    let y = c.y + 38.0 + i as f32 * 32.0;
                    if y + 17.0 > c.y + 28.0 + self.today_h + 8.0 {
                        break;
                    }
                    g.fill_style(hex("#B45AF0"));
                    g.fill_round_rect(c.x, y - 11.0, 3.0, 28.0, 1.5);
                    crate::app::fading_text(g, &e.title, c.x + 12.0, y - 2.0, 12.0, hex(pal::INK), c.x + 12.0, c.x + c.w);
                    let when = match (&e.end, e.start.as_str()) {
                        (_, "Seharian") => "Seharian".to_string(),
                        (Some(end), s) => format!("{s} – {end}"),
                        (None, s) => s.to_string(),
                    };
                    text::draw(g, &when, c.x + 12.0, y + 12.0, Face::Regular, 10.5, hex(pal::DIM), Align::Left);
                }
                if events.len() > 2 {
                    text::draw(g, &format!("+{} lagi", events.len() - 2), c.x + c.w, c.y + 12.0, Face::Regular, 10.0, hex(pal::DIM3), Align::Right);
                }
            }
            g.restore();
        }

        // ── Tasks ──
        let ty = c.y + 40.0 + self.today_h;
        text::draw(g, "Tugas", c.x, ty, Face::Bold, 12.5, hex(pal::INK), Align::Left);
        let tw = text::measure("Tugas", Face::Bold, 12.5);
        text::draw(g, "›", c.x + tw + 6.0, ty - 1.0, Face::Regular, 13.0, hex(pal::DIM), Align::Left);

        // Timer button: an icon when idle, the countdown when running.
        let running = activity::timer();
        let label = running.map(|(left, _)| activity::format_clock(left));
        let bw = if label.is_some() { 66.0 } else { 30.0 };
        let btn = Rect::new(c.x + c.w - bw, ty - 12.0, bw, 24.0);
        let (clicked, hover, _) = self.ui.click_region(id_of("dash-timer-btn", 93), btn);
        g.fill_style(rgba(255, 255, 255, if self.timer_menu { 0.16 } else if hover { 0.12 } else { 0.07 }));
        g.fill_round_rect(btn.x, btn.y, btn.w, btn.h, 12.0);
        let tint = if label.is_some() { hex(pal::AMBER) } else { hex(pal::INK) };
        let icon_x = if label.is_some() { btn.x + 15.0 } else { btn.cx() };
        crate::icons::fill(g, crate::icons::Icon::Timer, icon_x, btn.cy(), 13.0, tint);
        if let Some(l) = &label {
            text::draw(g, l, btn.x + 27.0, btn.cy(), Face::Bold, 11.5, hex(pal::INK), Align::Left);
        }
        if clicked {
            crate::sound::play("blip");
            self.timer_menu = !self.timer_menu;
            self.timer_custom = false;
        }

        let mut list_top = ty + 14.0;
        if self.timer_menu {
            self.dash_timer_menu(g, Rect::new(c.x, list_top, c.w, 26.0));
            list_top += 34.0;
        }
        self.dash_todos(g, Rect::new(c.x, list_top, c.w, c.y + c.h - list_top), Rect::new(c.x + tw + 22.0, ty - 8.0, c.w - tw - 22.0 - bw - 8.0, 16.0));
    }

    fn dash_timer_menu(&mut self, g: &mut Gfx, row: Rect) {
        let chip = |ui: &mut crate::ui::Ui, g: &mut Gfx, id: &str, r: Rect, label: &str, accent: bool| -> bool {
            let (clicked, hover, _) = ui.click_region(id_of(id, 94), r);
            g.fill_style(if accent { rgba(245, 165, 36, if hover { 0.34 } else { 0.24 }) } else { rgba(255, 255, 255, if hover { 0.14 } else { 0.07 }) });
            g.fill_round_rect(r.x, r.y, r.w, r.h, 9.0);
            text::draw(g, label, r.cx(), r.cy(), Face::Medium, 11.0, hex(pal::INK), Align::Center);
            clicked
        };

        if let Some((left, total)) = activity::timer() {
            // Running: progress and a cancel chip.
            let cancel = Rect::new(row.x + row.w - 52.0, row.y, 52.0, row.h);
            let bar_w = row.w - 52.0 - 10.0;
            let k = clamp(left / total.max(1.0), 0.0, 1.0);
            g.fill_style(rgba(255, 255, 255, 0.12));
            g.fill_round_rect(row.x, row.cy() - 2.0, bar_w, 4.0, 2.0);
            g.fill_style(hex(pal::AMBER));
            g.fill_round_rect(row.x, row.cy() - 2.0, (bar_w * k).max(4.0), 4.0, 2.0);
            if chip(&mut self.ui, g, "dash-timer-stop", cancel, "Batal", false) {
                activity::cancel_timer();
                self.timer_menu = false;
            }
            return;
        }

        if !self.timer_custom {
            let gear_w = 28.0;
            let cw = (row.w - gear_w - 3.0 * 6.0) / 3.0;
            for (i, mins) in [5.0f32, 15.0, 25.0].iter().enumerate() {
                let r = Rect::new(row.x + i as f32 * (cw + 6.0), row.y, cw, row.h);
                if chip(&mut self.ui, g, &format!("dash-timer-{mins}"), r, &format!("{}m", *mins as u32), false) {
                    crate::sound::play("blip");
                    activity::start_timer(*mins);
                    self.timer_menu = false;
                }
            }
            let gear = Rect::new(row.x + row.w - gear_w, row.y, gear_w, row.h);
            let (clicked, hover, _) = self.ui.click_region(id_of("dash-timer-gear", 94), gear);
            g.fill_style(rgba(255, 255, 255, if hover { 0.14 } else { 0.07 }));
            g.fill_round_rect(gear.x, gear.y, gear.w, gear.h, 9.0);
            crate::icons::fill(g, crate::icons::Icon::Gear, gear.cx(), gear.cy(), 13.0, hex(pal::INK));
            if clicked {
                self.timer_custom = true;
            }
            return;
        }

        // Custom length: back, minus, value, plus, start.
        let step = |m: u32, up: bool| -> u32 {
            if up { (m + 5).min(180) } else if m > 5 { m - 5 } else { 1 }
        };
        let back = Rect::new(row.x, row.y, 22.0, row.h);
        let minus = Rect::new(back.x + 22.0 + 4.0, row.y, 24.0, row.h);
        let start = Rect::new(row.x + row.w - 50.0, row.y, 50.0, row.h);
        let plus = Rect::new(start.x - 4.0 - 24.0, row.y, 24.0, row.h);
        let val = Rect::new(minus.x + 24.0 + 4.0, row.y, plus.x - 4.0 - (minus.x + 28.0), row.h);
        if chip(&mut self.ui, g, "dash-timer-back", back, "‹", false) {
            self.timer_custom = false;
        }
        if chip(&mut self.ui, g, "dash-timer-minus", minus, "−", false) {
            self.timer_custom_min = step(self.timer_custom_min, false);
        }
        text::draw(g, &format!("{} m", self.timer_custom_min), val.cx(), val.cy(), Face::Bold, 12.0, hex(pal::INK), Align::Center);
        if chip(&mut self.ui, g, "dash-timer-plus", plus, "+", false) {
            self.timer_custom_min = step(self.timer_custom_min, true);
        }
        if chip(&mut self.ui, g, "dash-timer-start", start, "Mulai", true) {
            crate::sound::play("blip");
            activity::start_timer(self.timer_custom_min as f32);
            self.timer_menu = false;
            self.timer_custom = false;
        }
    }

    /// `r` is everything under the tasks header (rows, then the add field at the bottom);
    /// `status_r` is the spot beside the header where the Google status and its button live.
    fn dash_todos(&mut self, g: &mut Gfx, r: Rect, status_r: Rect) {
        // Rows.
        let order = crate::todos::order(&self.todos);
        let field_h = 24.0;
        let rows = (((r.h - field_h - 8.0) / 21.0).floor().max(0.0) as usize).min(7);
        // Mouse wheel over the rows scrolls the list.
        let list_area = Rect::new(r.x, r.y, r.w, (r.h - field_h - 6.0).max(0.0));
        let (mx0, my0) = self.ui.input.mouse;
        if list_area.contains(mx0, my0) && self.ui.input.wheel != 0.0 {
            let step = -self.ui.input.wheel.round() as i32;
            let max = order.len().saturating_sub(rows) as i32;
            self.task_scroll = (self.task_scroll as i32 + step).clamp(0, max) as usize;
        }
        self.task_scroll = self.task_scroll.min(order.len().saturating_sub(rows));
        let mut y = r.y + 10.0;
        let mut act: Option<(usize, bool)> = None; // (index, toggle star instead of complete)
        for &i in order.iter().skip(self.task_scroll).take(rows) {
            let t = &self.todos[i];
            let (check, star) = (Rect::new(r.x - 2.0, y - 9.0, 18.0, 18.0), Rect::new(r.x + r.w - 18.0, y - 9.0, 18.0, 18.0));
            let (c_click, c_hover, _) = self.ui.click_region(id_of(&format!("todo-c{i}"), 95), check);
            let (s_click, s_hover, _) = self.ui.click_region(id_of(&format!("todo-s{i}"), 96), star);
            g.stroke_style(if c_hover { hex(pal::GREEN) } else { rgba(255, 255, 255, 0.4) });
            g.line_width(1.4);
            g.begin_path();
            g.circle(r.x + 7.0, y, 5.5);
            g.stroke();
            if c_hover {
                g.stroke_style(hex(pal::GREEN));
                g.begin_path();
                g.move_to(r.x + 4.5, y);
                g.line_to(r.x + 6.5, y + 2.2);
                g.line_to(r.x + 10.0, y - 2.4);
                g.stroke();
            }
            crate::app::fading_text(g, &t.text, r.x + 20.0, y, 11.5, hex(pal::INK), r.x + 20.0, r.x + r.w - 24.0);
            let star_color = if t.star { hex(pal::AMBER) } else if s_hover { rgba(255, 255, 255, 0.55) } else { rgba(255, 255, 255, 0.2) };
            crate::icons::fill(g, crate::icons::Icon::Star, star.cx(), star.cy(), 11.0, star_color);
            if c_click {
                act = Some((i, false));
            } else if s_click {
                act = Some((i, true));
            }
            y += 21.0;
        }
        if order.is_empty() && rows > 0 {
            text::draw(g, "Belum ada tugas", r.x + 2.0, r.y + 10.0, Face::Regular, 11.0, hex(pal::DIM3), Align::Left);
        }
        // "+N lagi" shows the next page of tasks; at the end it says "ke atas". The wheel scrolls too.
        let below = order.len().saturating_sub(self.task_scroll + rows);
        let above = self.task_scroll;
        if below > 0 || above > 0 {
            let label = if below > 0 { format!("+{below} lagi") } else { "ke atas".to_string() };
            let lw = text::measure(&label, Face::Medium, 10.0) + 8.0;
            let hit = Rect::new(r.x + r.w - lw, r.y + r.h - field_h - 17.0, lw, 15.0);
            let (clicked, hover, _) = self.ui.click_region(id_of("tasks-more", 101), hit);
            text::draw(g, &label, r.x + r.w, hit.cy(), Face::Medium, 10.0, if hover { hex(pal::INK) } else { hex(pal::DIM2) }, Align::Right);
            if above > 0 {
                text::draw(g, &format!("{}–{} dari {}", above + 1, (above + rows).min(order.len()), order.len()), r.x + 2.0, hit.cy(), Face::Regular, 9.5, hex(pal::DIM3), Align::Left);
            }
            if clicked {
                crate::sound::play("blip");
                self.task_scroll = if below > 0 { (self.task_scroll + rows).min(order.len().saturating_sub(rows)) } else { 0 };
            }
        }

        // Add field, at the bottom.
        let fb = Rect::new(r.x, r.y + r.h - field_h, r.w, field_h);
        g.fill_style(rgba(255, 255, 255, if self.todo_input.focused { 0.11 } else { 0.06 }));
        g.fill_round_rect(fb.x, fb.y, fb.w, fb.h, 12.0);
        let field = Rect::new(fb.x + 10.0, fb.y + 3.0, fb.w - 20.0, 18.0);
        let (mx, my) = self.ui.input.mouse;
        let over = field.contains(mx, my);
        if self.ui.input.pressed {
            if over {
                self.todo_input.focused = true;
                self.platform.set_activating(true);
                self.todo_input.click_at(mx, 11.5, crate::platform::shift_down());
            } else if self.todo_input.focused {
                self.todo_input.focused = false;
                self.platform.set_activating(false);
            }
        }
        if over {
            self.ui.hovering_text = true;
        }
        self.todo_input.draw(g, field, 11.5, pal::INK);

        // Google status beside the header: also the button that connects it.
        let (label, tint) = match crate::gtasks::status() {
            crate::gtasks::Status::NoClient => ("impor kredensial Google".to_string(), hex(pal::AMBER)),
            crate::gtasks::Status::NeedsLogin => ("masuk ke Google".to_string(), hex(pal::AMBER)),
            crate::gtasks::Status::LoggingIn => ("menunggu login…".to_string(), hex(pal::DIM)),
            crate::gtasks::Status::Linked if crate::gtasks::calendar_needs_login() => ("tersinkron".to_string(), hex(pal::GREEN2)),
            crate::gtasks::Status::Linked => ("tersinkron".to_string(), hex(pal::GREEN2)),
            crate::gtasks::Status::Error(e) => (format!("gagal: {e}"), hex(pal::RED_TEXT)),
        };
        let (s_click, s_hover, _) = self.ui.click_region(id_of("gt-status", 97), status_r);
        crate::app::fading_text(g, &label, status_r.x, status_r.cy(), 9.5, if s_hover { hex(pal::INK) } else { tint }, status_r.x, status_r.x + status_r.w);
        if s_click {
            crate::gtasks::connect();
        }

        if let Some((i, star_toggle)) = act {
            crate::sound::play("blip");
            if star_toggle {
                self.todos[i].star = !self.todos[i].star;
            } else {
                let done = self.todos.remove(i);
                if let Some(gid) = done.gid {
                    crate::gtasks::enqueue_complete(&gid);
                    self.done_gids.push(gid);
                }
            }
            crate::todos::save(&self.todos);
        }
    }

    /// Replaces the list with Google's open tasks, keeping stars, and keeps local items that
    /// have not reached Google yet (unless Google already has one with the same title).
    pub fn merge_remote_todos(&mut self, remote: Vec<(String, String)>) {
        let old = std::mem::take(&mut self.todos);
        let mut merged: Vec<crate::todos::Todo> = Vec::new();
        for (gid, title) in &remote {
            if self.done_gids.contains(gid) {
                continue;
            }
            let star = old.iter().any(|t| t.gid.as_deref() == Some(gid) && t.star);
            merged.push(crate::todos::Todo { text: title.clone(), star, gid: Some(gid.clone()) });
        }
        for t in old {
            if t.gid.is_none() && !remote.iter().any(|(_, title)| *title == t.text) {
                merged.push(t);
            }
        }
        // A completion Google has acknowledged no longer needs remembering.
        self.done_gids.retain(|g| remote.iter().any(|(id, _)| id == g));
        self.todos = merged;
        crate::todos::save(&self.todos);
    }

    /// Enter in the add field.
    pub fn add_todo(&mut self) {
        let text = self.todo_input.text().trim().to_string();
        if text.is_empty() {
            return;
        }
        let text: String = text.chars().take(120).collect();
        crate::gtasks::enqueue_add(&text);
        self.todos.push(crate::todos::Todo { text, star: false, gid: None });
        self.todo_input.clear();
        crate::todos::save(&self.todos);
        crate::sound::play("send");
    }
}
