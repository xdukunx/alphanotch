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
        let right_w = 190.0;
        let left = Rect::new(v.x, v.y, v.w - right_w - 8.0, v.h);
        let right = Rect::new(left.x + left.w + 8.0, v.y, right_w, v.h);
        ui::card(g, left, Wash::None, false);
        ui::card(g, right, Wash::None, false);
        self.chart_rect = Rect::default();

        let order = self.st.settings.stocks.clone();
        if self.stock_sel >= order.len() {
            self.stock_sel = 0;
        }

        // Left: the selected ticker, with its chart.
        let sel = order.get(self.stock_sel).and_then(|s| quotes.iter().find(|q| &q.symbol == s)).cloned();
        match sel {
            Some(q) if q.error.is_none() || q.price > 0.0 => self.draw_stock_detail(g, left, &q),
            Some(q) => {
                text::draw(g, &stocks::display(&q.symbol), left.x + 16.0, left.y + 22.0, Face::Bold, 15.0, hex(pal::INK), Align::Left);
                text::draw(g, "Tidak bisa memuat data", left.cx(), left.cy(), Face::Medium, 13.0, hex(pal::DIM), Align::Center);
                text::draw(g, q.error.as_deref().unwrap_or(""), left.cx(), left.cy() + 20.0, Face::Regular, 10.5, hex(pal::DIM3), Align::Center);
            }
            None => {
                text::draw(g, if order.is_empty() { "Watchlist kosong. Tambah ticker di kanan." } else { "Memuat harga…" }, left.cx(), left.cy(), Face::Medium, 13.0, hex(pal::DIM), Align::Center);
            }
        }

        // Right: the watchlist (two lines a row) and the add field.
        let pitch = 38.0;
        let rows_max = (((right.h - 10.0 - 42.0) / pitch).floor() as usize).clamp(2, 6);
        let max_off = order.len().saturating_sub(rows_max);
        let list = Rect::new(right.x, right.y + 8.0, right.w, rows_max as f32 * pitch);
        if max_off > 0 && self.ui.input.wheel != 0.0 && list.contains(self.ui.input.mouse.0, self.ui.input.mouse.1) {
            if self.ui.input.wheel > 0.0 {
                self.stock_scroll = self.stock_scroll.saturating_sub(1);
            } else {
                self.stock_scroll += 1;
            }
        }
        self.stock_scroll = self.stock_scroll.min(max_off);
        let off = self.stock_scroll;
        let mut y = right.y + 8.0;
        let mut act: Option<(usize, bool)> = None; // (index, remove)
        for (i, sym) in order.iter().enumerate().skip(off).take(rows_max) {
            let row = Rect::new(right.x + 6.0, y, right.w - 12.0, pitch - 4.0);
            let (clicked, hover, _) = self.ui.click_region(id_of(&format!("stk-row{i}"), 98), row);
            let selected = i == self.stock_sel;
            if selected || hover {
                g.fill_style(rgba(255, 255, 255, if selected { 0.09 } else { 0.05 }));
                g.fill_round_rect(row.x, row.y, row.w, row.h, 10.0);
            }
            let q = quotes.iter().find(|q| &q.symbol == sym);
            let col = q.map(|q| updown(q.change())).unwrap_or(hex(pal::DIM3));
            if selected {
                g.fill_style(col);
                g.fill_round_rect(row.x + 2.0, row.y + 7.0, 2.5, row.h - 14.0, 1.25);
            }
            let (l1, l2) = (row.y + 12.0, row.y + 27.0);
            text::draw(g, &stocks::display(sym), row.x + 12.0, l1, Face::Medium, 12.0, hex(pal::INK), Align::Left);
            if let Some(q) = q.filter(|q| q.price > 0.0) {
                text::draw(g, &stocks::fmt_price(q.price), row.x + 12.0, l2, Face::Regular, 10.5, hex(pal::DIM), Align::Left);
                spark(g, Rect::new(row.x + row.w - 62.0, row.y + 20.0, 52.0, 13.0), &q.series, col);
                if hover {
                    let x_r = Rect::new(row.x + row.w - 24.0, row.y + 2.0, 22.0, 20.0);
                    let (xc, xh, _) = self.ui.click_region(id_of(&format!("stk-del{i}"), 99), x_r);
                    text::draw(g, "×", x_r.cx(), x_r.cy(), Face::Bold, 14.0, if xh { hex(RED) } else { hex(pal::DIM) }, Align::Center);
                    if xc {
                        act = Some((i, true));
                    }
                } else {
                    text::draw(g, &format!("{}{:.2}%", if q.pct() >= 0.0 { "+" } else { "" }, q.pct()), row.x + row.w - 8.0, l1, Face::Medium, 11.0, col, Align::Right);
                }
            } else {
                text::draw(g, "…", row.x + row.w - 10.0, l1, Face::Regular, 11.5, hex(pal::DIM3), Align::Right);
            }
            if clicked && act.is_none() {
                act = Some((i, false));
            }
            y += pitch;
        }
        if max_off > 0 {
            let track_h = rows_max as f32 * pitch - 6.0;
            let thumb_h = (track_h * rows_max as f32 / order.len() as f32).max(14.0);
            let thumb_y = list.y + 2.0 + (track_h - thumb_h) * off as f32 / max_off as f32;
            g.fill_style(rgba(255, 255, 255, 0.28));
            g.fill_round_rect(right.x + right.w - 5.0, thumb_y, 2.5, thumb_h, 1.25);
        }

        // Add field.
        let fb = Rect::new(right.x + 10.0, right.y + right.h - 34.0, right.w - 20.0, 24.0);
        g.fill_style(rgba(255, 255, 255, if self.stock_input.focused { 0.11 } else { 0.06 }));
        g.fill_round_rect(fb.x, fb.y, fb.w, fb.h, 12.0);
        let field = Rect::new(fb.x + 12.0, fb.y + 3.0, fb.w - 24.0, 18.0);
        let (mx, my) = self.ui.input.mouse;
        let over = field.contains(mx, my);
        self.ui.note_region(id_of("stock-field", 111), field);
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

    /// The selected stock: name, price and change, the range tabs, the candle / line switch and
    /// the chart with its axes, volume, last-price tag and the crosshair under the pointer.
    fn draw_stock_detail(&mut self, g: &mut Gfx, r: Rect, q: &stocks::Quote) {
        let col = updown(q.change());
        let x = r.x + 16.0;

        text::draw(g, &stocks::display(&q.symbol), x, r.y + 18.0, Face::Bold, 14.0, hex(pal::INK), Align::Left);
        if !q.currency.is_empty() {
            let w = text::measure(&stocks::display(&q.symbol), Face::Bold, 14.0);
            text::draw(g, &q.currency, x + w + 8.0, r.y + 19.0, Face::Regular, 9.5, hex(pal::DIM3), Align::Left);
        }
        let price = stocks::fmt_price(q.price);
        let pw = text::measure(&price, Face::Bold, 22.0);
        text::draw(g, &price, x, r.y + 44.0, Face::Bold, 22.0, hex(pal::INK), Align::Left);
        let sign = if q.change() >= 0.0 { "+" } else { "" };
        text::draw(g, &format!("{sign}{}  {sign}{:.2}%", stocks::fmt_price(q.change()), q.pct()), x + pw + 10.0, r.y + 45.0, Face::Medium, 11.5, col, Align::Left);
        // Top right: how old the price is. Every free feed for the Jakarta exchange is ten
        // minutes behind, and the header says so instead of letting it pass for live.
        let now_s = crate::util::unix_now() as i64;
        let fresh = stocks::freshness(q.market_time, now_s, q.offset);
        if q.error.is_some() {
            text::draw(g, "data lama", r.x + r.w - 16.0, r.y + 18.0, Face::Regular, 9.5, hex(pal::AMBER), Align::Right);
        } else if let Some(f) = fresh {
            text::draw(g, &f, r.x + r.w - 16.0, r.y + 18.0, Face::Regular, 9.5, hex(pal::DIM3), Align::Right);
        }
        // The day as the exchange reports it: high, low, volume.
        if q.day_high > 0.0 && q.day_low > 0.0 {
            let line = format!("H {}   L {}   Vol {}", stocks::fmt_price(q.day_high), stocks::fmt_price(q.day_low), stocks::fmt_volume(q.volume));
            text::draw(g, &line, r.x + r.w - 16.0, r.y + 31.0, Face::Regular, 10.0, hex(pal::DIM), Align::Right);
        }

        // Range tabs on the left, candle / line on the right.
        let ty = r.y + 62.0;
        let mut tx = x - 6.0;
        for (i, (label, _, _)) in stocks::RANGES.iter().enumerate() {
            let w = text::measure(label, Face::Medium, 10.5) + 16.0;
            let tab = Rect::new(tx, ty, w, 18.0);
            let (clicked, hover, _) = self.ui.click_region(id_of(&format!("range-{i}"), 121), tab);
            let on = self.stock_range == i;
            if on {
                g.fill_style(rgba(45, 91, 255, 0.34));
                g.fill_round_rect(tab.x, tab.y, tab.w, tab.h, 9.0);
            } else if hover {
                g.fill_style(rgba(255, 255, 255, 0.07));
                g.fill_round_rect(tab.x, tab.y, tab.w, tab.h, 9.0);
            }
            text::draw(g, label, tab.cx(), tab.cy(), Face::Medium, 10.5, hex(if on { pal::INK } else { pal::DIM2 }), Align::Center);
            if clicked && !on {
                self.stock_range = i;
                crate::sound::play("blip");
            }
            tx += w + 2.0;
        }
        for (k, candles) in [(0usize, true), (1usize, false)] {
            let b = Rect::new(r.x + r.w - 16.0 - 52.0 + k as f32 * 26.0, ty - 1.0, 24.0, 20.0);
            let (clicked, hover, _) = self.ui.click_region(id_of(&format!("ctype-{k}"), 122), b);
            let on = self.st.settings.stocks_candles == candles;
            g.fill_style(rgba(255, 255, 255, if on { 0.16 } else if hover { 0.09 } else { 0.04 }));
            g.fill_round_rect(b.x, b.y, b.w, b.h, 7.0);
            let ink = if on { hex(pal::INK) } else { hex(pal::DIM2) };
            if candles {
                for (dx, up) in [(-4.5f32, true), (0.0, false), (4.5, true)] {
                    let top = if up { -4.0 } else { -2.0 };
                    g.fill_style(ink);
                    g.fill_rect(b.cx() + dx - 0.5, b.cy() - 6.0, 1.0, 12.0);
                    g.fill_rect(b.cx() + dx - 1.8, b.cy() + top, 3.6, 6.0);
                }
            } else {
                g.stroke_style(ink);
                g.line_width(1.6);
                g.line_cap_round();
                g.line_join_round();
                g.begin_path();
                g.move_to(b.x + 5.0, b.cy() + 3.5);
                g.line_to(b.x + 10.0, b.cy() - 1.0);
                g.line_to(b.x + 14.0, b.cy() + 1.5);
                g.line_to(b.x + 19.0, b.cy() - 4.0);
                g.stroke();
            }
            if clicked && !on {
                self.st.settings.stocks_candles = candles;
                self.save_settings();
            }
        }

        // The data: today's candles come with the quote, the other ranges are fetched.
        let detail = if self.stock_range == 0 {
            stocks::Detail { candles: q.candles.clone(), offset: q.offset, prev: q.prev, loading: false, error: None }
        } else {
            stocks::detail(&q.symbol, self.stock_range)
        };
        let area = Rect::new(r.x + 8.0, ty + 26.0, r.w - 16.0, (r.y + r.h - 10.0 - (ty + 26.0)).max(0.0));
        if detail.candles.len() < 2 {
            let msg = if detail.loading { "Memuat grafik…" } else if let Some(e) = &detail.error { e.as_str() } else { "Belum ada data" };
            text::draw(g, msg, area.cx(), area.cy(), Face::Regular, 11.0, hex(pal::DIM3), Align::Center);
            return;
        }
        let mouse = self.ui.input.mouse;
        self.chart_rect = area;
        let mouse_in = area.contains(mouse.0, mouse.1).then_some(mouse);
        draw_candles(g, area, &detail, if self.stock_range == 0 { Some(detail.prev).filter(|p| *p > 0.0) } else { None }, self.st.settings.stocks_candles, col, self.stock_range, mouse_in);
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
            self.stock_scroll = usize::MAX; // clamped on the next frame: shows the new row
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

/// "11:15" / "Sel 14:30" / "21 Okt" for a bar's start, in the exchange's time.
fn bar_time(t: i64, offset: i32, range: usize) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];
    const DAYS: [&str; 7] = ["Kam", "Jum", "Sab", "Min", "Sen", "Sel", "Rab"]; // 1970-01-01 was a Thursday
    let local = t + offset as i64;
    let days = local.div_euclid(86_400);
    let secs = local.rem_euclid(86_400);
    let (_, m, d) = crate::coach::civil_from_days(days);
    let clock = format!("{:02}:{:02}", secs / 3600, (secs % 3600) / 60);
    let month = MONTHS[(m as usize).clamp(1, 12) - 1];
    match range {
        0 => clock,
        1 => format!("{} {clock}", DAYS[days.rem_euclid(7) as usize]),
        2 => format!("{d} {month} {clock}"),
        _ => format!("{d} {month}"),
    }
}

