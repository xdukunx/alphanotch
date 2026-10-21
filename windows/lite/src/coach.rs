// Work suggestions and Claude-usage alerts.
//
// Everything here is local and an estimate, said as such on the cards:
//  * Usage comes from Claude Code's own transcripts (~/.claude/projects/*/*.jsonl),
//    which carry the token usage and a timestamp of every reply. Nothing is sent
//    anywhere and no credential is read. Claude does not expose the plan's limit, so
//    "how much is left" is measured against the user's own busiest 5-hour stretch of
//    the last 30 days.
//  * Suggestions come from Google Tasks (overdue, due today, old tasks without a
//    deadline), the agenda (a free gap before the next event) and from which projects
//    were worked on and then left alone.
//
// The scan is incremental (a byte offset per file) and runs on its own thread every
// five minutes, only while the user has one of the two features on.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

const WINDOW_HOURS: i64 = 5;
const HISTORY_DAYS: i64 = 30;
/// Below this much history there is no "usual" to compare with, so no alert.
const MIN_PEAK: f64 = 150_000.0;
const SCAN_EVERY: f64 = 270.0;

#[derive(Default)]
struct Usage {
    /// unix hour → tokens (input + output + cache writes; cache reads are nearly free)
    hours: HashMap<i64, f64>,
    offsets: HashMap<PathBuf, u64>,
    seen: HashSet<String>,
    /// project folder name → last activity, unix seconds
    projects: HashMap<String, f64>,
    last_scan: f64,
    scanning: bool,
}

static U: Mutex<Option<Usage>> = Mutex::new(None);

fn with_usage<R>(f: impl FnOnce(&mut Usage) -> R) -> R {
    let mut g = U.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(Usage::default))
}

// ── Time ──────────────────────────────────────────────────────────────────────

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The inverse: days since the epoch → (year, month, day).
pub(crate) fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

/// Local calendar date, `YYYY-MM-DD`.
pub fn local_date(now: f64) -> String {
    let days = ((now + crate::util::local_offset_secs() as f64) / 86_400.0).floor() as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `2026-10-02T13:00:45.360Z` (a trailing offset is honoured) → unix seconds.
pub fn parse_rfc3339(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || (b[10] != b'T' && b[10] != b' ') {
        return None;
    }
    let n = |a: usize, z: usize| s.get(a..z)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0, 4)?, n(5, 7)?, n(8, 10)?, n(11, 13)?, n(14, 16)?, n(17, 19)?);
    let mut t = (days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se) as f64;
    let rest = &s[19..];
    let rest = if let Some(frac) = rest.strip_prefix('.') {
        let end = frac.find(|c: char| !c.is_ascii_digit()).unwrap_or(frac.len());
        t += format!("0.{}", &frac[..end]).parse::<f64>().unwrap_or(0.0);
        &frac[end..]
    } else {
        rest
    };
    if let Some(sign) = rest.chars().next().filter(|c| *c == '+' || *c == '-') {
        let (oh, om) = (rest.get(1..3)?.parse::<i64>().ok()?, rest.get(4..6)?.parse::<i64>().ok()?);
        let off = (oh * 3600 + om * 60) as f64;
        t -= if sign == '+' { off } else { -off };
    }
    Some(t)
}

// ── Scanning ──────────────────────────────────────────────────────────────────

fn root() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(".claude")
        .join("projects")
}

fn transcripts() -> Vec<PathBuf> {
    let cutoff = SystemTime::now() - std::time::Duration::from_secs((HISTORY_DAYS as u64 + 5) * 86_400);
    let mut out = Vec::new();
    let Ok(dirs) = std::fs::read_dir(root()) else { return out };
    for d in dirs.flatten() {
        let Ok(files) = std::fs::read_dir(d.path()) else { continue };
        for f in files.flatten() {
            let p = f.path();
            if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if f.metadata().and_then(|m| m.modified()).map(|m| m >= cutoff).unwrap_or(false) {
                out.push(p);
            }
        }
    }
    out
}

struct Parsed {
    hour: i64,
    tokens: f64,
    id: Option<String>,
}

