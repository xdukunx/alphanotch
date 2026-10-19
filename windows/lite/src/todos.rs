// The dashboard's to-do list: local, plain JSON in %APPDATA%\Coucou\todos.json.
// Starred items float to the top. Completing an item removes it.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Todo {
    pub text: String,
    #[serde(default)]
    pub star: bool,
    /// Google Tasks id once the item is synced.
    #[serde(default)]
    pub gid: Option<String>,
    /// When to tap the user on the shoulder (unix seconds), set with "…14:30" in the text.
    #[serde(default)]
    pub due: Option<i64>,
    /// The reminder for `due` has been shown.
    #[serde(default)]
    pub notified: bool,
}

/// A time at the end of a new task: "Rapat 14:30", "Kirim laporan jam 9", "Telepon @16.15",
/// "… pukul 8:05". Returns the text without it, and (hour, minute).
pub fn split_time(text: &str) -> (String, Option<(u32, u32)>) {
    let t = text.trim();
    let words: Vec<&str> = t.split_whitespace().collect();
    let Some(last) = words.last() else { return (t.to_string(), None) };
    let raw = last.trim_start_matches('@');
    let parsed = {
        let parts: Vec<&str> = raw.split(|c| c == ':' || c == '.').collect();
        match parts.as_slice() {
            [h, m] if h.len() <= 2 && m.len() == 2 => h.parse::<u32>().ok().zip(m.parse::<u32>().ok()),
            [h] if h.len() <= 2 && words.len() >= 2 && matches!(words[words.len() - 2].to_lowercase().as_str(), "jam" | "pukul") => h.parse::<u32>().ok().map(|h| (h, 0)),
            _ => None,
        }
    };
    let Some((h, m)) = parsed.filter(|(h, m)| *h < 24 && *m < 60) else { return (t.to_string(), None) };
    let marker = last.starts_with('@') || raw.contains(':') || raw.contains('.') || words.len() >= 2;
    if !marker {
        return (t.to_string(), None);
    }
    let mut keep = words.len() - 1;
    if keep > 0 && matches!(words[keep - 1].to_lowercase().as_str(), "jam" | "pukul") {
        keep -= 1;
    }
    let rest = words[..keep].join(" ");
    if rest.is_empty() {
        return (t.to_string(), None);
    }
    (rest, Some((h, m)))
}

/// The next moment the local clock shows `h:m` (today, or tomorrow if it has passed).
pub fn next_at(now: f64, offset: i64, h: u32, m: u32) -> i64 {
    let local = now as i64 + offset;
    let day = local.div_euclid(86_400) * 86_400;
    let mut at = day + (h * 3600 + m * 60) as i64;
    if at <= local {
        at += 86_400;
    }
    at - offset
}

/// "14:30" in local time.
pub fn hhmm(at: i64, offset: i64) -> String {
    let s = (at + offset).rem_euclid(86_400);
    format!("{:02}:{:02}", s / 3600, (s % 3600) / 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_at_the_end_is_taken_out() {
        assert_eq!(split_time("Rapat tim 14:30"), ("Rapat tim".to_string(), Some((14, 30))));
        assert_eq!(split_time("Kirim laporan jam 9"), ("Kirim laporan".to_string(), Some((9, 0))));
        assert_eq!(split_time("Telepon ibu @16.15"), ("Telepon ibu".to_string(), Some((16, 15))));
        assert_eq!(split_time("Bayar pukul 8:05"), ("Bayar".to_string(), Some((8, 5))));
        // Not a time: left alone.
        assert_eq!(split_time("Beli 2 kopi").1, None);
        assert_eq!(split_time("Revisi bab 3.2").1, None);
        assert_eq!(split_time("14:30").1, None);
        assert_eq!(split_time("Rapat 25:00").1, None);
    }

    #[test]
    fn the_next_time_is_today_or_tomorrow() {
        // offset 0, now = day 10 at 12:00.
        let now = 10.0 * 86_400.0 + 12.0 * 3600.0;
        assert_eq!(next_at(now, 0, 14, 30), 10 * 86_400 + 14 * 3600 + 1800);
        assert_eq!(next_at(now, 0, 9, 0), 11 * 86_400 + 9 * 3600);
        assert_eq!(hhmm(10 * 86_400 + 14 * 3600 + 1800, 0), "14:30");
    }
}

fn path() -> PathBuf {
    crate::settings::config_dir().join("todos.json")
}

pub fn load() -> Vec<Todo> {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(list: &[Todo]) {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(s) = serde_json::to_string_pretty(list) {
        // Write beside, then rename: a crash never leaves half a list behind.
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}

/// Indices in display order: starred first, otherwise as added.
pub fn order(list: &[Todo]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..list.len()).collect();
    idx.sort_by_key(|&i| !list[i].star);
    idx
}
