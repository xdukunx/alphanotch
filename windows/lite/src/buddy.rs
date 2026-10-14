// Mochi as a companion: mood, a care streak, small celebrations, idle hobbies,
// falling asleep when nobody is around, and three daily quests that earn crowns.
//
// Light by construction: nothing here has a timer of its own. It rides the island's
// existing 30 Hz poll (which only runs while the island is on screen), every check is a
// couple of comparisons, and the only persistence is a tiny JSON file written when a
// counter changes.

use serde::{Deserialize, Serialize};

use crate::anim::now;
use crate::app::App;
use crate::layout::Mode;
use crate::mochi::engine::{BotState, Emote};
use crate::reminders::{RemKind, ReminderCard};
use crate::{sound, util};

/// No input for this long and Mochi falls asleep.
const SLEEP_AFTER: f32 = 600.0;
/// A hobby is possible after this much quiet.
const HOBBY_AFTER: f32 = 45.0;
/// A session this long deserves confetti when it ends.
const LONG_SESSION: f64 = 600.0;

#[derive(Default, Serialize, Deserialize, Clone, Debug)]
#[serde(default, rename_all = "camelCase")]
pub struct Buddy {
    /// The local day the counters below belong to.
    pub day: i64,
    /// Interval reminders confirmed with "Sudah" today.
    pub care: u32,
    pub tasks_done: u32,
    pub clean_sessions: u32,
    /// The day the crown for the quests was handed out (so it happens once a day).
    pub rewarded_day: i64,
    pub crowns: u32,
    /// Consecutive days with at least one confirmed care reminder, and the last such day.
    pub streak: u32,
    pub streak_day: i64,
    #[serde(skip)]
    pub errors: Vec<f64>,
    #[serde(skip)]
    pub errors_since_stop: u32,
    #[serde(skip)]
    pub last_clean_stop: f64,
    #[serde(skip)]
    pub prompt_at: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Quest {
    pub title: String,
    pub done: u32,
    pub target: u32,
}

impl Quest {
    pub fn complete(&self) -> bool {
        self.done >= self.target
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mood {
    Tired,
    Sad,
    Happy,
    Proud,
    Calm,
}

impl Buddy {
    fn path() -> std::path::PathBuf {
        crate::settings::config_dir().join("buddy.json")
    }

    pub fn load() -> Buddy {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(crate::settings::config_dir());
        if let Ok(s) = serde_json::to_string(self) {
            let _ = std::fs::write(Self::path(), s);
        }
    }

    /// Starts a new day: the daily counters reset, the streak and the crowns stay.
    pub fn roll(&mut self, day: i64) -> bool {
        if self.day == day {
            return false;
        }
        self.day = day;
        self.care = 0;
        self.tasks_done = 0;
        self.clean_sessions = 0;
        true
    }

    /// A care reminder was confirmed today. Returns the new streak when it just grew.
    pub fn confirm_care(&mut self, day: i64) -> Option<u32> {
        self.roll(day);
        self.care += 1;
        if self.streak_day == day {
            return None;
        }
        self.streak = if self.streak_day == day - 1 { self.streak + 1 } else { 1 };
        self.streak_day = day;
        Some(self.streak)
    }

    /// The streak as it stands on `day`: broken if yesterday had no confirmation either.
    pub fn streak_on(&self, day: i64) -> u32 {
        if self.streak_day == day || self.streak_day == day - 1 { self.streak } else { 0 }
    }

    pub fn quests(&self, care_enabled: bool) -> Vec<Quest> {
        vec![
            Quest { title: "Selesaikan 1 tugas".into(), done: self.tasks_done, target: 1 },
            // Without any reminder switched on this one could never be met: skip it.
            Quest {
                title: "Jaga dirimu 4 kali (minum / regang)".into(),
                done: self.care,
                target: if care_enabled { 4 } else { 0 },
            },
            Quest { title: "Satu sesi Claude tanpa error".into(), done: self.clean_sessions, target: 1 },
        ]
    }

    pub fn mood(&self, now_s: f64, local_hour: i64) -> Mood {
        let recent_errors = self.errors.iter().filter(|t| now_s - **t < 1800.0).count();
        if recent_errors >= 3 {
            Mood::Sad
        } else if self.rewarded_day == self.day && self.day != 0 {
            Mood::Proud
        } else if now_s - self.last_clean_stop < 600.0 && self.last_clean_stop > 0.0 {
            Mood::Happy
        } else if !(5..23).contains(&local_hour) {
            Mood::Tired
        } else {
            Mood::Calm
        }
    }
}

/// (tier, name, top colour, bottom colour) of the crown for the number of quest days.
pub fn crown_tier(crowns: u32) -> (u8, &'static str) {
    match crowns {
        0 => (0, "Emas"),
        1..=2 => (1, "Rose gold"),
        3..=6 => (2, "Zamrud"),
        7..=13 => (3, "Safir"),
        _ => (4, "Ametis"),
    }
}

fn local_day(now_s: f64) -> i64 {
    ((now_s + util::local_offset_secs() as f64) / 86_400.0).floor() as i64
}

fn local_hour(now_s: f64) -> i64 {
    (((now_s + util::local_offset_secs() as f64) / 3600.0).floor() as i64).rem_euclid(24)
}

impl App {
    fn buddy_today(&mut self) -> i64 {
        let day = local_day(util::unix_now());
        if self.buddy.roll(day) {
            self.buddy.save();
        }
        day
    }

    pub fn apply_crown(&self) {
        crate::mochi::character::set_crown_tier(crown_tier(self.buddy.crowns).0);
    }

    /// Confetti, a proud face and a little jingle.
    pub fn celebrate(&mut self) {
        self.engine.confetti(26);
        self.engine.trigger_emote(Emote::Proud, 2.2);
        sound::play("love");
        self.ensure_running();
    }

    /// Something happened: Mochi is not asleep any more, and startles if it was.
    pub fn note_activity(&mut self) {
        self.last_input = now();
        if self.sleeping {
            self.sleeping = false;
            if self.st.state_override == Some(BotState::Sleeping) {
                self.st.state_override = None;
            }
            self.engine.trigger_emote(Emote::Surprised, 1.2);
            self.ensure_running();
        }
    }

    /// "Sudah" on a water / stretch / eyes card.
    pub fn buddy_care_done(&mut self) {
        let day = self.buddy_today();
        let grew = self.buddy.confirm_care(day);
        self.buddy.save();
        if let Some(s) = grew {
            if matches!(s, 3 | 7 | 14 | 30 | 60 | 100) {
                self.celebrate();
            }
        }
        self.check_quests();
    }

    /// A task was ticked off in the dashboard.
    pub fn buddy_task_done(&mut self) {
        self.buddy_today();
        self.buddy.tasks_done += 1;
        self.buddy.save();
        self.celebrate();
        self.check_quests();
    }

    /// Claude Code session events, for the clean-session quest, mood and long-session confetti.
    pub fn buddy_session_event(&mut self, event: &str) {
        let t = util::unix_now();
        self.buddy_today();
        match event {
            "UserPromptSubmit" => {
                if self.buddy.prompt_at == 0.0 {
                    self.buddy.prompt_at = t;
                }
            }
            "PostToolUseFailure" | "StopFailure" => {
                self.buddy.errors.push(t);
                self.buddy.errors_since_stop += 1;
                if self.buddy.errors.len() > 40 {
                    self.buddy.errors.remove(0);
                }
            }
            "Stop" => {
                let clean = self.buddy.errors_since_stop == 0;
                let long = self.buddy.prompt_at > 0.0 && t - self.buddy.prompt_at > LONG_SESSION;
                if clean {
                    self.buddy.clean_sessions += 1;
                    self.buddy.last_clean_stop = t;
                }
                self.buddy.errors_since_stop = 0;
                self.buddy.prompt_at = 0.0;
                self.buddy.save();
                if clean && long {
                    self.celebrate();
                }
                self.check_quests();
            }
            _ => {}
        }
    }

    /// Hands out the crown when the three quests are done, once per day.
    fn check_quests(&mut self) {
        let care_on = {
            let s = &self.st.settings;
            s.water_every_min > 0 || s.stretch_every_min > 0 || s.eyes_every_min > 0
        };
        let all = self.buddy.quests(care_on).iter().all(Quest::complete);
        if !all || self.buddy.rewarded_day == self.buddy.day {
            return;
        }
        self.buddy.rewarded_day = self.buddy.day;
        self.buddy.crowns += 1;
        self.buddy.save();
        self.apply_crown();
        let (_, name) = crown_tier(self.buddy.crowns);
        let n = self.buddy.crowns;
        self.celebrate();
        self.reminder = Some(ReminderCard {
            kind: RemKind::Suggest,
            title: "Misi hari ini tuntas!".into(),
            body: format!("Mahkota Mochi sekarang: {name}. Total {n} hari misi tuntas."),
        });
        self.alert(crate::layout::View::Reminder);
    }

    /// Streak line appended to care reminders.
    pub fn streak_line(&self) -> Option<String> {
        let s = self.buddy.streak_on(local_day(util::unix_now()));
        (s >= 1).then(|| format!("Beruntun {s} hari berturut-turut"))
    }

    /// Mood emotes, idle hobbies and falling asleep. Called from the 30 Hz poll.
    pub fn buddy_poll(&mut self) {
        if self.st.mode == Mode::Hidden || self.st.paused {
            return;
        }
        let n = now();
        if self.last_input == 0.0 {
            self.last_input = n;
            self.next_hobby = n + 60.0;
            self.next_mood = n + 120.0;
        }
        let quiet = n - self.last_input;
        let idle = self.st.effective_state() == BotState::Idle || self.sleeping;

        // Sleep.
        if !self.sleeping
            && quiet > SLEEP_AFTER
            && idle
            && self.st.state_override.is_none()
            && self.st.pending_approval.is_none()
            && self.st.pending_question.is_none()
            && self.st.mode != Mode::Expanded
        {
            self.sleeping = true;
            self.st.state_override = Some(BotState::Sleeping);
            self.ensure_running();
            return;
        }
        if self.sleeping || !idle || self.st.mode == Mode::Expanded {
            return;
        }

        let t = util::unix_now();
        // Mood: an emote now and then that says how the day is going.
        if n >= self.next_mood {
            self.next_mood = n + 150.0 + (t.fract() as f32) * 120.0;
            let hour = local_hour(t);
            match self.buddy.mood(t, hour) {
                Mood::Tired => {
                    self.engine.trigger_emote(Emote::Yawn, 2.0);
                    self.ensure_running();
                }
                Mood::Sad => {
                    self.engine.trigger_emote(Emote::Annoyed, 1.4);
                    self.ensure_running();
                }
                Mood::Happy => {
                    self.engine.trigger_emote(Emote::Happy, 1.6);
                    self.ensure_running();
                }
                Mood::Proud => {
                    self.engine.trigger_emote(Emote::Proud, 2.0);
                    self.ensure_running();
                }
                Mood::Calm => {}
            }
            return;
        }

        // Hobbies, only after a little quiet and never while the user is working with it.
        if quiet > HOBBY_AFTER && n >= self.next_hobby {
            self.next_hobby = n + 55.0 + (t.fract() as f32) * 70.0;
            match ((t * 1000.0) as u64) % 3 {
                0 => self.engine.hum(),
                1 => self.engine.stargaze(3.2),
                _ => self.engine.trigger_emote(Emote::Wink, 1.4),
            }
            self.ensure_running();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_streak_grows_per_day_and_breaks_after_a_gap() {
        let mut b = Buddy::default();
        assert_eq!(b.confirm_care(100), Some(1));
        assert_eq!(b.confirm_care(100), None); // same day: no change
        assert_eq!(b.confirm_care(101), Some(2));
        assert_eq!(b.streak_on(102), 2); // yesterday counted, today still open
        assert_eq!(b.streak_on(103), 0); // a missed day breaks it
        assert_eq!(b.confirm_care(104), Some(1));
        assert_eq!(b.care, 1);
    }

    #[test]
    fn a_new_day_resets_the_daily_counters_but_not_crowns() {
        let mut b = Buddy { day: 5, care: 3, tasks_done: 2, clean_sessions: 1, crowns: 4, streak: 6, streak_day: 5, ..Default::default() };
        assert!(b.roll(6));
        assert_eq!((b.care, b.tasks_done, b.clean_sessions), (0, 0, 0));
        assert_eq!((b.crowns, b.streak), (4, 6));
        assert!(!b.roll(6));
    }

    #[test]
    fn quests_without_any_reminder_never_block_the_crown() {
        let b = Buddy { tasks_done: 1, clean_sessions: 1, ..Default::default() };
        assert!(b.quests(false).iter().all(Quest::complete));
        assert!(!b.quests(true).iter().all(Quest::complete));
        let c = Buddy { tasks_done: 1, clean_sessions: 1, care: 4, ..Default::default() };
        assert!(c.quests(true).iter().all(Quest::complete));
    }

    #[test]
    fn mood_follows_errors_then_wins_then_the_clock() {
        let t = 1_000_000.0;
        let mut b = Buddy { day: 7, ..Default::default() };
        assert_eq!(b.mood(t, 14), Mood::Calm);
        assert_eq!(b.mood(t, 2), Mood::Tired);
        b.last_clean_stop = t - 60.0;
        assert_eq!(b.mood(t, 14), Mood::Happy);
        b.rewarded_day = 7;
        assert_eq!(b.mood(t, 14), Mood::Proud);
        b.errors = vec![t - 10.0, t - 20.0, t - 30.0];
        assert_eq!(b.mood(t, 14), Mood::Sad);
    }

    #[test]
    fn crown_tiers_climb_with_quest_days() {
        assert_eq!(crown_tier(0).0, 0);
        assert_eq!(crown_tier(1).0, 1);
        assert_eq!(crown_tier(3).0, 2);
        assert_eq!(crown_tier(7).0, 3);
        assert_eq!(crown_tier(20).0, 4);
    }

    #[test]
    fn an_older_buddy_file_with_missing_fields_still_loads() {
        let b: Buddy = serde_json::from_str(r#"{"crowns":2}"#).unwrap();
        assert_eq!((b.crowns, b.streak, b.care), (2, 0, 0));
    }
}