/// What one transcript line contributes: a usage record, and/or a project touch.
fn read_line(line: &str, uses: &mut Vec<Parsed>, touches: &mut Vec<(String, f64)>) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { return };
    let Some(ts) = v.get("timestamp").and_then(|t| t.as_str()).and_then(parse_rfc3339) else { return };
    if let Some(cwd) = v.get("cwd").and_then(|c| c.as_str()) {
        let name = cwd.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().unwrap_or("");
        if !name.is_empty() {
            touches.push((name.to_string(), ts));
        }
    }
    if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
        return;
    }
    let Some(msg) = v.get("message") else { return };
    let Some(u) = msg.get("usage") else { return };
    let get = |k: &str| u.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
    let tokens = get("input_tokens") + get("output_tokens") + get("cache_creation_input_tokens");
    if tokens <= 0.0 {
        return;
    }
    uses.push(Parsed {
        hour: (ts / 3600.0).floor() as i64,
        tokens,
        id: msg.get("id").and_then(|i| i.as_str()).map(str::to_string),
    });
}

fn scan_blocking() {
    let mut new_uses: Vec<Parsed> = Vec::new();
    let mut touches: Vec<(String, f64)> = Vec::new();
    let mut new_offsets: Vec<(PathBuf, u64)> = Vec::new();
    for path in transcripts() {
        let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let mut off = with_usage(|u| u.offsets.get(&path).copied().unwrap_or(0));
        if len < off {
            off = 0; // rewritten or truncated: start over
        }
        if len == off {
            continue;
        }
        let Ok(mut f) = std::fs::File::open(&path) else { continue };
        if f.seek(SeekFrom::Start(off)).is_err() {
            continue;
        }
        let mut r = BufReader::new(f);
        let mut buf = Vec::new();
        let mut consumed = off;
        loop {
            buf.clear();
            match r.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if buf.last() != Some(&b'\n') {
                        break; // a line still being written: pick it up next time
                    }
                    consumed += n as u64;
                    if let Ok(line) = std::str::from_utf8(&buf) {
                        read_line(line, &mut new_uses, &mut touches);
                    }
                }
            }
        }
        new_offsets.push((path, consumed));
    }
    with_usage(|u| {
        for p in new_uses {
            if let Some(id) = &p.id {
                if !u.seen.insert(id.clone()) {
                    continue; // the same reply, written once per content block
                }
            }
            *u.hours.entry(p.hour).or_insert(0.0) += p.tokens;
        }
        if u.seen.len() > 300_000 {
            u.seen.clear();
        }
        for (name, ts) in touches {
            let e = u.projects.entry(name).or_insert(0.0);
            if ts > *e {
                *e = ts;
            }
        }
        for (p, o) in new_offsets {
            u.offsets.insert(p, o);
        }
        let now_h = (crate::util::unix_now() / 3600.0).floor() as i64;
        u.hours.retain(|h, _| *h > now_h - HISTORY_DAYS * 24 - 1);
        u.last_scan = crate::util::unix_now();
        u.scanning = false;
    });
}

/// Starts a background scan if the last one is old enough.
pub fn kick_scan() {
    let now = crate::util::unix_now();
    let go = with_usage(|u| {
        if u.scanning || now - u.last_scan < SCAN_EVERY {
            false
        } else {
            u.scanning = true;
            true
        }
    });
    if go {
        let _ = std::thread::Builder::new().name("coach-scan".into()).spawn(scan_blocking);
    }
}

/// A scan has completed at least once.
pub fn ready() -> bool {
    with_usage(|u| u.last_scan > 0.0)
}

// ── Plan limits, as Claude Code reports them ──────────────────────────────────

/// One window of the plan: used share (0–100) and when it resets (unix seconds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub pct: f32,
    pub resets: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plan {
    pub five: Option<Window>,
    pub week: Option<Window>,
    pub at: f64,
}

static PLAN: Mutex<Option<Plan>> = Mutex::new(None);

