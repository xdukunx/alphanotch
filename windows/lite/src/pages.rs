// The dashboard's other pages, reached from the header tabs: Stocks, Weather, Teleprompter.

use tiny_skia::Color;

use crate::activity;
use crate::anim::clamp;
use crate::app::{fading_text, App};
use crate::dashboard::{glyph_pause, glyph_play};
use crate::gfx::{hex, rgba, Gfx};
use crate::layout::Wash;
use crate::stocks;
use crate::text::{self, Align, Face};
use crate::ui::{self, id_of, pal, Rect};
use crate::weather::{self, Kind};

const GREEN: &str = "#34D399";
const RED: &str = "#F87171";

// ── Weather icons, drawn in code ──────────────────────────────────────────────

fn cloud(g: &mut Gfx, cx: f32, cy: f32, s: f32, color: Color) {
    g.fill_style(color);
    for (dx, dy, r) in [(-0.2, 0.02, 0.2), (0.02, -0.1, 0.27), (0.27, 0.04, 0.18)] {
        g.begin_path();
        g.circle(cx + dx * s, cy + dy * s, r * s);
        g.fill();
    }
    g.fill_round_rect(cx - 0.38 * s, cy + 0.02 * s, 0.84 * s, 0.22 * s, 0.11 * s);
}

fn sun(g: &mut Gfx, cx: f32, cy: f32, s: f32) {
    g.fill_style(hex("#FBBF24"));
    g.begin_path();
    g.circle(cx, cy, 0.2 * s);
    g.fill();
    g.stroke_style(hex("#FBBF24"));
    g.line_width((0.05 * s).max(1.2));
    g.line_cap_round();
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::FRAC_PI_4;
        let (c, sn) = (a.cos(), a.sin());
        g.begin_path();
        g.move_to(cx + c * 0.32 * s, cy + sn * 0.32 * s);
        g.line_to(cx + c * 0.44 * s, cy + sn * 0.44 * s);
        g.stroke();
    }
}

fn moon(g: &mut Gfx, cx: f32, cy: f32, s: f32) {
    g.fill_style(hex("#CBD5E1"));
    g.begin_path();
    g.circle(cx, cy, 0.24 * s);
    g.fill();
    g.fill_style(hex("#0E0F11"));
    g.begin_path();
    g.circle(cx + 0.11 * s, cy - 0.08 * s, 0.2 * s);
    g.fill();
}

pub fn weather_icon(g: &mut Gfx, kind: Kind, day: bool, cx: f32, cy: f32, s: f32) {
    let grey = hex("#94A3B8");
    match kind {
        Kind::Clear => {
            if day { sun(g, cx, cy, s) } else { moon(g, cx, cy, s) }
        }
        Kind::Partly => {
            if day { sun(g, cx - 0.14 * s, cy - 0.14 * s, s * 0.8) } else { moon(g, cx - 0.14 * s, cy - 0.14 * s, s * 0.8) }
            cloud(g, cx + 0.06 * s, cy + 0.12 * s, s * 0.85, hex("#CBD5E1"));
        }
        Kind::Cloudy => cloud(g, cx, cy, s, grey),
        Kind::Fog => {
            g.fill_style(grey);
            for i in 0..3 {
                let w = s * (0.8 - i as f32 * 0.14);
                g.fill_round_rect(cx - w / 2.0, cy - 0.22 * s + i as f32 * 0.22 * s, w, 0.09 * s, 0.045 * s);
            }
        }
        Kind::Rain => {
            cloud(g, cx, cy - 0.12 * s, s, grey);
            g.stroke_style(hex("#38BDF8"));
            g.line_width((0.06 * s).max(1.2));
            g.line_cap_round();
            for dx in [-0.22f32, 0.0, 0.22] {
                g.begin_path();
                g.move_to(cx + dx * s + 0.04 * s, cy + 0.18 * s);
                g.line_to(cx + dx * s - 0.02 * s, cy + 0.34 * s);
                g.stroke();
            }
        }
        Kind::Storm => {
            cloud(g, cx, cy - 0.12 * s, s, hex("#64748B"));
            g.fill_style(hex("#FBBF24"));
            g.begin_path();
            g.move_to(cx + 0.04 * s, cy + 0.12 * s);
            g.line_to(cx - 0.1 * s, cy + 0.32 * s);
            g.line_to(cx, cy + 0.32 * s);
            g.line_to(cx - 0.06 * s, cy + 0.48 * s);
            g.line_to(cx + 0.14 * s, cy + 0.24 * s);
            g.line_to(cx + 0.03 * s, cy + 0.24 * s);
            g.close_path();
            g.fill();
        }
        Kind::Snow => {
            cloud(g, cx, cy - 0.12 * s, s, grey);
            g.fill_style(hex("#E2E8F0"));
            for dx in [-0.2f32, 0.0, 0.2] {
                g.begin_path();
                g.circle(cx + dx * s, cy + 0.28 * s, 0.045 * s);
                g.fill();
            }
        }
    }
}

