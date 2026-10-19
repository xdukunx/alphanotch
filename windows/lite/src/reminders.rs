// Wellness reminders: drink water, stretch, rest your eyes.
//
// Opt-in and quiet: nothing is scheduled until an interval is chosen in Settings, and
// the only cost while the island is hidden is one 30 s wake-up from a Win32 timer.
// A reminder never interrupts an approval or a question — it waits for the next tick.

use crate::app::App;
use crate::layout::{Mode, View};
use crate::mochi::engine::Emote;
use crate::{settings::Settings, sound, util};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RemKind {
    Water,
    Stretch,
    Eyes,
    /// Work suggestions for today.
    Suggest,
    /// Claude usage: plenty of room / careful.
    LimitPlenty,
    LimitFast,
}

const KINDS: [RemKind; 3] = [RemKind::Water, RemKind::Stretch, RemKind::Eyes];

impl RemKind {
    /// Slot in the interval schedule; the coach cards have none.
    fn idx(self) -> usize {
        match self {
            RemKind::Water => 0,
            RemKind::Stretch => 1,
            _ => 2,
        }
    }

    /// An interval reminder (can be snoozed), as opposed to a coach card.
    pub fn is_interval(self) -> bool {
        matches!(self, RemKind::Water | RemKind::Stretch | RemKind::Eyes)
    }
}

#[derive(Clone, Debug)]
pub struct ReminderCard {
    pub kind: RemKind,
    pub title: String,
    pub body: String,
}

fn every(s: &Settings, k: RemKind) -> u32 {
    match k {
        RemKind::Water => s.water_every_min,
        RemKind::Stretch => s.stretch_every_min,
        RemKind::Eyes => s.eyes_every_min,
        _ => 0,
    }
}

/// The text of a reminder; `name` is the person's display name, possibly empty.
pub fn card_for(kind: RemKind, name: &str) -> ReminderCard {
    let who = if name.is_empty() { String::new() } else { format!(", {name}") };
    let (title, body) = match kind {
        RemKind::Water => (
            format!("Waktunya minum air{who}"),
            "Segelas air dulu, biar fokus tetap jalan.".to_string(),
        ),
        RemKind::Stretch => (
            format!("Waktunya peregangan{who}"),
            "Berdiri, putar bahu, dan regangkan leher selama 30 detik.".to_string(),
        ),
        RemKind::Eyes => (
            format!("Istirahatkan matamu{who}"),
            "Lihat sesuatu yang jauh (sekitar 6 meter) selama 20 detik.".to_string(),
        ),
        RemKind::Suggest => (format!("Saran hari ini{who}"), String::new()),
        RemKind::LimitPlenty => (String::new(), String::new()),
        RemKind::LimitFast => (String::new(), String::new()),
    };
    ReminderCard { kind, title, body }
}

/// The card for a usage estimate; `pct` is the share of the user's own busiest stretch.
pub fn limit_card(alert: crate::coach::LimitAlert) -> ReminderCard {
    use crate::coach::LimitAlert::*;
    match alert {
        Plenty { pct } => ReminderCard {
            kind: RemKind::LimitPlenty,
            title: "Limit Claude masih longgar".into(),
            body: format!("Lima jam terakhir baru ±{pct}% dari puncak pemakaianmu. Pakai dong, sayang kalau tidak terpakai. (perkiraan)"),
        },
        Fast { pct } => ReminderCard {
            kind: RemKind::LimitFast,
            title: "Hati-hati, pemakaian Claude ngebut".into(),
            body: format!("Sudah ±{pct}% dari puncak pemakaianmu. Pelan-pelan biar tidak habis sebelum limit reset. (perkiraan)"),
        },
    }
}

impl App {
    /// Starts or stops the 30 s timer, and restarts the schedule of any interval that changed.
    pub fn sync_reminder_timer(&mut self) {
        let now = util::unix_now();
        let mut any = false;
        for k in KINDS {
            let i = k.idx();
            let e = every(&self.st.settings, k);
            if e != self.rem_cfg[i] {
                self.rem_cfg[i] = e;
                self.rem_next[i] = if e > 0 { now + e as f64 * 60.0 } else { 0.0 };
            }
            any |= e > 0;
        }
        let s = &self.st.settings;
        any |= s.coach_suggestions || s.coach_limit;
        // The resting clock needs a wake-up every half minute anyway.
        let _ = any;
        self.platform.set_remind_timer(true);
    }

    pub fn reminder_tick(&mut self) {
        self.clock_tick();
        // Never over a decision somebody has to make, and not while one is already up.
        if self.st.paused || self.st.pending_approval.is_some() || self.st.pending_question.is_some() {
            return;
        }
        if self.st.mode == Mode::Expanded && self.st.view == View::Reminder {
            return;
        }
        let now = util::unix_now();
        for k in KINDS {
            let i = k.idx();
            let e = every(&self.st.settings, k);
            if e > 0 && self.rem_next[i] > 0.0 && now >= self.rem_next[i] {
                self.rem_next[i] = now + e as f64 * 60.0;
                self.fire_reminder(k);
                return; // one at a time; the next one waits for the next tick
            }
        }
        self.coach_tick(now);
    }