fn window_of(v: Option<&serde_json::Value>) -> Option<Window> {
    let v = v?;
    let pct = v.get("used_percentage")?.as_f64()? as f32;
    let resets = v.get("resets_at")?.as_f64()?;
    (pct >= 0.0 && resets > 0.0 && resets < 1.0e11).then_some(Window { pct: pct.min(100.0), resets })
}

pub fn parse_plan(limits: &serde_json::Value, now: f64) -> Option<Plan> {
    let five = window_of(limits.get("five_hour"));
    let week = window_of(limits.get("seven_day"));
    (five.is_some() || week.is_some()).then_some(Plan { five, week, at: now })
}

pub fn set_plan(p: Plan) {
    if let Ok(mut s) = PLAN.lock() {
        *s = Some(p);
    }
}

/// The last reading, if it is less than six hours old.
pub fn plan(now: f64) -> Option<Plan> {
    PLAN.lock().ok()?.filter(|p| now - p.at < 6.0 * 3600.0)
}

/// A window's share now: zero once it has reset.
pub fn effective(w: Window, now: f64) -> f32 {
    if w.resets <= now { 0.0 } else { w.pct }
}

/// With real limits: (alert, minutes until the 5-hour window resets).
pub fn official_alert(now: f64, local_hour: i64) -> Option<(LimitAlert, i64)> {
    let p = plan(now)?;
    let w = p.five?;
    let pct = effective(w, now);
    let mins = ((w.resets - now) / 60.0).round() as i64;
    if w.resets <= now {
        return None;
    }
    if pct >= 80.0 && mins > 30 {
        return Some((LimitAlert::Fast { pct: pct.round() as u32 }, mins));
    }
    if pct <= 40.0 && mins <= 60 && (8..22).contains(&local_hour) {
        return Some((LimitAlert::Plenty { pct: pct.round() as u32 }, mins));
    }
    None
}

// ── Limit estimate ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LimitAlert {
    /// The last five hours are a small part of the usual peak: room to use more.
    Plenty { pct: u32 },
    /// Close to the usual peak, or burning fast.
    Fast { pct: u32 },
}

fn window_sum(h: &HashMap<i64, f64>, end_hour: i64) -> f64 {
    (end_hour - WINDOW_HOURS + 1..=end_hour).map(|x| h.get(&x).copied().unwrap_or(0.0)).sum()
}

/// The pure part: decides from hourly buckets, so it can be tested.
pub fn judge(h: &HashMap<i64, f64>, now_h: i64, local_hour: i64) -> Option<LimitAlert> {
    let peak = (now_h - HISTORY_DAYS * 24..=now_h).map(|e| window_sum(h, e)).fold(0.0, f64::max);
    if peak < MIN_PEAK {
        return None;
    }
    let cur = window_sum(h, now_h);
    let pct = (cur / peak * 100.0).round() as u32;
    let last_hour = h.get(&now_h).copied().unwrap_or(0.0);
    if pct >= 85 || last_hour / peak >= 0.45 {
        return Some(LimitAlert::Fast { pct: pct.max((last_hour / peak * 100.0).round() as u32) });
    }
    // "Use it": only in waking hours, and only if there has been any activity in the last
    // three days (a holiday is not a reason to nag).
    let recent: f64 = (now_h - 72..=now_h).map(|x| h.get(&x).copied().unwrap_or(0.0)).sum();
    if pct <= 25 && recent > 0.0 && (8..22).contains(&local_hour) {
        return Some(LimitAlert::Plenty { pct });
    }
    None
}

pub fn limit_alert(now: f64) -> Option<LimitAlert> {
    let now_h = (now / 3600.0).floor() as i64;
    let local_hour = ((now + crate::util::local_offset_secs() as f64) / 3600.0).floor() as i64 % 24;
    with_usage(|u| judge(&u.hours, now_h, local_hour))
}

// ── Suggestions ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub struct TaskInfo {
    pub title: String,
    /// `YYYY-MM-DD`, when the task has a deadline.
    pub due: Option<String>,
    pub updated: f64,
}

#[derive(Clone, Debug)]
pub struct EventInfo {
    pub title: String,
    pub start_min: i32,
    pub start: String,
}

