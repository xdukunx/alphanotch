// Prayer time alarms, like the water and stretch reminders.
//
// The times are computed on this computer (no service, nothing downloaded): the sun's
// position for the day and the place, with the method of Indonesia's Ministry of Religious
// Affairs (Kemenag RI): Fajr with the sun 20° below the horizon, Isha at 18°, Asr by the
// Shafi'i shadow rule (one shadow length), sunrise and sunset at 0.833°, and two minutes of
// ihtiyat on every time. The place is the one the weather page already uses.
//
// It is an estimate good to a minute or two: compare it with your local mosque's or
// Kemenag's schedule before relying on it.

use crate::app::App;
use crate::layout::View;
use crate::mochi::engine::Emote;
use crate::reminders::{RemKind, ReminderCard};
use crate::{sound, util};

pub const NAMES: [&str; 5] = ["Subuh", "Dzuhur", "Ashar", "Maghrib", "Isya"];
/// A time is still announced for this long after it has come (a restart, a busy moment).
const GRACE: f64 = 15.0 * 60.0;
/// Added to every computed time, as Kemenag's tables do.
const IHTIYAT_MIN: f64 = 2.0;

#[derive(Clone, Copy, Debug)]
pub struct Times {
    pub fajr: f64,
    pub sunrise: f64,
    pub dhuhr: f64,
    pub asr: f64,
    pub maghrib: f64,
    pub isha: f64,
}

impl Times {
    /// The five prayers, in the order of `NAMES`, as local hours with ihtiyat.
    pub fn five(&self) -> [f64; 5] {
        let a = IHTIYAT_MIN / 60.0;
        [self.fajr + a, self.dhuhr + a, self.asr + a, self.maghrib + a, self.isha + a]
    }
}

fn rad(d: f64) -> f64 {
    d.to_radians()
}
fn deg(r: f64) -> f64 {
    r.to_degrees()
}
fn fix_angle(a: f64) -> f64 {
    a.rem_euclid(360.0)
}
fn fix_hour(a: f64) -> f64 {
    a.rem_euclid(24.0)
}

fn julian(y: i64, m: i64, d: i64) -> f64 {
    let (mut y, mut m) = (y, m);
    if m <= 2 {
        y -= 1;
        m += 12;
    }
    let a = (y as f64 / 100.0).floor();
    let b = 2.0 - a + (a / 4.0).floor();
    (365.25 * (y as f64 + 4716.0)).floor() + (30.6001 * (m as f64 + 1.0)).floor() + d as f64 + b - 1524.5
}

/// (declination in degrees, equation of time in hours) at a Julian date.
fn sun_position(jd: f64) -> (f64, f64) {
    let d = jd - 2_451_545.0;
    let g = fix_angle(357.529 + 0.985_600_28 * d);
    let q = fix_angle(280.459 + 0.985_647_36 * d);
    let l = fix_angle(q + 1.915 * rad(g).sin() + 0.020 * rad(2.0 * g).sin());
    let e = 23.439 - 0.000_000_36 * d;
    let ra = fix_hour(deg((rad(e).cos() * rad(l).sin()).atan2(rad(l).cos())) / 15.0);
    let mut eqt = q / 15.0 - ra;
    if eqt > 12.0 {
        eqt -= 24.0;
    } else if eqt < -12.0 {
        eqt += 24.0;
    }
    let decl = deg((rad(e).sin() * rad(l).sin()).asin());
    (decl, eqt)
}

fn mid_day(jd: f64, t: f64) -> f64 {
    fix_hour(12.0 - sun_position(jd + t).1)
}

/// The time the sun is `angle` degrees below the horizon (before noon when `ccw`).
fn sun_angle_time(jd: f64, lat: f64, angle: f64, t: f64, ccw: bool) -> f64 {
    let decl = sun_position(jd + t).0;
    let noon = mid_day(jd, t);
    let x = (-rad(angle).sin() - rad(decl).sin() * rad(lat).sin()) / (rad(decl).cos() * rad(lat).cos());
    if x.abs() > 1.0 {
        return f64::NAN;
    }
    let dt = deg(x.acos()) / 15.0;
    noon + if ccw { -dt } else { dt }
}

/// Asr: the afternoon when a shadow is `factor` times the object plus its noon shadow.
fn asr_time(jd: f64, lat: f64, factor: f64, t: f64) -> f64 {
    let decl = sun_position(jd + t).0;
    let angle = -deg((1.0f64).atan2(factor + rad((lat - decl).abs()).tan()));
    sun_angle_time(jd, lat, angle, t, false)
}