fn updown(v: f32) -> Color {
    hex(if v >= 0.0 { GREEN } else { RED })
}

impl App {
    // ── Weather ───────────────────────────────────────────────────────────────

    pub fn draw_weather(&mut self, g: &mut Gfx, v: Rect) {
        let place = weather::report().map(|r| r.place).unwrap_or_else(|| "memuat…".to_string());
        self.dash_page_title(g, v, "Cuaca", &place);
        let Some(r) = weather::report() else {
            ui::card(g, v, Wash::None, false);
            text::draw(g, "Memuat cuaca…", v.cx(), v.cy() - 8.0, Face::Medium, 14.0, hex(pal::INK), Align::Center);
            text::draw(g, &format!("Kota: {}  (`weatherCity` di settings.json: 'auto' atau nama kota)", self.st.settings.weather_city), v.cx(), v.cy() + 14.0, Face::Regular, 11.0, hex(pal::DIM), Align::Center);
            return;
        };
        let left = Rect::new(v.x, v.y, 200.0, v.h);
        let right = Rect::new(v.x + 210.0, v.y, v.w - 210.0, v.h);
        ui::card(g, left, Wash::None, false);
        ui::card(g, right, Wash::None, false);

        // Now.
        text::draw(g, &r.place, left.x + 18.0, left.y + 24.0, Face::Medium, 12.5, hex(pal::DIM), Align::Left);
        if r.estimated {
            let pw = text::measure(&r.place, Face::Medium, 12.5);
            text::draw(g, "≈ dari IP", left.x + 18.0 + pw + 8.0, left.y + 24.5, Face::Regular, 9.5, hex(pal::DIM3), Align::Left);
        }
        weather_icon(g, weather::kind_of(r.now.code), r.now.is_day, left.x + 50.0, left.y + 66.0, 56.0);
        text::draw(g, &format!("{:.0}°", r.now.temp), left.x + 92.0, left.y + 58.0, Face::Bold, 34.0, hex(pal::INK), Align::Left);
        crate::app::fading_text(g, weather::label_of(r.now.code), left.x + 92.0, left.y + 86.0, 11.5, hex(pal::INK), left.x + 92.0, left.x + left.w - 10.0);
        let details = [
            format!("Terasa {:.0}°", r.now.feels),
            format!("Lembap {}%", r.now.humidity),
            format!("Angin {:.0} km/j", r.now.wind),
        ];
        for (i, d) in details.iter().enumerate() {
            text::draw(g, d, left.x + 18.0, left.y + 118.0 + i as f32 * 18.0, Face::Regular, 11.0, hex(pal::DIM), Align::Left);
        }

        // Week.
        let cols = ((((right.w - 24.0) / 46.0).floor()) as usize).clamp(3, 7);
        text::draw(g, &format!("{} hari ke depan", cols.min(r.days.len())), right.x + 18.0, right.y + 24.0, Face::Medium, 12.5, hex(pal::DIM), Align::Left);
        let n = r.days.len().min(cols).max(1) as f32;
        let colw = (right.w - 24.0) / n;
        for (i, d) in r.days.iter().take(cols).enumerate() {
            let cx = right.x + 12.0 + colw * (i as f32 + 0.5);
            let today = i == 0;
            if today {
                g.fill_style(rgba(255, 255, 255, 0.06));
                g.fill_round_rect(cx - colw / 2.0 + 2.0, right.y + 34.0, colw - 4.0, right.h - 46.0, 14.0);
            }
            text::draw(g, if today { "Hari ini" } else { weather::WEEKDAYS[d.weekday % 7] }, cx, right.y + 52.0, Face::Medium, 11.0, hex(pal::INK), Align::Center);
            weather_icon(g, weather::kind_of(d.code), true, cx, right.y + 82.0, 30.0);
            text::draw(g, &format!("{:.0}°", d.max), cx, right.y + 112.0, Face::Bold, 12.5, hex(pal::INK), Align::Center);
            text::draw(g, &format!("{:.0}°", d.min), cx, right.y + 128.0, Face::Regular, 11.5, hex(pal::DIM), Align::Center);
            text::draw(g, &format!("{}%", d.rain), cx, right.y + 148.0, Face::Regular, 10.0, hex("#38BDF8"), Align::Center);
        }
    }

