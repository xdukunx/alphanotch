// Two pages of the dashboard group, in Nusa's way of moving between them: round chevrons
// and page dots at the bottom instead of one more tab. Water counts what you drank and
// charts it by the hour; Focus is a Pomodoro timer with its ring.

use serde::{Deserialize, Serialize};

use crate::activity;
use crate::anim::clamp;
use crate::app::App;
use crate::gfx::{hex, rgba, with_alpha, Gfx};
use crate::icons::Icon;
use crate::layout::{View, Wash};
use crate::text::{self, Align, Face};
use crate::ui::{self, id_of, pal, Rect};
use crate::util;

// ── Water data ────────────────────────────────────────────────────────────────

#[derive(Default, Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Water {
    /// The local day the entries belong to.
    pub day: i64,
    /// (unix seconds, millilitres), oldest first.
    pub entries: Vec<(f64, u32)>,
}

impl Water {
    fn path() -> std::path::PathBuf {
        crate::settings::config_dir().join("water.json")
    }

    pub fn load() -> Water {
        std::fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(crate::settings::config_dir());
        if let Ok(s) = serde_json::to_string(self) {
            let _ = std::fs::write(Self::path(), s);
        }
    }

    /// Starts a new day: yesterday's drinks are forgotten.
    pub fn roll(&mut self, day: i64) -> bool {
        if self.day == day {
            return false;
        }
        self.day = day;
        self.entries.clear();
        true
    }

    pub fn total(&self) -> u32 {
        self.entries.iter().map(|e| e.1).sum()
    }

    pub fn add(&mut self, ml: u32, at: f64) {
        self.entries.push((at, ml));
    }

    pub fn undo(&mut self) {
        self.entries.pop();
    }

    /// Eight three-hour bins, from midnight. `offset` is the local UTC offset in seconds.
    pub fn bins(&self, offset: i64) -> [u32; 8] {
        let mut b = [0u32; 8];
        for (t, ml) in &self.entries {
            let h = (((*t as i64 + offset).rem_euclid(86_400)) / 3600) as usize;
            b[(h / 3).min(7)] += ml;
        }
        b
    }
}

fn litres(ml: u32) -> String {
    format!("{:.1}", ml as f32 / 1000.0).replace('.', ",")
}

fn local_day(now: f64) -> i64 {
    ((now + util::local_offset_secs() as f64) / 86_400.0).floor() as i64
}

// ── Pager ─────────────────────────────────────────────────────────────────────

/// The pages of the dashboard group that are switched on, in order.
pub fn pages(s: &crate::settings::Settings) -> Vec<View> {
    let mut v = vec![View::Dashboard];
    if s.feature_on("focus") {
        v.push(View::Focus);
    }
    if s.feature_on("water") {
        v.push(View::Water);
    }
    v
}

pub fn is_page(v: View) -> bool {
    matches!(v, View::Dashboard | View::Focus | View::Water)
}

pub const PAGER_H: f32 = 14.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FocusKind {
    Focus,
    Short,
    Long,
}

impl FocusKind {
    pub fn minutes(self) -> f32 {
        match self {
            FocusKind::Focus => 25.0,
            FocusKind::Short => 5.0,
            FocusKind::Long => 15.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FocusKind::Focus => "Waktu fokus",
            FocusKind::Short => "Istirahat singkat",
            FocusKind::Long => "Istirahat panjang",
        }
    }
}

impl App {
    /// Chevrons and dots along the bottom edge of a page of the group.
    pub fn draw_pager(&mut self, g: &mut Gfx, iw: f32, ih: f32) {
        let pages = pages(&self.st.settings);
        let Some(idx) = pages.iter().position(|p| *p == self.st.view) else { return };
        if pages.len() < 2 {
            return;
        }
        let cy = ih - 11.0;
        let cx = iw / 2.0;
        let gap = 13.0;
        let span = (pages.len() - 1) as f32 * gap / 2.0;
        let mut go: Option<usize> = None;
        for i in 0..pages.len() {
            let x = cx - span + i as f32 * gap;
            let on = i == idx;
            let hit = Rect::new(x - 7.0, cy - 7.0, 14.0, 14.0);
            let (clicked, hover, _) = self.ui.click_region(id_of(&format!("page-dot-{i}"), 131), hit);
            g.fill_style(rgba(255, 255, 255, if on { 0.85 } else if hover { 0.55 } else { 0.28 }));
            if on {
                g.fill_round_rect(x - 5.0, cy - 2.0, 10.0, 4.0, 2.0);
            } else {
                g.fill_round_rect(x - 2.0, cy - 2.0, 4.0, 4.0, 2.0);
            }
            if clicked {
                go = Some(i);
            }
        }
        let reach = span + 22.0;
        if idx > 0 && self.ui.icon_button(g, "page-prev", cx - reach, cy, 16.0, Icon::ChevronLeft, 8.0) {
            go = Some(idx - 1);
        }
        if idx + 1 < pages.len() && self.ui.icon_button(g, "page-next", cx + reach, cy, 16.0, Icon::ChevronRight, 8.0) {
            go = Some(idx + 1);
        }
        if let Some(i) = go {
            if i != idx {
                crate::sound::play("blip");
                self.set_view(pages[i]);
            }
        }
    }

