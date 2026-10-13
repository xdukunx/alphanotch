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
}

const KINDS: [RemKind; 3] = [RemKind::Water, RemKind::Stretch, RemKind::Eyes];

impl RemKind {
    fn idx(self) -> usize {
        match self {
            RemKind::Water => 0,
            RemKind::Stretch => 1,
            RemKind::Eyes => 2,
        }
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
    };
    ReminderCard { kind, title, body }
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
        self.platform.set_remind_timer(any);
    }

    pub fn reminder_tick(&mut self) {
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
                break; // one at a time; the next one waits for the next tick
            }
        }
    }

    fn fire_reminder(&mut self, kind: RemKind) {
        let name = self.st.settings.display_name.trim().to_string();
        self.reminder = Some(card_for(kind, &name));
        sound::play("greet");
        self.engine.trigger_emote(
            match kind {
                RemKind::Water => Emote::Happy,
                RemKind::Stretch => Emote::Yawn,
                RemKind::Eyes => Emote::Wink,
            },
            3.0,
        );
        self.alert(View::Reminder);
        self.ensure_running();
    }

    /// "Sudah" closes it; "Tunda" brings the same reminder back in 10 minutes.
    pub fn dismiss_reminder(&mut self, snooze: bool) {
        if let Some(card) = self.reminder.take() {
            if snooze {
                self.rem_next[card.kind.idx()] = util::unix_now() + 10.0 * 60.0;
            }
        }
        sound::play("blip");
        self.collapse();
    }
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