    /// Usage alerts and the daily suggestions. Quiet by construction: at most four cards a
    /// day, the same alert never twice within four hours, suggestions once per day.
    fn coach_tick(&mut self, now: f64) {
        let (want_sug, want_lim) = (self.st.settings.coach_suggestions, self.st.settings.coach_limit);
        if !(want_sug || want_lim) {
            return;
        }
        crate::coach::kick_scan();
        if !crate::coach::ready() {
            return;
        }
        let off = util::local_offset_secs() as f64;
        let day = ((now + off) / 86_400.0).floor() as i64;
        if day != self.coach_day {
            self.coach_day = day;
            self.coach_popups = 0;
        }
        if self.coach_popups >= 4 {
            return;
        }
        let name = self.st.settings.display_name.trim().to_string();

        if want_lim {
            if let Some(a) = crate::coach::limit_alert(now) {
                let slot = matches!(a, crate::coach::LimitAlert::Fast { .. }) as usize;
                if now - self.coach_alert_at[slot] > 4.0 * 3600.0 {
                    self.coach_alert_at[slot] = now;
                    self.coach_popups += 1;
                    self.show_coach_card(limit_card(a), if slot == 1 { Emote::Surprised } else { Emote::Happy });
                    return;
                }
            }
        }
        let hour = ((now + off) / 3600.0).floor() as i64 % 24;
        if want_sug && hour >= 8 && coach_day_done() != Some(day) {
            let s = crate::coach::suggestions(now);
            if !s.is_empty() {
                mark_coach_day(day);
                self.coach_popups += 1;
                let mut card = card_for(RemKind::Suggest, &name);
                card.body = s.join("\n");
                self.show_coach_card(card, Emote::Proud);
            }
        }
    }

    fn show_coach_card(&mut self, card: ReminderCard, emote: Emote) {
        self.reminder = Some(card);
        sound::play("greet");
        self.engine.trigger_emote(emote, 3.0);
        self.alert(View::Reminder);
        self.ensure_running();
    }

    fn fire_reminder(&mut self, kind: RemKind) {
        let name = self.st.settings.display_name.trim().to_string();
        let mut card = card_for(kind, &name);
        if let Some(line) = self.streak_line() {
            card.body = format!("{}\n{}", card.body, line);
        }
        self.reminder = Some(card);
        sound::play("greet");
        self.engine.trigger_emote(
            match kind {
                RemKind::Water => Emote::Happy,
                RemKind::Stretch => Emote::Yawn,
                _ => Emote::Wink,
            },
            3.0,
        );
        self.alert(View::Reminder);
        self.ensure_running();
    }

    /// "Sudah" closes it; "Tunda" brings the same reminder back in 10 minutes.
    pub fn dismiss_reminder(&mut self, snooze: bool) {
        let mut cared = false;
        let mut cared_water = false;
        if let Some(card) = self.reminder.take() {
            cared_water = !snooze && card.kind == RemKind::Water;
            if snooze && card.kind.is_interval() {
                self.rem_next[card.kind.idx()] = util::unix_now() + 10.0 * 60.0;
            }
            cared = !snooze && card.kind.is_interval();
        }
        if cared {
            if cared_water {
                self.water_drink();
            }
            self.buddy_care_done();
        }
        sound::play("blip");
        self.collapse();
    }
}

fn coach_day_file() -> std::path::PathBuf {
    crate::settings::local_dir().join("coach.day")
}

/// The local day number the suggestions were last shown on (survives restarts).
fn coach_day_done() -> Option<i64> {
    std::fs::read_to_string(coach_day_file()).ok()?.trim().parse().ok()
}

fn mark_coach_day(day: i64) {
    let _ = std::fs::create_dir_all(crate::settings::local_dir());
    let _ = std::fs::write(coach_day_file(), day.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_is_used_when_there_is_one() {
        assert_eq!(card_for(RemKind::Water, "Dhana").title, "Waktunya minum air, Dhana");
        assert_eq!(card_for(RemKind::Water, "").title, "Waktunya minum air");
        assert!(card_for(RemKind::Eyes, "").body.contains("20 detik"));
    }

    #[test]
    fn coach_cards_carry_the_estimate_label_and_cannot_be_snoozed() {
        let c = limit_card(crate::coach::LimitAlert::Fast { pct: 91 });
        assert!(c.body.contains("91") && c.body.contains("perkiraan"));
        assert!(!c.kind.is_interval() && RemKind::Water.is_interval());
        assert!(limit_card(crate::coach::LimitAlert::Plenty { pct: 12 }).title.contains("longgar"));
    }

    #[test]
    fn a_fresh_install_has_every_reminder_off_and_every_feature_on() {
        let s = Settings::default();
        assert_eq!((s.water_every_min, s.stretch_every_min, s.eyes_every_min), (0, 0, 0));
        assert!(s.feature_on("stocks") && s.feature_on("chat"));
        let mut s2 = s.clone();
        s2.disabled_features.push("stocks".into());
        assert!(!s2.feature_on("stocks") && s2.feature_on("weather"));
    }

    #[test]
    fn an_older_settings_file_still_loads() {
        let old = r#"{"soundEnabled":true,"soundVolume":0.12,"autoCloseInterval":15.0,"absenceInterval":180.0,
            "activeIntegrations":[],"screen":"primary","autostart":false,"hooksInstalled":false}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert!(s.disabled_features.is_empty() && s.water_every_min == 0);
    }
}