    // ── Water ────────────────────────────────────────────────────────────────

    pub fn water_today(&mut self) -> u32 {
        let day = local_day(util::unix_now());
        if self.water.roll(day) {
            self.water.save();
        }
        self.water.total()
    }

    /// One bottle more, from the page or from "Sudah" on a water reminder.
    pub fn water_drink(&mut self) {
        self.water_today();
        let ml = self.st.settings.water_bottle_ml.max(50);
        self.water.add(ml, util::unix_now());
        self.water.save();
    }

    pub fn draw_water(&mut self, g: &mut Gfx, v: Rect) {
        let goal = self.st.settings.water_goal_ml.max(250);
        let bottle = self.st.settings.water_bottle_ml.max(50);
        let total = self.water_today();
        let pct = (total as f32 / goal as f32 * 100.0).round() as u32;
        self.dash_page_title(g, v, "Air minum", &format!("Target {} L per hari", litres(goal)));

        let left = Rect::new(v.x, v.y, 288.0, v.h);
        let right = Rect::new(v.x + 298.0, v.y, v.w - 298.0, v.h);
        ui::card(g, left, Wash::None, false);
        ui::card(g, right, Wash::None, false);

        let x = left.x + 20.0;
        let big = litres(total);
        let bw = text::measure(&big, Face::Bold, 34.0);
        text::draw(g, &big, x, left.y + 34.0, Face::Bold, 34.0, hex(pal::INK), Align::Left);
        text::draw(g, &format!("/ {} L", litres(goal)), x + bw + 8.0, left.y + 40.0, Face::Regular, 14.0, hex(pal::DIM), Align::Left);
        let done = total >= goal;
        let sub = if done { format!("Hari ini · {pct}% · target tercapai") } else { format!("Hari ini · {pct}%") };
        text::draw(g, &sub, x, left.y + 62.0, Face::Medium, 11.5, hex(if done { pal::GREEN2 } else { pal::DIM }), Align::Left);

        // Progress bar.
        let bar = Rect::new(x, left.y + 74.0, left.w - 40.0, 6.0);
        g.fill_style(rgba(255, 255, 255, 0.10));
        g.fill_round_rect(bar.x, bar.y, bar.w, bar.h, 3.0);
        let k = clamp(total as f32 / goal as f32, 0.0, 1.0);
        if k > 0.0 {
            let grad = g.linear(bar.x, 0.0, bar.x + bar.w, 0.0, &[(0.0, hex("#19B8FF")), (1.0, hex("#2D5BFF"))]);
            g.fill_style(grad);
            g.fill_round_rect(bar.x, bar.y, (bar.w * k).max(6.0), bar.h, 3.0);
        }

        let (drink, dw) = self.ui.button(g, x, left.y + 92.0, &format!("Minum +{bottle} ml"), true, None);
        let (undo, _) = self.ui.button(g, x + dw + 8.0, left.y + 92.0, "Batal", false, None);
        if drink {
            crate::sound::play("approve");
            self.water_drink();
            self.buddy_care_done();
        } else if undo && !self.water.entries.is_empty() {
            self.water.undo();
            self.water.save();
        }

        // Right: intake through the day, in three-hour bars.
        let bins = self.water.bins(util::local_offset_secs());
        let peak = (*bins.iter().max().unwrap_or(&0)).max(bottle * 2) as f32;
        let area = Rect::new(right.x + 22.0, right.y + 26.0, right.w - 44.0, right.h - 26.0 - 30.0);
        text::draw(g, "Per 3 jam", right.x + 22.0, right.y + 14.0, Face::Medium, 11.0, hex(pal::DIM2), Align::Left);
        let now_bin = ((util::unix_now() + util::local_offset_secs() as f64).rem_euclid(86_400.0) / 3600.0 / 3.0) as usize;
        let slot = area.w / 8.0;
        for (i, ml) in bins.iter().enumerate() {
            let bx = area.x + i as f32 * slot + slot * 0.2;
            let bwid = slot * 0.6;
            g.fill_style(rgba(255, 255, 255, 0.06));
            g.fill_round_rect(bx, area.y, bwid, area.h, 3.0);
            if *ml > 0 {
                let h = (area.h * (*ml as f32 / peak)).max(4.0);
                let grad = g.linear(0.0, area.y + area.h - h, 0.0, area.y + area.h, &[(0.0, hex("#38BDF8")), (1.0, hex("#2D5BFF"))]);
                g.fill_style(grad);
                g.fill_round_rect(bx, area.y + area.h - h, bwid, h, 3.0);
            }
            if i == now_bin.min(7) {
                g.fill_style(hex("#38BDF8"));
                g.fill_round_rect(bx + bwid / 2.0 - 3.0, area.y + area.h + 5.0, 6.0, 2.0, 1.0);
            }
            if i % 2 == 0 {
                text::draw(g, &format!("{:02}", i * 3), bx + bwid / 2.0, area.y + area.h + 17.0, Face::Regular, 9.5, hex(pal::DIM3), Align::Center);
            }
        }
    }