/// `f32::clamp` panics when min > max, which a small box does while the island is still growing.
fn safe_clamp(x: f32, lo: f32, hi: f32) -> f32 {
    x.max(lo).min(hi.max(lo))
}

/// Where each of `n` slots is, left to right, in `plot` (a slot is one bar wide).
fn slot_x(plot: Rect, n: usize, i: usize) -> f32 {
    plot.x + plot.w * (i as f32 + 0.5) / n.max(1) as f32
}

/// A TradingView-like chart: grid and price axis on the right, candles (or a line with a soft
/// fill), the volume under them, the last price as a tag on the axis, the previous close as a
/// dashed line, and a crosshair with a tooltip when the pointer is over it.
fn draw_candles(g: &mut Gfx, area: Rect, d: &stocks::Detail, prev: Option<f32>, candles: bool, up_col: Color, range: usize, mouse: Option<(f32, f32)>) {
    let cs = &d.candles;
    let n = cs.len();
    // Mid-animation the box can be a sliver: nothing sensible fits in it yet.
    if n < 2 || area.h < 70.0 || area.w < 120.0 {
        return;
    }
    // Today: the slots of the whole session (about 84 five-minute bars), so a morning chart
    // fills from the left and leaves the afternoon empty, as on TradingView.
    let slots = if range == 0 { n.max(84) } else { n };
    let axis_w = 44.0;
    let time_h = 13.0;
    let vol_h = (area.h * 0.17).clamp(14.0, 26.0);
    let plot = Rect::new(area.x, area.y, area.w - axis_w, area.h - time_h - vol_h - 3.0);
    let vol = Rect::new(plot.x, plot.y + plot.h + 3.0, plot.w, vol_h);

    // Price range: the lows and highs (or just the closes for a line), the previous close, a margin.
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for c in cs {
        let (a, b) = if candles { (c.l, c.h) } else { (c.c, c.c) };
        lo = lo.min(a);
        hi = hi.max(b);
    }
    if let Some(p) = prev {
        lo = lo.min(p);
        hi = hi.max(p);
    }
    if (hi - lo).abs() < 1e-6 {
        hi = lo + 1.0;
    }
    let pad = (hi - lo) * 0.07;
    let (lo, hi) = (lo - pad, hi + pad);
    let ypx = |p: f32| plot.y + plot.h * (1.0 - (p - lo) / (hi - lo));

    // Grid and the price axis (a label the last-price tag would sit on is left out).
    let tag_y = safe_clamp(ypx(cs[n - 1].c), plot.y + 6.0, plot.y + plot.h - 6.0);
    for i in 0..=4 {
        let y = plot.y + plot.h * i as f32 / 4.0;
        g.fill_style(rgba(255, 255, 255, 0.05));
        g.fill_rect(plot.x, y, plot.w, 1.0);
        let p = hi - (hi - lo) * i as f32 / 4.0;
        let ly = safe_clamp(y, plot.y + 5.0, plot.y + plot.h - 5.0);
        if (ly - tag_y).abs() > 10.0 {
            text::draw(g, &stocks::fmt_price(p), plot.x + plot.w + 6.0, ly, Face::Regular, 9.0, hex(pal::DIM3), Align::Left);
        }
    }
    // The previous close, dashed.
    if let Some(p) = prev {
        let y = ypx(p);
        g.fill_style(rgba(255, 255, 255, 0.22));
        let mut x = plot.x;
        while x < plot.x + plot.w {
            g.fill_rect(x, y, 3.0, 1.0);
            x += 7.0;
        }
    }

    // The bars: candles, or a line over a soft fill.
    let slot = plot.w / slots as f32;
    let (green, red) = (hex(GREEN), hex(RED));
    if candles {
        let bw = (slot * 0.64).clamp(1.0, 9.0);
        for (i, c) in cs.iter().enumerate() {
            let cx = slot_x(plot, slots, i).floor() + 0.5;
            let col = if c.c >= c.o { green } else { red };
            g.fill_style(col);
            g.fill_rect(cx - 0.5, ypx(c.h), 1.0, (ypx(c.l) - ypx(c.h)).max(1.0));
            let (top, bot) = (ypx(c.o.max(c.c)), ypx(c.o.min(c.c)));
            g.fill_rect((cx - bw / 2.0).floor(), top, bw.floor().max(1.0), (bot - top).max(1.0));
        }
    } else {
        let pts: Vec<(f32, f32)> = cs.iter().enumerate().map(|(i, c)| (slot_x(plot, slots, i), ypx(c.c))).collect();
        let fill = g.linear(0.0, plot.y, 0.0, plot.y + plot.h, &[(0.0, crate::gfx::with_alpha(up_col, 0.26)), (1.0, crate::gfx::with_alpha(up_col, 0.0))]);
        g.fill_style(fill);
        g.begin_path();
        g.move_to(pts[0].0, plot.y + plot.h);
        for p in &pts {
            g.line_to(p.0, p.1);
        }
        g.line_to(pts[n - 1].0, plot.y + plot.h);
        g.close_path();
        g.fill();
        g.stroke_style(up_col);
        g.line_width(1.6);
        g.line_join_round();
        g.begin_path();
        g.move_to(pts[0].0, pts[0].1);
        for p in pts.iter().skip(1) {
            g.line_to(p.0, p.1);
        }
        g.stroke();
    }

    // Volume under the price.
    let vmax = cs.iter().map(|c| c.v).fold(0.0f32, f32::max);
    if vmax > 0.0 {
        let bw = (slot * 0.64).clamp(1.0, 9.0);
        for (i, c) in cs.iter().enumerate() {
            let h = (vol.h * (c.v / vmax)).max(if c.v > 0.0 { 1.0 } else { 0.0 });
            let col = if c.c >= c.o { green } else { red };
            g.fill_style(crate::gfx::with_alpha(col, 0.42));
            g.fill_rect((slot_x(plot, slots, i) - bw / 2.0).floor(), vol.y + vol.h - h, bw.floor().max(1.0), h);
        }
    }

    // The last price: a dotted line to the axis and a tag on it, in the colour of the day.
    let last = cs[n - 1].c;
    let ly = safe_clamp(ypx(last), plot.y + 6.0, plot.y + plot.h - 6.0);
    g.fill_style(crate::gfx::with_alpha(up_col, 0.5));
    let mut x = slot_x(plot, slots, n - 1);
    while x < plot.x + plot.w {
        g.fill_rect(x, ly, 2.0, 1.0);
        x += 5.0;
    }
    let tag = stocks::fmt_price(last);
    let tw = text::measure(&tag, Face::Medium, 9.5) + 8.0;
    g.fill_style(up_col);
    g.fill_round_rect(plot.x + plot.w + 2.0, ly - 7.0, tw.max(axis_w - 4.0), 14.0, 4.0);
    text::draw(g, &tag, plot.x + plot.w + 2.0 + tw.max(axis_w - 4.0) / 2.0, ly, Face::Medium, 9.5, hex("#0B0C0E"), Align::Center);

    // The time axis: first, middle, last.
    let t0 = cs[0].t;
    let step = if range == 0 && n > 1 { (cs[n - 1].t - t0) / (n as i64 - 1) } else { 0 };
    for (frac, a) in [(0.0f32, Align::Left), (0.5, Align::Center), (1.0, Align::Right)] {
        let (x, t) = match a {
            Align::Left => (plot.x, t0),
            Align::Center => (plot.x + plot.w * 0.5, if range == 0 { t0 + step * (slots as i64 / 2) } else { cs[n / 2].t }),
            _ => (plot.x + plot.w, if range == 0 { t0 + step * slots as i64 } else { cs[n - 1].t }),
        };
        let _ = frac;
        text::draw(g, &bar_time(t, d.offset, range), x, area.y + area.h - 5.0, Face::Regular, 9.0, hex(pal::DIM3), a);
    }

    // The crosshair and its tooltip.
    if let Some((mx, my)) = mouse {
        if mx >= plot.x && mx <= plot.x + plot.w {
            let i = (((mx - plot.x) / plot.w * slots as f32).floor() as usize).min(slots - 1);
            if i >= n {
                return;
            }
            let c = cs[i];
            let cx = slot_x(plot, slots, i);
            g.fill_style(rgba(255, 255, 255, 0.28));
            g.fill_rect(cx - 0.5, plot.y, 1.0, plot.h + vol.h + 3.0);
            if my >= plot.y && my <= plot.y + plot.h {
                g.fill_rect(plot.x, my - 0.5, plot.w, 1.0);
                let p = lo + (hi - lo) * (1.0 - (my - plot.y) / plot.h);
                let label = stocks::fmt_price(p);
                let w = text::measure(&label, Face::Medium, 9.0) + 8.0;
                g.fill_style(rgba(236, 239, 247, 0.9));
                g.fill_round_rect(plot.x + plot.w + 2.0, my - 7.0, w.max(axis_w - 4.0), 14.0, 4.0);
                text::draw(g, &label, plot.x + plot.w + 2.0 + w.max(axis_w - 4.0) / 2.0, my, Face::Medium, 9.0, hex("#0B0C0E"), Align::Center);
            }
            let col = if c.c >= c.o { green } else { red };
            let chg = if c.o.abs() > 1e-9 { (c.c - c.o) / c.o * 100.0 } else { 0.0 };
            let l1 = bar_time(c.t, d.offset, range);
            let l2 = format!("O {}  H {}  L {}  C {}", stocks::fmt_price(c.o), stocks::fmt_price(c.h), stocks::fmt_price(c.l), stocks::fmt_price(c.c));
            let l3 = format!("{}{chg:.2}%   Vol {}", if chg >= 0.0 { "+" } else { "" }, stocks::fmt_volume(c.v));
            let bw = text::measure(&l2, Face::Regular, 9.5).max(text::measure(&l1, Face::Medium, 10.0)) + 16.0;
            let bx = if cx > plot.x + plot.w / 2.0 { plot.x + 6.0 } else { plot.x + plot.w - bw - 6.0 };
            let by = plot.y + 4.0;
            g.fill_style(rgba(3, 4, 10, 0.88));
            g.fill_round_rect(bx, by, bw, 44.0, 7.0);
            g.stroke_style(rgba(236, 239, 247, 0.12));
            g.line_width(1.0);
            g.round_rect(bx + 0.5, by + 0.5, bw - 1.0, 43.0, 6.5);
            g.stroke();
            text::draw(g, &l1, bx + 8.0, by + 11.0, Face::Medium, 10.0, hex(pal::INK), Align::Left);
            text::draw(g, &l2, bx + 8.0, by + 24.0, Face::Regular, 9.5, hex(pal::DIM), Align::Left);
            text::draw(g, &l3, bx + 8.0, by + 37.0, Face::Medium, 9.5, col, Align::Left);
        }
    }
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

#[cfg(test)]
mod chart_tests {
    use super::*;

    #[test]
    fn a_clamp_with_inverted_bounds_does_not_panic() {
        // The crash: min = 136, max = 134 while the island was still growing.
        assert_eq!(safe_clamp(120.0, 136.0, 134.0), 136.0);
        assert_eq!(safe_clamp(500.0, 136.0, 134.0), 136.0);
        assert_eq!(safe_clamp(10.0, 0.0, 20.0), 10.0);
        assert_eq!(safe_clamp(30.0, 0.0, 20.0), 20.0);
    }

    #[test]
    fn the_chart_survives_every_box_size_the_animation_passes_through() {
        let cs: Vec<stocks::Candle> = (0..90)
            .map(|i| stocks::Candle { t: 1_792_000_000 + i * 300, o: 100.0 + i as f32, h: 103.0 + i as f32, l: 98.0 + i as f32, c: 101.0 + i as f32, v: 1000.0 })
            .collect();
        let d = stocks::Detail { candles: cs, offset: 25_200, prev: 100.0, loading: false, error: None };
        let mut g = Gfx::new(600, 400, 1.0);
        for h in [0.0f32, 5.0, 20.0, 60.0, 69.0, 70.0, 71.0, 90.0, 120.0, 200.0] {
            for w in [0.0f32, 60.0, 119.0, 120.0, 300.0] {
                for candles in [true, false] {
                    draw_candles(&mut g, Rect::new(10.0, 130.0, w, h), &d, Some(100.0), candles, hex(GREEN), 0, Some((50.0, 140.0)));
                }
            }
        }
    }
}
