// The dashboard: what the island shows when you open it and nothing needs you.
// Greeting, Now Playing (artwork, seek bar, transport), a timer with presets and a
// few system gauges. Everything here is read from the system or from `activity`.

use tiny_skia::Color;

use crate::activity::{self, Transport};
use crate::anim::clamp;
use crate::app::{fading_text, App};
use crate::gfx::{hex, rgba, Gfx};
use crate::text::{self, Align, Face};
use crate::ui::{id_of, pal, Rect};

const LEFT_W: f32 = 296.0;

// ── Greeting ──────────────────────────────────────────────────────────────────

pub(crate) fn greeting(display_name: &str) -> (String, String, String) {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let t = unsafe { GetLocalTime() };
    let hello = match t.wHour {
        4..=10 => "Selamat pagi",
        11..=14 => "Selamat siang",
        15..=17 => "Selamat sore",
        _ => "Selamat malam",
    };
    let name = if display_name.trim().is_empty() { std::env::var("USERNAME").unwrap_or_default() } else { display_name.trim().to_string() };
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
    let short = format!("{}, {} {}", &days[(t.wDayOfWeek as usize) % 7][..3], t.wDay, &months[(t.wMonth as usize).clamp(1, 12) - 1][..3]);
    (if name.is_empty() { hello.to_string() } else { format!("{hello}, {name}") }, date, short)
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

/// Two crossing arrows.
fn glyph_shuffle(g: &mut Gfx, cx: f32, cy: f32, col: Color) {
    g.stroke_style(col);
    g.line_width(1.5);
    g.line_cap_round();
    g.line_join_round();
    for (a, b) in [(-1.0f32, 1.0f32), (1.0, -1.0)] {
        g.begin_path();
        g.move_to(cx - 7.0, cy + a * 4.5);
        g.line_to(cx - 3.0, cy + a * 4.5);
        g.line_to(cx + 3.0, cy + b * 4.5);
        g.line_to(cx + 7.0, cy + b * 4.5);
        g.stroke();
        // arrow head
        g.begin_path();
        g.move_to(cx + 4.6, cy + b * 4.5 - 2.4);
        g.line_to(cx + 7.2, cy + b * 4.5);
        g.line_to(cx + 4.6, cy + b * 4.5 + 2.4);
        g.stroke();
    }
}

/// A loop of two arrows; a "1" in the middle when it repeats the track.
fn glyph_repeat(g: &mut Gfx, cx: f32, cy: f32, col: Color, one: bool) {
    g.stroke_style(col);
    g.line_width(1.5);
    g.line_cap_round();
    g.line_join_round();
    g.begin_path();
    g.move_to(cx - 6.5, cy);
    g.line_to(cx - 6.5, cy - 3.0);
    g.line_to(cx + 5.0, cy - 3.0);
    g.stroke();
    g.begin_path();
    g.move_to(cx + 3.0, cy - 5.4);
    g.line_to(cx + 5.6, cy - 3.0);
    g.line_to(cx + 3.0, cy - 0.6);
    g.stroke();
    g.begin_path();
    g.move_to(cx + 6.5, cy);
    g.line_to(cx + 6.5, cy + 3.0);
    g.line_to(cx - 5.0, cy + 3.0);
    g.stroke();
    g.begin_path();
    g.move_to(cx - 3.0, cy + 0.6);
    g.line_to(cx - 5.6, cy + 3.0);
    g.line_to(cx - 3.0, cy + 5.4);
    g.stroke();
    if one {
        text::draw(g, "1", cx, cy + 0.5, Face::Bold, 7.0, col, Align::Center);
    }
}

fn mmss(secs: f32) -> String {
    let s = secs.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

impl App {
    /// The header-row title beside Mochi on the pages that have no greeting: a name and one line of context.
    pub fn dash_page_title(&self, g: &mut Gfx, v: Rect, title: &str, sub: &str) {
        let gx = v.x + 304.0;
        let gy = v.y - 42.0;
        text::draw(g, title, gx, gy + 15.0, Face::Bold, 13.0, hex(pal::INK), Align::Left);
        crate::app::fading_text(g, sub, gx, gy + 31.0, 10.0, hex(pal::DIM), gx, v.x + v.w - 56.0);
    }

    pub fn draw_dashboard(&mut self, g: &mut Gfx, v: Rect, n: f32) {
        // The greeting lives in the header row, to the right of the tabs; Mochi sits just before it
        // (see layout). Two short lines: who and what day, then the weather.
        let (hello, _long, date) = greeting(&self.st.settings.display_name);
        let gx = v.x + 304.0;
        let gy = v.y - 42.0;
        text::draw(g, &hello, gx, gy + 15.0, Face::Bold, 13.0, hex(pal::INK), Align::Left);
        let mut line2 = date.clone();
        if let Some(w) = crate::weather::report() {
            line2 = format!("{date}  ·  {:.0}° {}", w.now.temp, crate::weather::label_of(w.now.code));
        }
        crate::app::fading_text(g, &line2, gx, gy + 31.0, 10.0, hex(pal::DIM), gx, v.x + v.w - 56.0);

        let left = Rect::new(v.x, v.y, LEFT_W, v.h.max(0.0));
        // The right column has no card: text sits straight on the island, as in OmniNotch.
        let col = Rect::new(v.x + LEFT_W + 16.0, v.y + 2.0, v.w - LEFT_W - 16.0 - 4.0, (v.h - 2.0).max(0.0));
        crate::app::prof("card", || crate::ui::card(g, left, crate::layout::Wash::None, false));

        crate::app::prof("player", || self.dash_now_playing(g, left, n));
        let media = if self.lyrics_open && self.st.settings.feature_on("lyrics") { activity::media() } else { None };
        if let Some(m) = media {
            self.dash_lyrics(g, col, &m);
        } else {
            crate::app::prof("column", || self.dash_column(g, col));
        }
    }

    fn dash_now_playing(&mut self, g: &mut Gfx, r: Rect, n: f32) {
        let _ = n;
        // Round cover with a progress ring around it, on the left.
        let ring_c = (r.x + 64.0, r.cy());
        let cover_r = 38.0;
        let ring_r = 45.0;
        let col = r.x + 124.0;
        let colw = r.w - 124.0 - 14.0;

        let Some(m) = activity::media() else {
            g.fill_style(rgba(255, 255, 255, 0.06));
            g.begin_path();
            g.circle(ring_c.0, ring_c.1, cover_r);
            g.fill();
            text::draw(g, "♪", ring_c.0, ring_c.1, Face::Bold, 32.0, rgba(255, 255, 255, 0.25), Align::Center);
            text::draw(g, "Tidak ada media", col, r.cy() - 8.0, Face::Regular, 15.0, hex(pal::INK), Align::Left);
            text::draw(g, "Putar musik di Spotify atau browser.", col, r.cy() + 12.0, Face::Regular, 11.0, hex(pal::DIM), Align::Left);
            return;
        };
        let art_img = activity::art();
        let tint = art_img.as_ref().map(|a| a.tint).unwrap_or((120, 120, 150));
        let mix = |t: f32| -> Color {
            let f = |c: u8| (c as f32 + (255.0 - c as f32) * t).round().clamp(0.0, 255.0) as u8;
            Color::from_rgba8(f(tint.0), f(tint.1), f(tint.2), 255)
        };

        // A quiet glow in the cover's colour behind the ring.
        let mut lap = std::time::Instant::now();
        crate::ui::glow(g, r, 24.0, 50.0, 280.0, Color::from_rgba8(tint.0, tint.1, tint.2, 80));

        crate::app::prof_lap("p.glow", &mut lap);
        // Cover, as a circle.
        match &art_img {
            Some(a) => {
                // The cover only changes with the track: draw it once into a cached layer.
                let side = cover_r * 2.0;
                let key = crate::gfx::layer_key(&[f32::from_bits(a.key as u32), f32::from_bits((a.key >> 32) as u32), side, 5.0]);
                let (px, size) = (a.rgba.as_slice(), a.size);
                g.cached_layer(key, side, side, ring_c.0 - cover_r, ring_c.1 - cover_r, |l| l.fill_image_round_rect(px, size, 0.0, 0.0, side, side, cover_r));
            }
            None => {
                g.fill_style(rgba(255, 255, 255, 0.07));
                g.begin_path();
                g.circle(ring_c.0, ring_c.1, cover_r);
                g.fill();
                text::draw(g, "♪", ring_c.0, ring_c.1, Face::Bold, 32.0, rgba(255, 255, 255, 0.25), Align::Center);
            }
        }

        crate::app::prof_lap("p.cover", &mut lap);
        // Progress ring: the track, the played part, and a click anywhere on it seeks.
        let pos = m.position_now();
        let k = if m.duration > 0.0 { clamp(pos / m.duration, 0.0, 1.0) } else { 0.0 };
        let (mx, my) = self.ui.input.mouse;
        let dist = ((mx - ring_c.0).powi(2) + (my - ring_c.1).powi(2)).sqrt();
        let on_ring = dist >= ring_r - 8.0 && dist <= ring_r + 8.0;
        let ring_box = Rect::new(ring_c.0 - ring_r - 8.0, ring_c.1 - ring_r - 8.0, (ring_r + 8.0) * 2.0, (ring_r + 8.0) * 2.0);
        let (rc, _, _) = self.ui.click_region(id_of("dash-ring", 91), ring_box);
        // The empty track never changes: cached.
        let tside = (ring_r + 4.0) * 2.0;
        let tkey = crate::gfx::layer_key(&[ring_r, 3.5, 7.0]);
        g.cached_layer(tkey, tside, tside, ring_c.0 - tside / 2.0, ring_c.1 - tside / 2.0, |l| {
            l.line_width(3.5);
            l.stroke_style(rgba(255, 255, 255, 0.12));
            l.begin_path();
            l.circle(tside / 2.0, tside / 2.0, ring_r);
            l.stroke();
        });
        g.line_width(3.5);
        g.line_cap_round();
        if k > 0.002 {
            let a0 = -std::f32::consts::FRAC_PI_2;
            g.stroke_style(if on_ring { mix(0.85) } else { mix(0.6) });
            g.begin_path();
            g.arc(ring_c.0, ring_c.1, ring_r, a0, a0 + k * std::f32::consts::TAU, false);
            g.stroke();
        }
        if rc && on_ring && m.duration > 0.0 {
            let mut ang = (mx - ring_c.0).atan2(-(my - ring_c.1)); // 0 at the top, clockwise
            if ang < 0.0 {
                ang += std::f32::consts::TAU;
            }
            activity::transport(Transport::Seek(ang / std::f32::consts::TAU * m.duration));
        }
        // Elapsed time sits on the ring, at the bottom.
        let tl = mmss(pos);
        let tw = text::measure(&tl, Face::Medium, 10.5) + 14.0;
        g.fill_style(hex("#0E0F11"));
        g.fill_round_rect(ring_c.0 - tw / 2.0, ring_c.1 + ring_r - 8.0, tw, 16.0, 8.0);
        text::draw(g, &tl, ring_c.0, ring_c.1 + ring_r, Face::Medium, 10.5, rgba(255, 255, 255, 0.85), Align::Center);

        crate::app::prof_lap("p.ring", &mut lap);
        // Title, artist, where it plays from.
        if !m.source.is_empty() {
            text::draw(g, &m.source.to_uppercase(), col, r.y + 20.0, Face::Medium, 8.5, rgba(255, 255, 255, 0.45), Align::Left);
        }
        fading_text(g, &m.title, col, r.y + 42.0, 14.5, hex(pal::INK), col, col + colw);
        if !m.artist.is_empty() {
            fading_text(g, &m.artist, col, r.y + 60.0, 11.0, rgba(255, 255, 255, 0.6), col, col + colw);
        }
        if m.duration > 0.0 {
            text::draw(g, &mmss(m.duration), col + colw, r.y + 20.0, Face::Regular, 9.0, rgba(255, 255, 255, 0.4), Align::Right);
        }

        crate::app::prof_lap("p.text", &mut lap);
        // One row of controls: shuffle, previous, play, next, repeat.
        let cy = r.y + 100.0;
        let step = colw / 5.0;
        let xs: Vec<f32> = (0..5).map(|i| col + step * (i as f32 + 0.5)).collect();
        for (idx, (id, kind)) in [("dash-shuffle", 0), ("dash-prev", 1), ("dash-play", 2), ("dash-next", 3), ("dash-repeat", 4)].into_iter().enumerate() {
            let cx = xs[idx];
            let hit = Rect::new(cx - step / 2.0, cy - 22.0, step, 44.0);
            let (clicked, hover, pressed) = self.ui.click_region(id_of(id, 92), hit);
            match kind {
                2 => {
                    let rad = if pressed { 14.0 } else if hover { 17.0 } else { 16.0 };
                    g.fill_style(hex("#F5F6F8"));
                    g.begin_path();
                    g.circle(cx, cy, rad);
                    g.fill();
                    g.fill_style(hex("#0B0C0E"));
                    if m.playing { glyph_pause(g, cx, cy, 12.0) } else { glyph_play(g, cx + 1.0, cy, 12.0) }
                }
                1 | 3 => {
                    g.fill_style(if pressed { rgba(255, 255, 255, 0.5) } else if hover { hex("#FFFFFF") } else { rgba(255, 255, 255, 0.8) });
                    glyph_skip(g, cx, cy, 12.0, if kind == 1 { -1.0 } else { 1.0 });
                }
                _ => {
                    let (can, on) = if kind == 0 { (m.can_shuffle, m.shuffle) } else { (m.can_repeat, m.repeat != 0) };
                    let col_btn = if !can { rgba(255, 255, 255, 0.18) } else if on { mix(0.7) } else if hover { hex("#FFFFFF") } else { rgba(255, 255, 255, 0.55) };
                    if kind == 0 { glyph_shuffle(g, cx, cy, col_btn) } else { glyph_repeat(g, cx, cy, col_btn, m.repeat == 1) }
                }
            }
            if clicked {
                match kind {
                    0 if m.can_shuffle => activity::transport(Transport::Shuffle(!m.shuffle)),
                    1 => activity::transport(Transport::Previous),
                    2 => activity::transport(Transport::PlayPause),
                    3 => activity::transport(Transport::Next),
                    4 if m.can_repeat => activity::transport(Transport::Repeat((m.repeat + 1) % 3)),
                    _ => {}
                }
            }
        }
        crate::app::prof_lap("p.buttons", &mut lap);

        // Lyrics: one small button, bottom left of the player.
        if self.st.settings.feature_on("lyrics") {
            let (bx, by) = (r.x + 20.0, r.y + r.h - 14.0);
            if self.lyrics_open {
                g.fill_style(rgba(255, 255, 255, 0.14));
                g.fill_round_rect(bx - 11.0, by - 9.0, 22.0, 18.0, 9.0);
            }
            if self.ui.icon_button(g, "lyrics-btn", bx, by, 20.0, crate::icons::Icon::Bubble, 11.0) {
                self.lyrics_open = !self.lyrics_open;
                crate::lyrics::retry_if_failed();
                crate::sound::play("blip");
            }
        }
    }

    /// The lyrics panel that replaces the agenda column: the line being sung is bright and
    /// centred, the others fade away from it; a click on a line jumps to it.
    fn dash_lyrics(&mut self, g: &mut Gfx, c: Rect, m: &activity::Media) {
        use crate::lyrics::{self, State};
        lyrics::request(&m.title, &m.artist, m.duration);
        let Some(ly) = lyrics::current() else { return };
        let say = |g: &mut Gfx, s: &str| {
            text::draw(g, s, c.cx(), c.cy(), Face::Medium, 12.5, hex(pal::DIM), Align::Center);
        };
        match ly.state {
            State::Loading => return say(g, "Memuat lirik…"),
            State::NotFound => return say(g, "Lirik tidak ditemukan"),
            State::Failed => return say(g, "Lirik tidak bisa dimuat (offline?)"),
            _ => {}
        }
        let synced = ly.state == State::Synced;
        let pos = m.position_now();
        let act = if synced { lyrics::active_index(&ly.lines, pos) } else { None };
        let target = act.map(|i| i as f32).unwrap_or(0.0);
        self.lyrics_scroll += (target - self.lyrics_scroll) * 0.4;
        if (target - self.lyrics_scroll).abs() < 0.01 {
            self.lyrics_scroll = target;
        }
        let pitch = 24.0;
        g.save();
        g.clip_rect_xywh(c.x, c.y, c.w, c.h);
        let first = (self.lyrics_scroll.floor() as i32 - 4).max(0) as usize;
        let last = ((self.lyrics_scroll.ceil() as i32 + 4).max(0) as usize).min(ly.lines.len().saturating_sub(1));
        for i in first..=last {
            let (t, line) = &ly.lines[i];
            let y = c.cy() + (i as f32 - self.lyrics_scroll) * pitch;
            let dist = (i as f32 - self.lyrics_scroll).abs();
            let fade = clamp(1.0 - dist / 3.2, 0.0, 1.0);
            if fade <= 0.02 {
                continue;
            }
            let on = act == Some(i);
            let shown = if line.is_empty() { "♪" } else { line.as_str() };
            let size = if on { 14.5 } else { 12.0 };
            let face = if on { Face::Bold } else { Face::Regular };
            let color = if on { hex(pal::INK) } else { crate::gfx::with_alpha(hex(pal::DIM), fade) };
            let hit = Rect::new(c.x, y - pitch / 2.0, c.w, pitch);
            let (clicked, hover, _) = self.ui.click_region(id_of(&format!("lyric-{i}"), 135), hit);
            let col = if hover && !on { crate::gfx::with_alpha(hex(pal::INK), fade.max(0.7)) } else { color };
            let line_txt = text::ellipsize(shown, face, size, c.w - 8.0);
            text::draw(g, &line_txt, c.x + 4.0, y, face, size, col, Align::Left);
            if clicked && synced && *t >= 0.0 {
                activity::transport(Transport::Seek(*t));
            }
        }
        g.restore();
    }

    /// Right column: the next thing on today's agenda in one soft row, the tasks as pills, and one
    /// bottom row to add a task, see today's count and start a timer.
    fn dash_column(&mut self, g: &mut Gfx, c: Rect) {
        self.today_target = 0.0;
        self.dash_today_row(g, Rect::new(c.x, c.y, c.w, 26.0));
        let bottom = Rect::new(c.x, c.y + c.h - 24.0, c.w, 24.0);
        let top = c.y + 34.0;
        let mut list = Rect::new(c.x, top, c.w, (bottom.y - 7.0 - top).max(0.0));
        if self.timer_menu {
            self.dash_timer_menu(g, Rect::new(list.x, list.y, list.w, 22.0));
            list.y += 28.0;
            list.h = (list.h - 28.0).max(0.0);
        }
        self.dash_todos(g, list, bottom);
    }

    /// "Today" in one row: a little date tile, then the next event (or why there is none).
    fn dash_today_row(&mut self, g: &mut Gfx, r: Rect) {
        use crate::gtasks::Status;
        let status = crate::gtasks::status();
        let needs = crate::gtasks::calendar_needs_login();
        let connected = status == Status::Linked && !needs;
        let events = upcoming(&crate::gtasks::events());
        let (clicked, hover, _) = self.ui.click_region(id_of("today-row", 97), r);
        g.fill_style(rgba(255, 255, 255, if hover && !connected { 0.08 } else { 0.05 }));
        g.fill_round_rect(r.x, r.y, r.w, r.h, 9.0);

        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        let tile = Rect::new(r.x + 4.0, r.y + 4.0, 18.0, 18.0);
        g.fill_style(rgba(180, 90, 240, 0.30));
        g.fill_round_rect(tile.x, tile.y, tile.w, tile.h, 6.0);
        text::draw(g, &t.wDay.to_string(), tile.cx(), tile.cy(), Face::Bold, 9.5, hex("#EBDDFF"), Align::Center);

        let x = r.x + 30.0;
        let right = r.x + r.w - 10.0;
        let cy = r.cy();
        match &status {
            Status::LoggingIn => {
                text::draw(g, "Menunggu login Google…", x, cy, Face::Regular, 11.0, hex(pal::DIM), Align::Left);
            }
            Status::Error(e) => {
                crate::app::fading_text(g, &format!("Google: {e}"), x, cy, 11.0, hex(pal::RED_TEXT), x, right);
            }
            _ if !connected => {
                let label = if needs { "Kalender perlu masuk ulang" } else { "Hubungkan Google" };
                text::draw(g, label, x, cy, Face::Medium, 11.0, if hover { hex(pal::INK) } else { hex(pal::AMBER) }, Align::Left);
                text::draw(g, "›", right, cy - 1.0, Face::Regular, 13.0, hex(pal::DIM), Align::Right);
                if clicked {
                    crate::gtasks::connect();
                }
            }
            _ if events.is_empty() => {
                text::draw(g, "Tidak ada agenda lagi hari ini", x, cy, Face::Regular, 11.0, hex(pal::DIM2), Align::Left);
            }
            _ => {
                let e = &events[0];
                let more = events.len() - 1;
                let tw = text::measure(&e.start, Face::Medium, 10.5);
                text::draw(g, &e.start, x, cy, Face::Medium, 10.5, hex("#C9A7F5"), Align::Left);
                let end = if more > 0 { right - 22.0 } else { right };
                crate::app::fading_text(g, &e.title, x + tw + 8.0, cy, 11.5, hex(pal::INK), x + tw + 8.0, end);
                if more > 0 {
                    text::draw(g, &format!("+{more}"), right, cy, Face::Medium, 10.0, hex(pal::DIM3), Align::Right);
                }
            }
        }
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

    /// The tasks as soft pills in `r`; the bottom row holds the add field, today's count and the
    /// timer button.
    fn dash_todos(&mut self, g: &mut Gfx, r: Rect, bottom: Rect) {
        let order = crate::todos::order(&self.todos);
        let (pitch, row_h) = (25.0, 22.0);
        let slots = ((r.h + 3.0) / pitch).floor().max(0.0) as usize;
        let overflow = order.len() > slots;
        let rows = slots;
        let below = order.len().saturating_sub(self.task_scroll + rows);
        let (mx0, my0) = self.ui.input.mouse;
        if r.contains(mx0, my0) && self.ui.input.wheel != 0.0 && rows > 0 {
            let step = -self.ui.input.wheel.round() as i32;
            let max = order.len().saturating_sub(rows) as i32;
            self.task_scroll = (self.task_scroll as i32 + step).clamp(0, max) as usize;
        }
        self.task_scroll = self.task_scroll.min(order.len().saturating_sub(rows));

        let mut y = r.y;
        let mut act: Option<(usize, bool)> = None; // (index, toggle star instead of complete)
        for (k, &i) in order.iter().skip(self.task_scroll).take(rows).enumerate() {
            let t = &self.todos[i];
            let row = Rect::new(r.x, y, r.w, row_h);
            // The last row carries the way to the rest of the list.
            let more_chip = overflow && k + 1 == rows;
            let (_, row_hover, _) = self.ui.click_region(id_of(&format!("todo-r{i}"), 94), row);
            g.fill_style(rgba(255, 255, 255, if row_hover { 0.085 } else { 0.045 }));
            g.fill_round_rect(row.x, row.y, row.w, row.h, 8.0);

            let check = Rect::new(r.x + 2.0, y + 2.0, 20.0, 18.0);
            let (c_click, c_hover, _) = self.ui.click_region(id_of(&format!("todo-c{i}"), 95), check);
            let (cx, cy) = (r.x + 12.0, y + row_h / 2.0);
            if c_hover {
                g.fill_style(rgba(34, 197, 94, 0.28));
                g.begin_path();
                g.circle(cx, cy, 6.0);
                g.fill();
            }
            g.stroke_style(if c_hover { hex(pal::GREEN) } else { rgba(255, 255, 255, 0.42) });
            g.line_width(1.4);
            g.begin_path();
            g.circle(cx, cy, 5.5);
            g.stroke();
            if c_hover {
                g.line_cap_round();
                g.line_join_round();
                g.begin_path();
                g.move_to(cx - 2.6, cy + 0.1);
                g.line_to(cx - 0.6, cy + 2.1);
                g.line_to(cx + 2.8, cy - 2.2);
                g.stroke();
            }

            let show_star = (t.star || row_hover) && !more_chip;
            let chip_label = if below > 0 { format!("+{below}") } else { "atas".to_string() };
            let chip_w = text::measure(&chip_label, Face::Medium, 10.0) + 14.0;
            let text_end = r.x + r.w - if more_chip { chip_w + 10.0 } else if show_star { 24.0 } else { 8.0 };
            crate::app::fading_text(g, &t.text, r.x + 24.0, cy, 11.5, hex(pal::INK), r.x + 24.0, text_end);
            if show_star {
                let star = Rect::new(r.x + r.w - 21.0, y + 2.0, 18.0, 18.0);
                let (s_click, s_hover, _) = self.ui.click_region(id_of(&format!("todo-s{i}"), 96), star);
                let col = if t.star { hex(pal::AMBER) } else if s_hover { rgba(255, 255, 255, 0.85) } else { rgba(255, 255, 255, 0.4) };
                crate::icons::fill(g, crate::icons::Icon::Star, star.cx(), star.cy(), 11.0, col);
                if s_click {
                    act = Some((i, true));
                }
            }
            if more_chip {
                let chip = Rect::new(r.x + r.w - chip_w - 4.0, y + 3.0, chip_w, row_h - 6.0);
                let (m_click, m_hover, _) = self.ui.click_region(id_of("tasks-more", 101), chip);
                g.fill_style(rgba(255, 255, 255, if m_hover { 0.18 } else { 0.10 }));
                g.fill_round_rect(chip.x, chip.y, chip.w, chip.h, chip.h / 2.0);
                text::draw(g, &chip_label, chip.cx(), chip.cy(), Face::Medium, 10.0, hex(if m_hover { pal::INK } else { pal::DIM }), Align::Center);
                if m_click {
                    crate::sound::play("blip");
                    self.task_scroll = if below > 0 { (self.task_scroll + rows).min(order.len().saturating_sub(rows)) } else { 0 };
                }
            }
            if c_click {
                act = Some((i, false));
            }
            y += pitch;
        }
        if order.is_empty() && slots > 0 {
            text::draw(g, "Belum ada tugas. Tulis di bawah.", r.x + 4.0, r.y + row_h / 2.0, Face::Regular, 11.0, hex(pal::DIM3), Align::Left);
        }
        // Bottom row: [ + add a task … ] [ 2/5 ] [ timer ].
        let running = activity::timer();
        let label = running.map(|(left, _)| activity::format_clock(left));
        let bw = if label.is_some() { 64.0 } else { 24.0 };
        let btn = Rect::new(bottom.x + bottom.w - bw, bottom.y, bw, bottom.h);
        let (t_click, t_hover, _) = self.ui.click_region(id_of("dash-timer-btn", 93), btn);
        g.fill_style(rgba(255, 255, 255, if self.timer_menu { 0.16 } else if t_hover { 0.12 } else { 0.06 }));
        g.fill_round_rect(btn.x, btn.y, btn.w, btn.h, 12.0);
        let tint = if label.is_some() { hex(pal::AMBER) } else { hex(pal::DIM) };
        let icon_x = if label.is_some() { btn.x + 14.0 } else { btn.cx() };
        crate::icons::fill(g, crate::icons::Icon::Timer, icon_x, btn.cy(), 12.0, tint);
        if let Some(l) = &label {
            text::draw(g, l, btn.x + 25.0, btn.cy(), Face::Bold, 11.0, hex(pal::INK), Align::Left);
        }
        if t_click {
            crate::sound::play("blip");
            self.timer_menu = !self.timer_menu;
            self.timer_custom = false;
        }

        let done_today = self.buddy.tasks_done;
        let total_today = self.todos.len() as u32 + done_today;
        let mut field_end = btn.x - 6.0;
        if total_today > 0 {
            let all = done_today >= total_today;
            let lbl = format!("{done_today}/{total_today}");
            let cw = text::measure(&lbl, Face::Medium, 10.5) + 14.0;
            let pill = Rect::new(field_end - cw, bottom.y + 4.0, cw, 16.0);
            g.fill_style(if all { rgba(34, 197, 94, 0.22) } else { rgba(255, 255, 255, 0.07) });
            g.fill_round_rect(pill.x, pill.y, pill.w, pill.h, 8.0);
            text::draw(g, &lbl, pill.cx(), pill.cy(), Face::Medium, 10.5, hex(if all { pal::GREEN2 } else { pal::DIM }), Align::Center);
            field_end = pill.x - 6.0;
        }
        let fb = Rect::new(bottom.x, bottom.y, (field_end - bottom.x).max(40.0), bottom.h);
        g.fill_style(rgba(255, 255, 255, if self.todo_input.focused { 0.11 } else { 0.06 }));
        g.fill_round_rect(fb.x, fb.y, fb.w, fb.h, 12.0);
        crate::icons::fill(g, crate::icons::Icon::Plus, fb.x + 12.0, fb.cy(), 10.0, hex(pal::DIM2));
        let field = Rect::new(fb.x + 22.0, fb.y + 3.0, fb.w - 30.0, 18.0);
        let (mx, my) = self.ui.input.mouse;
        let over = fb.contains(mx, my);
        self.ui.note_region(id_of("todo-field", 110), fb);
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

        if let Some((i, star_toggle)) = act {
            crate::sound::play("blip");
            if star_toggle {
                self.todos[i].star = !self.todos[i].star;
            } else {
                let done = self.todos.remove(i);
                self.buddy_task_done();
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