    // ── Focus ────────────────────────────────────────────────────────────────

    pub fn draw_focus(&mut self, g: &mut Gfx, v: Rect) {
        self.buddy_today_pub();
        let kind = self.focus_kind;
        let running = activity::timer();
        let (left_s, total_s) = running.unwrap_or((kind.minutes() * 60.0, kind.minutes() * 60.0));
        let color = if kind == FocusKind::Focus { "#F4505E" } else { "#34D399" };
        self.dash_page_title(g, v, "Fokus", &format!("Pomodoro · {} sesi hari ini", self.buddy.focus_done));

        let card = Rect::new(v.x, v.y, v.w, v.h);
        ui::card(g, card, if kind == FocusKind::Focus { Wash::Red } else { Wash::Green }, false);
        let cy = card.cy();

        // The ring and its icon.
        let rc = (card.x + 70.0, cy);
        let r = 34.0;
        g.line_width(6.0);
        g.line_cap_round();
        g.stroke_style(rgba(255, 255, 255, 0.10));
        g.begin_path();
        g.circle(rc.0, rc.1, r);
        g.stroke();
        let k = clamp(left_s / total_s.max(1.0), 0.0, 1.0);
        if k > 0.004 {
            g.stroke_style(hex(color));
            g.begin_path();
            let a0 = -std::f32::consts::FRAC_PI_2;
            g.arc(rc.0, rc.1, r, a0, a0 + k * std::f32::consts::TAU, false);
            g.stroke();
        }
        crate::icons::fill(g, Icon::Timer, rc.0, rc.1, 26.0, hex(color));

        // The time and what it is.
        let tx = card.x + 128.0;
        text::draw(g, &activity::format_clock(left_s), tx, cy - 8.0, Face::Bold, 38.0, hex(pal::INK), Align::Left);
        text::draw(g, kind.label(), tx, cy + 24.0, Face::Regular, 12.5, hex(pal::DIM), Align::Left);

        // Kind pills and the big play / stop button.
        let px = card.x + card.w - 250.0;
        let mut x = px;
        for (k2, label) in [(FocusKind::Short, "Istirahat"), (FocusKind::Focus, "Fokus"), (FocusKind::Long, "Panjang")] {
            let w = text::measure(label, Face::Medium, 11.5) + 22.0;
            let rr = Rect::new(x, cy + 14.0, w, 26.0);
            let (clicked, hover, _) = self.ui.click_region(id_of(&format!("focus-{label}"), 133), rr);
            let on = kind == k2;
            g.fill_style(if on { with_alpha(hex(color), 0.30) } else { rgba(255, 255, 255, if hover { 0.14 } else { 0.08 }) });
            g.fill_round_rect(rr.x, rr.y, rr.w, rr.h, 13.0);
            text::draw(g, label, rr.cx(), rr.cy(), Face::Medium, 11.5, hex(if on { pal::INK } else { pal::DIM }), Align::Center);
            x += w + 6.0;
            if clicked && running.is_none() && !on {
                self.focus_kind = k2;
            }
        }
        let bc = (card.x + card.w - 46.0, cy - 14.0);
        let hit = Rect::new(bc.0 - 24.0, bc.1 - 24.0, 48.0, 48.0);
        let (clicked, hover, pressed) = self.ui.click_region(id_of("focus-play", 134), hit);
        let rad = if pressed { 21.0 } else if hover { 24.0 } else { 23.0 };
        g.fill_style(hex(color));
        g.begin_path();
        g.circle(bc.0, bc.1, rad);
        g.fill();
        g.fill_style(hex("#0B0C0E"));
        if running.is_some() {
            g.fill_round_rect(bc.0 - 7.0, bc.1 - 7.0, 14.0, 14.0, 3.0);
        } else {
            crate::dashboard::glyph_play(g, bc.0 + 1.0, bc.1, 16.0);
        }
        if clicked {
            if running.is_some() {
                activity::cancel_timer();
            } else {
                activity::start_timer(kind.minutes());
                self.last_timer_total = kind.minutes() * 60.0;
                crate::sound::play("blip");
            }
            self.ensure_running();
        }
    }