    // ── Stocks ────────────────────────────────────────────────────────────────

    pub fn draw_stocks(&mut self, g: &mut Gfx, v: Rect) {
        let n = self.st.settings.stocks.len();
        self.dash_page_title(g, v, "Saham", &format!("{n} di watchlist · IHSG"));
        stocks::touch();
        let quotes = stocks::quotes();
        let left = Rect::new(v.x, v.y, 250.0, v.h);
        let right = Rect::new(v.x + 260.0, v.y, v.w - 260.0, v.h);
        ui::card(g, left, Wash::None, false);
        ui::card(g, right, Wash::None, false);

        let order = self.st.settings.stocks.clone();
        if self.stock_sel >= order.len() {
            self.stock_sel = 0;
        }

        // Left: the selected ticker and its chart.
        let sel = order.get(self.stock_sel).and_then(|s| quotes.iter().find(|q| &q.symbol == s));
        match sel {
            Some(q) if q.error.is_none() || q.price > 0.0 => {
                text::draw(g, &stocks::display(&q.symbol), left.x + 18.0, left.y + 24.0, Face::Bold, 15.0, hex(pal::INK), Align::Left);
                if !q.currency.is_empty() {
                    text::draw(g, &q.currency, left.x + left.w - 18.0, left.y + 24.0, Face::Regular, 10.5, hex(pal::DIM3), Align::Right);
                }
                text::draw(g, &stocks::fmt_price(q.price), left.x + 18.0, left.y + 56.0, Face::Bold, 26.0, hex(pal::INK), Align::Left);
                let col = updown(q.change());
                text::draw(g, &format!("{}{}  ({}{:.2}%)", if q.change() >= 0.0 { "+" } else { "" }, stocks::fmt_price(q.change()), if q.pct() >= 0.0 { "+" } else { "" }, q.pct()), left.x + 18.0, left.y + 82.0, Face::Medium, 12.0, col, Align::Left);
                let chart = Rect::new(left.x + 16.0, left.y + 100.0, left.w - 32.0, left.h - 100.0 - 16.0);
                chart_area(g, chart, &q.series, q.prev, col);
                if let Some(e) = &q.error {
                    text::draw(g, &format!("data lama ({e})"), left.x + left.w - 18.0, left.y + 82.0, Face::Regular, 10.0, hex(pal::AMBER), Align::Right);
                }
            }
            Some(q) => {
                text::draw(g, &stocks::display(&q.symbol), left.x + 18.0, left.y + 24.0, Face::Bold, 15.0, hex(pal::INK), Align::Left);
                text::draw(g, "Tidak bisa memuat data", left.cx(), left.cy(), Face::Medium, 13.0, hex(pal::DIM), Align::Center);
                text::draw(g, q.error.as_deref().unwrap_or(""), left.cx(), left.cy() + 20.0, Face::Regular, 10.5, hex(pal::DIM3), Align::Center);
            }
            None => {
                text::draw(g, if order.is_empty() { "Watchlist kosong. Tambah ticker di kanan." } else { "Memuat harga…" }, left.cx(), left.cy(), Face::Medium, 13.0, hex(pal::DIM), Align::Center);
            }
        }

        // Right: watchlist rows and the add field.
        let rows_max = (((right.h - 12.0 - 44.0) / 30.0).floor() as usize).clamp(2, 6);
        let mut y = right.y + 12.0;
        let mut act: Option<(usize, bool)> = None; // (index, remove)
        for (i, sym) in order.iter().enumerate().take(rows_max) {
            let row = Rect::new(right.x + 8.0, y, right.w - 16.0, 28.0);
            let (clicked, hover, _) = self.ui.click_region(id_of(&format!("stk-row{i}"), 98), row);
            if i == self.stock_sel || hover {
                g.fill_style(rgba(255, 255, 255, if i == self.stock_sel { 0.09 } else { 0.05 }));
                g.fill_round_rect(row.x, row.y, row.w, row.h, 10.0);
            }
            text::draw(g, &stocks::display(sym), row.x + 10.0, row.cy(), Face::Medium, 12.0, hex(pal::INK), Align::Left);
            let q = quotes.iter().find(|q| &q.symbol == sym);
            if let Some(q) = q.filter(|q| q.price > 0.0) {
                let col = updown(q.change());
                spark(g, Rect::new(row.x + 62.0, row.y + 6.0, 54.0, 16.0), &q.series, col);
                text::draw(g, &stocks::fmt_price(q.price), row.x + row.w - 66.0, row.cy(), Face::Regular, 11.5, hex(pal::INK), Align::Right);
                if hover {
                    let x_r = Rect::new(row.x + row.w - 24.0, row.y + 3.0, 22.0, 22.0);
                    let (xc, xh, _) = self.ui.click_region(id_of(&format!("stk-del{i}"), 99), x_r);
                    text::draw(g, "×", x_r.cx(), x_r.cy(), Face::Bold, 14.0, if xh { hex(RED) } else { hex(pal::DIM) }, Align::Center);
                    if xc {
                        act = Some((i, true));
                    }
                } else {
                    text::draw(g, &format!("{}{:.2}%", if q.pct() >= 0.0 { "+" } else { "" }, q.pct()), row.x + row.w - 8.0, row.cy(), Face::Medium, 11.0, col, Align::Right);
                }
            } else {
                text::draw(g, "…", row.x + row.w - 10.0, row.cy(), Face::Regular, 11.5, hex(pal::DIM3), Align::Right);
            }
            if clicked && act.is_none() {
                act = Some((i, false));
            }
            y += 30.0;
        }
        if order.len() > rows_max {
            text::draw(g, &format!("+{} lagi", order.len() - rows_max), right.x + right.w - 14.0, y + 2.0, Face::Regular, 10.0, hex(pal::DIM3), Align::Right);
        }

        // Add field.
        let fb = Rect::new(right.x + 12.0, right.y + right.h - 38.0, right.w - 24.0, 26.0);
        g.fill_style(rgba(255, 255, 255, if self.stock_input.focused { 0.11 } else { 0.06 }));
        g.fill_round_rect(fb.x, fb.y, fb.w, fb.h, 13.0);
        let field = Rect::new(fb.x + 12.0, fb.y + 4.0, fb.w - 24.0, 18.0);
        let (mx, my) = self.ui.input.mouse;
        let over = field.contains(mx, my);
        if self.ui.input.pressed {
            if over {
                self.stock_input.focused = true;
                self.platform.set_activating(true);
                self.stock_input.click_at(mx, 11.5, crate::platform::shift_down());
            } else if self.stock_input.focused {
                self.stock_input.focused = false;
                self.platform.set_activating(false);
            }
        }
        if over {
            self.ui.hovering_text = true;
        }
        self.stock_input.draw(g, field, 11.5, pal::INK);

        if let Some((i, remove)) = act {
            crate::sound::play("blip");
            if remove {
                self.st.settings.stocks.remove(i);
                self.save_settings();
                stocks::set_symbols(&self.st.settings.stocks);
            } else {
                self.stock_sel = i;
            }
        }
    }