fn trim_title(s: &str) -> String {
    let t: String = s.chars().take(46).collect();
    if s.chars().count() > 46 { format!("{t}…") } else { t }
}

/// The pure part of the suggestions. `today` is `YYYY-MM-DD`; at most three lines.
pub fn build_suggestions(
    now: f64,
    today: &str,
    now_min: i32,
    tasks: &[TaskInfo],
    events: &[EventInfo],
    projects: &[(String, f64)],
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen_titles: HashSet<&str> = HashSet::new();
    let push = |s: String, out: &mut Vec<String>| {
        if out.len() < 3 {
            out.push(s);
        }
    };

    // 1. Past due, then due today.
    for t in tasks.iter().filter(|t| t.due.as_deref().map(|d| d < today).unwrap_or(false)).take(2) {
        seen_titles.insert(&t.title);
        push(format!("Telat: {}", trim_title(&t.title)), &mut out);
    }
    for t in tasks.iter().filter(|t| t.due.as_deref() == Some(today)).take(2) {
        seen_titles.insert(&t.title);
        push(format!("Jatuh tempo hari ini: {}", trim_title(&t.title)), &mut out);
    }

    // 2. The oldest task that never got a deadline and has not been touched for two weeks.
    let stale = tasks
        .iter()
        .filter(|t| t.due.is_none() && t.updated > 0.0 && now - t.updated > 14.0 * 86_400.0 && !seen_titles.contains(t.title.as_str()))
        .min_by(|a, b| a.updated.partial_cmp(&b.updated).unwrap_or(std::cmp::Ordering::Equal));
    if let Some(t) = stale {
        let days = ((now - t.updated) / 86_400.0).floor() as i64;
        push(format!("Sudah {days} hari belum disentuh: {}", trim_title(&t.title)), &mut out);
    }

    // 3. A free stretch before the next event, with something to put in it.
    if let Some(next) = events.iter().filter(|e| e.start_min > now_min).min_by_key(|e| e.start_min) {
        let gap = next.start_min - now_min;
        if gap >= 60 {
            let what = tasks.first().map(|t| format!(" — cocok untuk: {}", trim_title(&t.title))).unwrap_or_default();
            push(format!("Kosong ~{gap} menit sebelum {} ({}){what}", trim_title(&next.title), next.start), &mut out);
        }
    }

    // 4. A project worked on lately, then left alone.
    let mut quiet: Vec<&(String, f64)> = projects
        .iter()
        .filter(|(_, ts)| now - ts > 3.0 * 86_400.0 && now - ts < 21.0 * 86_400.0)
        .collect();
    quiet.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    if let Some((name, ts)) = quiet.first() {
        let days = ((now - ts) / 86_400.0).floor() as i64;
        push(format!("Proyek “{name}” terakhir dikerjakan {days} hari lalu"), &mut out);
    }
    out
}

fn minutes_of(s: &str) -> Option<i32> {
    let (h, m) = s.split_once(':')?;
    Some(h.trim().parse::<i32>().ok()? * 60 + m.trim().parse::<i32>().ok()?)
}