    /// A timer ran out: count a finished focus session and move to the matching break.
    pub fn on_focus_finished(&mut self) {
        let was_focus = self.last_timer_total >= 20.0 * 60.0 && self.focus_kind == FocusKind::Focus;
        let (title, sub) = if was_focus {
            self.buddy_today_pub();
            self.buddy.focus_done += 1;
            self.buddy.save();
            self.focus_kind = FocusKind::Short;
            ("Sesi fokus selesai".to_string(), format!("Istirahat 5 menit · {} sesi hari ini", self.buddy.focus_done))
        } else if self.focus_kind != FocusKind::Focus {
            self.focus_kind = FocusKind::Focus;
            ("Istirahat selesai".to_string(), "Siap fokus lagi?".to_string())
        } else {
            ("Timer selesai".to_string(), String::new())
        };
        self.show_overlay(crate::overlay::Overlay {
            kind: crate::overlay::OverlayKind::Info,
            title,
            sub,
            metric: String::new(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drinks_add_up_and_undo_and_a_new_day_starts_empty() {
        let mut w = Water { day: 10, ..Default::default() };
        w.add(250, 1_000.0);
        w.add(250, 2_000.0);
        assert_eq!(w.total(), 500);
        w.undo();
        assert_eq!(w.total(), 250);
        assert!(w.roll(11));
        assert_eq!(w.total(), 0);
        assert!(!w.roll(11));
    }

    #[test]
    fn the_chart_bins_are_three_hours_wide() {
        let mut w = Water::default();
        // 07:30 and 08:50 local (offset 0) fall in the 06–09 bin; 21:00 in the last.
        w.add(200, 7.5 * 3600.0);
        w.add(300, 8.0 * 3600.0 + 3000.0);
        w.add(250, 21.0 * 3600.0);
        let b = w.bins(0);
        assert_eq!(b[2], 500);
        assert_eq!(b[7], 250);
        // The local offset shifts the bins: 07:30 UTC is 14:30 at +7.
        assert_eq!(Water { day: 0, entries: vec![(7.5 * 3600.0, 100)] }.bins(7 * 3600)[4], 100);
    }

    #[test]
    fn litres_use_a_decimal_comma() {
        assert_eq!(litres(1500), "1,5");
        assert_eq!(litres(2000), "2,0");
    }

    #[test]
    fn the_pages_follow_the_feature_switches() {
        let mut s = crate::settings::Settings::default();
        assert_eq!(pages(&s), vec![View::Dashboard, View::Focus, View::Water]);
        s.disabled_features.push("focus".into());
        assert_eq!(pages(&s), vec![View::Dashboard, View::Water]);
        s.disabled_features.push("water".into());
        assert_eq!(pages(&s), vec![View::Dashboard]);
    }

    #[test]
    fn focus_kinds_have_the_usual_lengths() {
        assert_eq!(FocusKind::Focus.minutes(), 25.0);
        assert_eq!(FocusKind::Short.minutes(), 5.0);
        assert_eq!(FocusKind::Long.minutes(), 15.0);
    }
}