    /// Enter in the add-ticker field.
    pub fn add_stock(&mut self) {
        let raw = self.stock_input.text();
        let Some(sym) = stocks::normalize(&raw) else {
            self.stock_input.clear();
            return;
        };
        if !self.st.settings.stocks.contains(&sym) && self.st.settings.stocks.len() < 12 {
            self.st.settings.stocks.push(sym);
            self.stock_sel = self.st.settings.stocks.len() - 1;
            self.save_settings();
            stocks::set_symbols(&self.st.settings.stocks);
        }
        self.stock_input.clear();
        crate::sound::play("send");
    }

    // ── Teleprompter ──────────────────────────────────────────────────────────

    fn tp_path() -> std::path::PathBuf {
        crate::settings::config_dir().join("teleprompter.txt")
    }

    /// Reads the script; the first time, a short placeholder explains where to write it.
    pub fn tp_reload(&mut self, width: f32) {
        let path = Self::tp_path();
        if !path.exists() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, "Tulis naskahmu di sini.\n\nTekan Edit di island untuk membuka file ini, simpan, lalu naskah di layar ikut berubah.\n\nTekan putar, dan teks bergulir pelan di bawah kamera sementara kamu tetap menatap lensa.");
        }
        self.tp_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        self.tp_text = std::fs::read_to_string(&path).unwrap_or_default().replace('\r', "");
        self.tp_lines = text::wrap(&self.tp_text, Face::Medium, 21.0, width);
        self.tp_width = width;
        self.tp_scroll = self.tp_scroll.min(self.tp_total());
    }

    fn tp_total(&self) -> f32 {
        self.tp_lines.len() as f32 * TP_LINE
    }

    pub fn draw_teleprompter(&mut self, g: &mut Gfx, v: Rect) {
        self.dash_page_title(g, v, "Teleprompter", if self.tp_playing { "sedang berjalan" } else { "naskah di bawah kamera" });
        ui::card(g, v, Wash::None, false);
        let text_w = v.w - 72.0;
        // Reload when the file changed (checked about once a second, see dash_t) or the width did.
        let stale = std::fs::metadata(Self::tp_path()).and_then(|m| m.modified()).ok() != self.tp_mtime;
        if self.tp_lines.is_empty() || stale || (self.tp_width - text_w).abs() > 1.0 {
            self.tp_reload(text_w);
        }

        let top = v.y + 14.0;
        let bottom = v.y + v.h - 52.0;
        let band = top + (bottom - top) * 0.38; // the line to read sits a little above the middle
        let x = v.cx();
        for (i, line) in self.tp_lines.iter().enumerate() {
            let y = band + i as f32 * TP_LINE - self.tp_scroll;
            if y < top - TP_LINE || y > bottom + TP_LINE {
                continue;
            }
            let fade = clamp((y - top) / 26.0, 0.0, 1.0) * clamp((bottom - y) / 26.0, 0.0, 1.0);
            let focus = 1.0 - clamp((y - band).abs() / 90.0, 0.0, 1.0);
            let a = fade * (0.35 + 0.65 * focus);
            if a > 0.02 {
                g.save();
                g.mul_alpha(a);
                text::draw(g, line, x, y, Face::Medium, 21.0, hex(pal::INK), Align::Center);
                g.restore();
            }
        }

        // Controls.
        let cy = v.y + v.h - 26.0;
        let chip = |ui: &mut ui::Ui, g: &mut Gfx, id: &str, r: Rect, label: &str| -> bool {
            let (clicked, hover, _) = ui.click_region(id_of(id, 100), r);
            g.fill_style(rgba(255, 255, 255, if hover { 0.14 } else { 0.07 }));
            g.fill_round_rect(r.x, r.y, r.w, r.h, 11.0);
            text::draw(g, label, r.cx(), r.cy(), Face::Medium, 11.0, hex(pal::INK), Align::Center);
            clicked
        };
        let mid = v.cx();
        // Restart.
        if chip(&mut self.ui, g, "tp-restart", Rect::new(mid - 96.0, cy - 12.0, 44.0, 24.0), "↺") {
            self.tp_scroll = 0.0;
            self.tp_playing = false;
        }
        // Play / pause.
        let pr = Rect::new(mid - 22.0, cy - 15.0, 44.0, 30.0);
        let (pc, ph, _) = self.ui.click_region(id_of("tp-play", 100), pr);
        g.fill_style(rgba(255, 255, 255, if ph { 0.2 } else { 0.12 }));
        g.fill_round_rect(pr.x, pr.y, pr.w, pr.h, 15.0);
        g.fill_style(hex(pal::INK));
        if self.tp_playing { glyph_pause(g, pr.cx(), pr.cy(), 13.0) } else { glyph_play(g, pr.cx(), pr.cy(), 13.0) }
        if pc {
            self.tp_playing = !self.tp_playing;
            if self.tp_playing && self.tp_scroll >= self.tp_total() {
                self.tp_scroll = 0.0;
            }
        }
        // Speed.
        if chip(&mut self.ui, g, "tp-slower", Rect::new(mid + 52.0, cy - 12.0, 26.0, 24.0), "−") {
            self.tp_speed = (self.tp_speed - 6.0).max(6.0);
        }
        text::draw(g, &format!("{:.0}", self.tp_speed), mid + 96.0, cy, Face::Bold, 12.0, hex(pal::INK), Align::Center);
        if chip(&mut self.ui, g, "tp-faster", Rect::new(mid + 114.0, cy - 12.0, 26.0, 24.0), "+") {
            self.tp_speed = (self.tp_speed + 6.0).min(150.0);
        }
        // Edit.
        if chip(&mut self.ui, g, "tp-edit", Rect::new(v.x + v.w - 76.0, cy - 12.0, 60.0, 24.0), "Edit") {
            let _ = std::process::Command::new("notepad.exe").arg(Self::tp_path()).spawn();
        }
        text::draw(g, "kecepatan", mid + 96.0, cy + 14.0, Face::Regular, 9.0, hex(pal::DIM3), Align::Center);
        let _ = (activity::timer, fading_text);
    }

    /// Moves the script while it plays; called from the frame loop.
    pub fn tp_step(&mut self, dt: f32) {
        if !self.tp_playing {
            return;
        }
        self.tp_scroll += self.tp_speed * dt;
        let end = self.tp_total();
        if self.tp_scroll >= end {
            self.tp_scroll = end;
            self.tp_playing = false;
        }
    }
}