/// Suggestions from the live data (tasks, agenda, project history).
pub fn suggestions(now: f64) -> Vec<String> {
    let today = local_date(now);
    let off = crate::util::local_offset_secs() as f64;
    let now_min = (((now + off) % 86_400.0) / 60.0) as i32;
    let tasks: Vec<TaskInfo> = crate::gtasks::task_meta()
        .into_iter()
        .map(|m| TaskInfo {
            title: m.title,
            due: m.due.as_deref().map(|d| d.chars().take(10).collect()),
            updated: parse_rfc3339(&m.updated).unwrap_or(0.0),
        })
        .collect();
    let events: Vec<EventInfo> = crate::gtasks::events()
        .into_iter()
        .filter_map(|e| Some(EventInfo { start_min: minutes_of(&e.start)?, start: e.start, title: e.title }))
        .collect();
    let projects: Vec<(String, f64)> = with_usage(|u| u.projects.iter().map(|(k, v)| (k.clone(), *v)).collect());
    build_suggestions(now, &today, now_min, &tasks, &events, &projects)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Manual: `cargo test -p coucou-lite report_on_this_machine -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn report_on_this_machine() {
        let t0 = std::time::Instant::now();
        scan_blocking();
        let first = t0.elapsed();
        let t1 = std::time::Instant::now();
        scan_blocking();
        let again = t1.elapsed();
        let now = crate::util::unix_now();
        let now_h = (now / 3600.0).floor() as i64;
        with_usage(|u| {
            let peak = (now_h - HISTORY_DAYS * 24..=now_h).map(|e| window_sum(&u.hours, e)).fold(0.0, f64::max);
            let cur = window_sum(&u.hours, now_h);
            println!("scan: first {first:?}, again {again:?}; files {}, hours {}", u.offsets.len(), u.hours.len());
            println!("peak5h {peak:.0}  cur5h {cur:.0}  ({:.0}%)", cur / peak.max(1.0) * 100.0);
            println!("projects: {}", u.projects.len());
        });
        println!("alert: {:?}", limit_alert(now));
        println!("suggestions (tasks/agenda not loaded in a test): {:?}", suggestions(now));
    }

    #[test]
    fn civil_dates_round_trip() {
        for days in [0i64, 59, 60, 365, 11_000, 20_000, 20_729] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
        assert_eq!(civil_from_days(20_728), (2026, 10, 2));
    }

    #[test]
    fn timestamps_parse_with_fraction_and_offset() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(parse_rfc3339("2026-10-02T13:00:45Z"), Some(1_790_946_045.0));
        let f = parse_rfc3339("2026-10-02T13:00:45.500Z").unwrap();
        assert!((f - 1_790_946_045.5).abs() < 1e-6);
        assert_eq!(parse_rfc3339("2026-10-02T20:00:45+07:00"), Some(1_790_946_045.0));
        assert_eq!(parse_rfc3339("nonsense"), None);
    }

    fn buckets(spec: &[(i64, f64)]) -> HashMap<i64, f64> {
        spec.iter().copied().collect()
    }

    #[test]
    fn real_limits_parse_and_drive_the_alerts() {
        let now = 1_000_000.0;
        let l = serde_json::json!({ "five_hour": { "used_percentage": 86, "resets_at": now + 7200.0 }, "seven_day": { "used_percentage": 54, "resets_at": now + 86_400.0 } });
        let p = parse_plan(&l, now).unwrap();
        assert_eq!(p.five.unwrap().pct, 86.0);
        set_plan(p);
        assert!(matches!(official_alert(now, 14), Some((LimitAlert::Fast { .. }, 120))));
        set_plan(parse_plan(&serde_json::json!({ "five_hour": { "used_percentage": 20, "resets_at": now + 1800.0 } }), now).unwrap());
        assert!(matches!(official_alert(now, 14), Some((LimitAlert::Plenty { pct: 20 }, 30))));
        assert!(official_alert(now, 3).is_none());
        // A window that has reset counts as empty.
        assert_eq!(effective(Window { pct: 90.0, resets: now - 1.0 }, now), 0.0);
        assert!(parse_plan(&serde_json::json!({}), now).is_none());
    }

    #[test]
    fn no_history_means_no_opinion() {
        assert_eq!(judge(&buckets(&[(1000, 10_000.0)]), 1000, 12), None);
    }

    #[test]
    fn a_light_stretch_after_a_heavy_peak_says_there_is_room() {
        // A peak of 5 × 100k six days ago, almost nothing in the last five hours.
        let now_h = 1_000_000;
        let mut v: Vec<(i64, f64)> = (0..5).map(|i| (now_h - 140 + i, 100_000.0)).collect();
        v.push((now_h - 30, 5_000.0));
        v.push((now_h - 1, 10_000.0));
        match judge(&buckets(&v), now_h, 14) {
            Some(LimitAlert::Plenty { pct }) => assert!(pct <= 5),
            other => panic!("{other:?}"),
        }
        // …but not in the middle of the night.
        assert_eq!(judge(&buckets(&v), now_h, 3), None);
    }

    #[test]
    fn near_the_peak_or_burning_fast_says_careful() {
        let now_h = 1_000_000;
        let mut v: Vec<(i64, f64)> = (0..5).map(|i| (now_h - 140 + i, 100_000.0)).collect();
        // Now: 5 hours at 90k each → 90% of the peak.
        v.extend((0..5).map(|i| (now_h - i, 90_000.0)));
        assert!(matches!(judge(&buckets(&v), now_h, 14), Some(LimitAlert::Fast { .. })));
        // One enormous hour alone also counts as fast.
        let mut w: Vec<(i64, f64)> = (0..5).map(|i| (now_h - 140 + i, 100_000.0)).collect();
        w.push((now_h, 300_000.0));
        assert!(matches!(judge(&buckets(&w), now_h, 14), Some(LimitAlert::Fast { .. })));
    }

    #[test]
    fn suggestions_are_ordered_and_capped_at_three() {
        let now = 1_790_946_045.0;
        let day = 86_400.0;
        let tasks = vec![
            TaskInfo { title: "Kirim laporan".into(), due: Some("2026-10-01".into()), updated: now - day },
            TaskInfo { title: "Bayar hosting".into(), due: Some("2026-10-02".into()), updated: now - day },
            TaskInfo { title: "Rapikan arsip".into(), due: None, updated: now - 40.0 * day },
        ];
        let events = vec![EventInfo { title: "Rapat tim".into(), start_min: 14 * 60, start: "14:00".into() }];
        let projects = vec![("coucou".to_string(), now - 6.0 * day)];
        let s = build_suggestions(now, "2026-10-02", 10 * 60, &tasks, &events, &projects);
        assert_eq!(s.len(), 3);
        assert!(s[0].starts_with("Telat: Kirim laporan"));
        assert!(s[1].starts_with("Jatuh tempo hari ini: Bayar hosting"));
        assert!(s[2].starts_with("Sudah 40 hari belum disentuh: Rapikan arsip"));
    }

    #[test]
    fn an_old_task_without_a_deadline_is_reminded_and_a_recent_one_is_not() {
        let now = 1_790_946_045.0;
        let day = 86_400.0;
        let tasks = vec![
            TaskInfo { title: "Baru".into(), due: None, updated: now - 2.0 * day },
            TaskInfo { title: "Lama".into(), due: None, updated: now - 30.0 * day },
        ];
        let s = build_suggestions(now, "2026-10-02", 600, &tasks, &[], &[]);
        assert_eq!(s, vec!["Sudah 30 hari belum disentuh: Lama".to_string()]);
    }

    #[test]
    fn a_free_gap_and_a_quiet_project_show_up_when_nothing_else_does() {
        let now = 1_790_946_045.0;
        let events = vec![EventInfo { title: "Kuliah".into(), start_min: 13 * 60, start: "13:00".into() }];
        let projects = vec![("alphanotch".to_string(), now - 5.0 * 86_400.0)];
        let s = build_suggestions(now, "2026-10-02", 10 * 60, &[], &events, &projects);
        assert!(s[0].starts_with("Kosong ~180 menit sebelum Kuliah (13:00)"));
        assert!(s[1].contains("alphanotch") && s[1].contains("5 hari"));
    }

    #[test]
    fn a_transcript_line_is_counted_once_per_reply() {
        let line = r#"{"type":"assistant","timestamp":"2026-10-02T13:00:45.360Z","cwd":"C:\\p\\coucou","message":{"id":"m1","usage":{"input_tokens":2,"output_tokens":300,"cache_creation_input_tokens":1000,"cache_read_input_tokens":50000}}}"#;
        let (mut uses, mut touches) = (Vec::new(), Vec::new());
        read_line(line, &mut uses, &mut touches);
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].tokens, 1302.0); // cache reads are not counted
        assert_eq!(touches[0].0, "coucou");
        let user = r#"{"type":"user","timestamp":"2026-10-02T13:00:45Z","cwd":"C:\\p\\x"}"#;
        let (mut u2, mut t2) = (Vec::new(), Vec::new());
        read_line(user, &mut u2, &mut t2);
        assert!(u2.is_empty() && t2.len() == 1);
    }
}