/// Prayer times for a civil date, a place, and a time zone in hours east of UTC. All in local
/// hours (14.5 = 14:30). NaN where the sun never reaches the angle (far from the tropics).
pub fn compute(y: i64, m: i64, d: i64, lat: f64, lon: f64, tz: f64) -> Times {
    let jd = julian(y, m, d) - lon / (15.0 * 24.0);
    let shift = tz - lon / 15.0;
    let at = |t: [f64; 6]| -> Times {
        Times {
            fajr: sun_angle_time(jd, lat, 20.0, t[0], true) + shift,
            sunrise: sun_angle_time(jd, lat, 0.833, t[1], true) + shift,
            dhuhr: mid_day(jd, t[2]) + shift,
            asr: asr_time(jd, lat, 1.0, t[3]) + shift,
            maghrib: sun_angle_time(jd, lat, 0.833, t[4], false) + shift,
            isha: sun_angle_time(jd, lat, 18.0, t[5], false) + shift,
        }
    };
    // Two passes: the sun moves during the day, so the second uses the first one's times.
    let first = at([5.0 / 24.0, 6.0 / 24.0, 12.0 / 24.0, 13.0 / 24.0, 18.0 / 24.0, 18.0 / 24.0]);
    let portion = |h: f64| (h - shift) / 24.0;
    at([portion(first.fajr), portion(first.sunrise), portion(first.dhuhr), portion(first.asr), portion(first.maghrib), portion(first.isha)])
}

/// "11:13" for local hours, rounded up to the minute (as a printed schedule does).
pub fn hhmm(hours: f64) -> String {
    if !hours.is_finite() {
        return "--:--".to_string();
    }
    let total = (hours * 60.0 - 1e-6).ceil() as i64;
    let total = total.rem_euclid(24 * 60);
    format!("{:02}:{:02}", total / 60, total % 60)
}

fn zone_label(tz: f64) -> String {
    match tz as i64 {
        7 => "WIB".to_string(),
        8 => "WITA".to_string(),
        9 => "WIT".to_string(),
        z => format!("UTC{z:+}"),
    }
}

/// The card for a prayer: `minutes_left` > 0 when it is an early heads-up.
pub fn card(index: usize, hours: f64, tz: f64, place: &str, minutes_left: i64) -> ReminderCard {
    let name = NAMES[index.min(4)];
    let title = if minutes_left > 0 { format!("{minutes_left} menit lagi {name}") } else { format!("Waktu {name}") };
    let mut body = format!("{} {}", hhmm(hours), zone_label(tz));
    if !place.is_empty() {
        body.push_str(&format!(" · {place}"));
    }
    ReminderCard { kind: RemKind::Prayer, title, body }
}

// ── What has been announced today, kept across restarts ──────────────────────

fn fired_file() -> std::path::PathBuf {
    crate::settings::local_dir().join("prayer.day")
}

fn fired_mask(day: i64) -> u8 {
    let s = std::fs::read_to_string(fired_file()).unwrap_or_default();
    let mut it = s.split_whitespace();
    match (it.next().and_then(|d| d.parse::<i64>().ok()), it.next().and_then(|m| m.parse::<u8>().ok())) {
        (Some(d), Some(m)) if d == day => m,
        _ => 0,
    }
}

fn mark_fired(day: i64, mask: u8) {
    let _ = std::fs::create_dir_all(crate::settings::local_dir());
    let _ = std::fs::write(fired_file(), format!("{day} {mask}"));
}

/// Today's times for the place the weather page uses, if it has one yet:
/// (times, time zone in hours, place name, civil date).
pub fn today(now: f64) -> Option<(Times, f64, String)> {
    let r = crate::weather::report()?;
    let off = util::local_offset_secs();
    let tz = off as f64 / 3600.0;
    let day = ((now + off as f64) / 86_400.0).floor() as i64;
    let (y, m, d) = crate::coach::civil_from_days(day);
    Some((compute(y, m, d, r.lat, r.lon, tz), tz, r.place))
}

