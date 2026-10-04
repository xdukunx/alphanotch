// Small helpers: ISO-8601 parsing and relative / local time formatting.

use serde_json::Value;
use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `2026-10-02T14:03:09.123Z` (or with a ±hh:mm offset) → unix seconds.
pub fn parse_iso(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    if b.len() < 10 {
        return None;
    }
    let num = |a: usize, n: usize| -> Option<i64> { s.get(a..a + n)?.parse().ok() };
    let (y, mo, d) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (mut h, mut mi, mut sec) = (0, 0, 0.0f64);
    let mut offset = 0i64;
    if b.len() >= 16 {
        h = num(11, 2)?;
        mi = num(14, 2)?;
        let mut i = 16;
        if b.get(i) == Some(&b':') {
            let end = s[i + 1..].find(|c: char| !(c.is_ascii_digit() || c == '.')).map(|e| i + 1 + e).unwrap_or(s.len());
            sec = s[i + 1..end].parse().unwrap_or(0.0);
            i = end;
        }
        if let Some(&c) = b.get(i) {
            if c == b'+' || c == b'-' {
                let oh = num(i + 1, 2).unwrap_or(0);
                let om = num(i + 4, 2).or_else(|| num(i + 3, 2)).unwrap_or(0);
                offset = (oh * 3600 + om * 60) * if c == b'-' { -1 } else { 1 };
            }
        }
    }
    let days = days_from_civil(y, mo, d);
    Some((days * 86_400 + h * 3600 + mi * 60) as f64 + sec - offset as f64)
}

/// A JSON value that is either an ISO string or epoch milliseconds → unix seconds.
pub fn as_epoch(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64().map(|ms| ms / 1000.0),
        Value::String(s) => parse_iso(s),
        _ => None,
    }
}

pub fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Same shape as the Swift `timeAgo` computed properties.
pub fn time_ago(v: &Value) -> String {
    let Some(t) = as_epoch(v) else { return String::new() };
    let diff = unix_now() - t;
    if diff < 60.0 {
        "just now".into()
    } else if diff < 3600.0 {
        format!("{}m", (diff / 60.0) as i64)
    } else if diff < 86_400.0 {
        format!("{}h", (diff / 3600.0) as i64)
    } else {
        format!("{}d", (diff / 86_400.0) as i64)
    }
}

/// Offset of the local time zone from UTC, in seconds (east positive).
pub fn local_offset_secs() -> i64 {
    unsafe {
        let mut tz = TIME_ZONE_INFORMATION::default();
        let id = GetTimeZoneInformation(&mut tz);
        let bias = tz.Bias + if id == 2 { tz.DaylightBias } else { tz.StandardBias };
        -(bias as i64) * 60
    }
}

/// `dd/mm hh:mm` in local time.
pub fn local_day_time(epoch: f64) -> String {
    let t = epoch as i64 + local_offset_secs();
    let days = t.div_euclid(86_400);
    let rem = t.rem_euclid(86_400);
    let (_, m, d) = civil_from_days(days);
    format!("{d:02}/{m:02} {:02}:{:02}", rem / 3600, (rem % 3600) / 60)
}