const TP_LINE: f32 = 30.0;

// ── Charts ────────────────────────────────────────────────────────────────────

fn range_of(series: &[f32], prev: f32) -> (f32, f32) {
    let (mut lo, mut hi) = (prev, prev);
    for &p in series {
        lo = lo.min(p);
        hi = hi.max(p);
    }
    if (hi - lo).abs() < 1e-6 {
        hi = lo + 1.0;
    }
    let pad = (hi - lo) * 0.08;
    (lo - pad, hi + pad)
}

fn plot(g: &mut Gfx, r: Rect, series: &[f32], lo: f32, hi: f32) -> Vec<(f32, f32)> {
    let n = series.len().max(2) as f32 - 1.0;
    series
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let _ = &g;
            (r.x + r.w * i as f32 / n, r.y + r.h * (1.0 - (p - lo) / (hi - lo)))
        })
        .collect()
}

/// Intraday line with a soft fill and a faint line at the previous close.
fn chart_area(g: &mut Gfx, r: Rect, series: &[f32], prev: f32, col: Color) {
    if series.len() < 2 {
        text::draw(g, "Belum ada data intraday", r.cx(), r.cy(), Face::Regular, 11.0, hex(pal::DIM3), Align::Center);
        return;
    }
    let (lo, hi) = range_of(series, prev);
    let pts = plot(g, r, series, lo, hi);
    let yp = r.y + r.h * (1.0 - (prev - lo) / (hi - lo));
    g.fill_style(rgba(255, 255, 255, 0.10));
    g.fill_rect(r.x, yp, r.w, 1.0);

    let fill = g.linear(0.0, r.y, 0.0, r.y + r.h, &[(0.0, crate::gfx::with_alpha(col, 0.28)), (1.0, crate::gfx::with_alpha(col, 0.0))]);
    g.fill_style(fill);
    g.begin_path();
    g.move_to(pts[0].0, r.y + r.h);
    for p in &pts {
        g.line_to(p.0, p.1);
    }
    g.line_to(pts[pts.len() - 1].0, r.y + r.h);
    g.close_path();
    g.fill();

    g.stroke_style(col);
    g.line_width(1.8);
    g.line_join_round();
    g.line_cap_round();
    g.begin_path();
    g.move_to(pts[0].0, pts[0].1);
    for p in pts.iter().skip(1) {
        g.line_to(p.0, p.1);
    }
    g.stroke();
    let last = pts[pts.len() - 1];
    g.fill_style(col);
    g.begin_path();
    g.circle(last.0, last.1, 3.0);
    g.fill();
}

fn spark(g: &mut Gfx, r: Rect, series: &[f32], col: Color) {
    if series.len() < 2 {
        return;
    }
    let (lo, hi) = range_of(series, series[0]);
    let pts = plot(g, r, series, lo, hi);
    g.stroke_style(col);
    g.line_width(1.3);
    g.line_join_round();
    g.begin_path();
    g.move_to(pts[0].0, pts[0].1);
    for p in pts.iter().skip(1) {
        g.line_to(p.0, p.1);
    }
    g.stroke();
}