impl App {
    /// Called every half minute: announces a prayer when its time (or the early heads-up) comes.
    pub fn prayer_tick(&mut self, now: f64) -> bool {
        if !self.st.settings.prayer_alarm {
            return false;
        }
        if let Some((at, card)) = self.prayer_again.clone() {
            if now >= at {
                self.prayer_again = None;
                self.show_prayer(card);
                return true;
            }
        }
        let Some((times, tz, place)) = today(now) else { return false };
        let off = util::local_offset_secs();
        let day = ((now + off as f64) / 86_400.0).floor() as i64;
        let day_start = day as f64 * 86_400.0 - off as f64;
        let lead = self.st.settings.prayer_lead_min as f64 * 60.0;
        let mut fired = fired_mask(day);
        for (i, h) in times.five().iter().enumerate() {
            if !h.is_finite() || self.st.settings.prayer_mask & (1 << i) == 0 || fired & (1 << i) != 0 {
                continue;
            }
            let at = day_start + h * 3600.0;
            if now >= at - lead && now < at + GRACE {
                fired |= 1 << i;
                mark_fired(day, fired);
                let left = ((at - now) / 60.0).ceil() as i64;
                self.show_prayer(card(i, *h, tz, &place, left.max(0)));
                return true;
            }
        }
        false
    }

    fn show_prayer(&mut self, card: ReminderCard) {
        self.reminder = Some(card);
        sound::play("greet");
        self.engine.trigger_emote(Emote::Happy, 3.0);
        self.alert(View::Reminder);
        self.ensure_running();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minutes(h: f64) -> i64 {
        (h * 60.0).round() as i64
    }

    #[test]
    fn jakarta_in_october_matches_a_hand_calculation() {
        // Jakarta, 21 October 2026, WIB, worked out by hand from the same angles:
        // noon = 12:00 − (106.85−105)×4 min − 15.3 min (equation of time) = 11:37, then
        //  Subuh (20°) 04:10, Ashar (shadow = object + noon shadow) 14:48, sunset 17:46,
        //  Isya (18°) 18:56, each plus 2 minutes of ihtiyat.
        let t = compute(2026, 10, 21, -6.2088, 106.8456, 7.0);
        let five = t.five();
        let want = [(4, 12), (11, 40), (14, 50), (17, 48), (18, 58)];
        for (i, (h, m)) in want.iter().enumerate() {
            let diff = (minutes(five[i]) - (h * 60 + m)).abs();
            assert!(diff <= 3, "{} {} (wanted {h:02}:{m:02})", NAMES[i], hhmm(five[i]));
        }
    }

    #[test]
    fn surabaya_matches_kemenags_published_times_for_the_day() {
        // Aladhan's Kemenag RI method for Surabaya on 21 October 2026 (no ihtiyat in it):
        // Fajr 03:46, Dhuhr 11:14, Asr 14:23, Maghrib 17:23, Isha 18:34. Ours add 2 minutes.
        let five = compute(2026, 10, 21, -7.2575, 112.7521, 7.0).five();
        let want = [(3, 48), (11, 16), (14, 25), (17, 25), (18, 36)];
        for (i, (h, m)) in want.iter().enumerate() {
            let diff = (minutes(five[i]) - (h * 60 + m)).abs();
            assert!(diff <= 1, "{} {} (wanted {h:02}:{m:02})", NAMES[i], hhmm(five[i]));
        }
    }

    #[test]
    fn the_five_prayers_come_in_order_whatever_the_place_and_day() {
        for (lat, lon, tz) in [(-7.2575, 112.7521, 7.0), (-6.9175, 107.6191, 7.0), (3.5952, 98.6722, 7.0), (-8.65, 115.2167, 8.0), (-2.5, 140.7, 9.0)] {
            for (m, d) in [(1, 15), (3, 21), (6, 21), (9, 23), (12, 21)] {
                let t = compute(2026, m, d, lat, lon, tz);
                let v = [t.fajr, t.sunrise, t.dhuhr, t.asr, t.maghrib, t.isha];
                assert!(v.windows(2).all(|w| w[0] < w[1]), "{lat},{lon} {m}/{d}: {v:?}");
            }
        }
    }

    #[test]
    fn times_print_rounded_up_to_the_minute() {
        assert_eq!(hhmm(11.0 + 13.0 / 60.0), "11:13");
        assert_eq!(hhmm(11.0 + 13.2 / 60.0), "11:14");
        assert_eq!(hhmm(f64::NAN), "--:--");
        assert_eq!(hhmm(24.5), "00:30");
    }

    #[test]
    fn the_card_says_what_and_when() {
        let c = card(1, 11.0 + 15.0 / 60.0, 7.0, "Surabaya", 0);
        assert_eq!((c.title.as_str(), c.body.as_str()), ("Waktu Dzuhur", "11:15 WIB · Surabaya"));
        assert_eq!(card(4, 18.5, 8.0, "", 10).title, "10 menit lagi Isya");
        assert_eq!(card(4, 18.5, 8.0, "", 10).body, "18:30 WITA");
    }
}
