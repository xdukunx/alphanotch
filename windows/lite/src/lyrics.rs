// Synced lyrics for the song that is playing, from LRCLIB (lrclib.net): a free, open lyrics
// database. Opt-in by use: nothing is requested until the lyrics panel is opened, and then
// only the title, the artist and the length of the current song leave the computer.
// Results are kept in memory for the session.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Loading,
    /// Lines with timestamps: the active one can be highlighted.
    Synced,
    /// Lines without timestamps.
    Plain,
    NotFound,
    Failed,
}

#[derive(Clone, Debug)]
pub struct Lyrics {
    pub key: String,
    pub state: State,
    /// (seconds, text); every time is -1 for plain lyrics.
    pub lines: Vec<(f32, String)>,
}

static CURRENT: Mutex<Option<Lyrics>> = Mutex::new(None);
static CACHE: Mutex<Vec<Lyrics>> = Mutex::new(Vec::new());

pub fn current() -> Option<Lyrics> {
    CURRENT.lock().ok()?.clone()
}

fn key_of(title: &str, artist: &str) -> String {
    format!("{}|{}", title.trim().to_lowercase(), artist.trim().to_lowercase())
}

/// Snapshot builds only: lyrics around the demo song's position.
#[cfg(feature = "snapshot")]
pub fn set_demo() {
    let lines = ["If I could be by your side", "I'd give you all my seasons and every quiet morning in between", "Every summer, every rain", "Every dawn that finds you again", "Paper sky above our heads"];
    let lines: Vec<(f32, String)> = lines.iter().enumerate().map(|(i, l)| (50.0 + i as f32 * 9.0, l.to_string())).collect();
    if let Ok(mut c) = CURRENT.lock() {
        *c = Some(Lyrics { key: key_of("Paper Sky", "Lumen Coast"), state: State::Synced, lines });
    }
}

/// Starts fetching lyrics for this song unless they are already there or on their way.
pub fn request(title: &str, artist: &str, duration: f32) {
    let key = key_of(title, artist);
    if title.trim().is_empty() {
        return;
    }
    if let Ok(mut cur) = CURRENT.lock() {
        if cur.as_ref().map(|c| c.key == key).unwrap_or(false) {
            return;
        }
        if let Some(hit) = CACHE.lock().ok().and_then(|c| c.iter().find(|l| l.key == key).cloned()) {
            *cur = Some(hit);
            return;
        }
        *cur = Some(Lyrics { key: key.clone(), state: State::Loading, lines: Vec::new() });
    }
    let (title, artist) = (title.to_string(), artist.to_string());
    crate::rt::spawn(async move {
        let result = fetch(&title, &artist, duration).await;
        let lyrics = match result {
            Ok(Some((state, lines))) => Lyrics { key: key.clone(), state, lines },
            Ok(None) => Lyrics { key: key.clone(), state: State::NotFound, lines: Vec::new() },
            Err(_) => Lyrics { key: key.clone(), state: State::Failed, lines: Vec::new() },
        };
        // Only a failure is retried next time; the rest is remembered for the session.
        if lyrics.state != State::Failed {
            if let Ok(mut c) = CACHE.lock() {
                c.retain(|l| l.key != key);
                c.push(lyrics.clone());
                if c.len() > 30 {
                    c.remove(0);
                }
            }
        }
        if let Ok(mut cur) = CURRENT.lock() {
            // The song may have changed while we waited.
            if cur.as_ref().map(|c| c.key == key).unwrap_or(false) {
                *cur = Some(lyrics);
            }
        }
    });
}

/// Forgets a failed attempt so the next request tries again.
pub fn retry_if_failed() {
    if let Ok(mut cur) = CURRENT.lock() {
        if cur.as_ref().map(|c| c.state == State::Failed).unwrap_or(false) {
            *cur = None;
        }
    }
}

async fn fetch(title: &str, artist: &str, duration: f32) -> Result<Option<(State, Vec<(f32, String)>)>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent("AlphaNotch (lyrics for the notch)")
        .build()
        .map_err(|e| e.to_string())?;

    // An exact match on title, artist and length first.
    let mut q = vec![("track_name", title.to_string()), ("artist_name", artist.to_string())];
    if duration > 1.0 {
        q.push(("duration", format!("{}", duration.round() as i64)));
    }
    let r = client.get("https://lrclib.net/api/get").query(&q).send().await.map_err(|e| e.to_string())?;
    if r.status().is_success() {
        let v: Value = r.json().await.map_err(|e| e.to_string())?;
        if let Some(found) = from_record(&v) {
            return Ok(Some(found));
        }
    }
    // Then a search, with the noise ("(feat. …)", "- Remastered") taken out of the title.
    let query = format!("{} {}", clean_title(title), artist);
    let r = client.get("https://lrclib.net/api/search").query(&[("q", query)]).send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Ok(None);
    }
    let v: Value = r.json().await.map_err(|e| e.to_string())?;
    let list = v.as_array().cloned().unwrap_or_default();
    // Synced first, and the closest in length.
    let mut best: Option<(f32, (State, Vec<(f32, String)>))> = None;
    for rec in &list {
        let Some(found) = from_record(rec) else { continue };
        let d = rec["duration"].as_f64().unwrap_or(0.0) as f32;
        let dist = if duration > 1.0 && d > 0.0 { (d - duration).abs() } else { 0.0 };
        let rank = dist + if found.0 == State::Synced { 0.0 } else { 1000.0 };
        if best.as_ref().map(|b| rank < b.0).unwrap_or(true) {
            best = Some((rank, found));
        }
    }
    Ok(best.map(|b| b.1))
}

fn from_record(v: &Value) -> Option<(State, Vec<(f32, String)>)> {
    if let Some(s) = v["syncedLyrics"].as_str().filter(|s| !s.trim().is_empty()) {
        let lines = parse_lrc(s);
        if !lines.is_empty() {
            return Some((State::Synced, lines));
        }
    }
    let plain = v["plainLyrics"].as_str().filter(|s| !s.trim().is_empty())?;
    let lines: Vec<(f32, String)> = plain.lines().map(|l| (-1.0, l.trim().to_string())).collect();
    (!lines.is_empty()).then_some((State::Plain, lines))
}

/// "Song (feat. X) - Remastered 2011" → "Song".
pub fn clean_title(t: &str) -> String {
    let mut s = t.to_string();
    for open in ['(', '['] {
        if let Some(i) = s.find(open) {
            s.truncate(i);
        }
    }
    if let Some(i) = s.find(" - ") {
        s.truncate(i);
    }
    s.trim().to_string()
}

/// LRC: `[01:23.45] text`, several stamps per line allowed, `[ar:…]` tags skipped.
pub fn parse_lrc(s: &str) -> Vec<(f32, String)> {
    let mut out: Vec<(f32, String)> = Vec::new();
    for line in s.lines() {
        let mut rest = line.trim();
        let mut stamps: Vec<f32> = Vec::new();
        while let Some(r) = rest.strip_prefix('[') {
            let Some(end) = r.find(']') else { break };
            let tag = &r[..end];
            if let Some(t) = stamp(tag) {
                stamps.push(t);
            } else if stamps.is_empty() {
                // [ar:Artist], [ti:Title]…: metadata, not a lyric line.
                rest = "";
                break;
            }
            rest = r[end + 1..].trim_start();
        }
        let text = rest.trim().to_string();
        for t in stamps {
            out.push((t, text.clone()));
        }
    }
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    out
}

fn stamp(tag: &str) -> Option<f32> {
    let (m, s) = tag.split_once(':')?;
    let m: f32 = m.trim().parse().ok()?;
    let s: f32 = s.trim().parse().ok()?;
    Some(m * 60.0 + s)
}

/// The line being sung at `pos` seconds: the last one that has started.
pub fn active_index(lines: &[(f32, String)], pos: f32) -> Option<usize> {
    let n = lines.partition_point(|l| l.0 <= pos);
    n.checked_sub(1)
}

#[allow(dead_code)]
fn _unused(_: HashMap<u8, u8>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_lines_and_several_stamps_parse() {
        let l = parse_lrc("[ar:Someone]\n[ti:Song]\n[00:12.50] First\n[00:20.00][01:05.25]Chorus\n[00:30.00]\n");
        assert_eq!(l.len(), 4);
        assert_eq!(l[0], (12.5, "First".to_string()));
        assert_eq!(l[1], (20.0, "Chorus".to_string()));
        assert_eq!(l[2], (30.0, String::new()));
        assert_eq!(l[3], (65.25, "Chorus".to_string()));
    }

    #[test]
    fn the_active_line_is_the_last_one_started() {
        let l = parse_lrc("[00:10.00]a\n[00:20.00]b\n[00:30.00]c");
        assert_eq!(active_index(&l, 5.0), None);
        assert_eq!(active_index(&l, 10.0), Some(0));
        assert_eq!(active_index(&l, 25.0), Some(1));
        assert_eq!(active_index(&l, 99.0), Some(2));
    }

    #[test]
    fn title_noise_is_removed_for_the_search() {
        assert_eq!(clean_title("Paper Sky (feat. Mia)"), "Paper Sky");
        assert_eq!(clean_title("Summer Flows - Remastered 2011"), "Summer Flows");
        assert_eq!(clean_title("Plain"), "Plain");
    }

    #[test]
    fn a_record_prefers_synced_and_falls_back_to_plain() {
        let v: Value = serde_json::from_str(r#"{"syncedLyrics":"[00:01.00]hi","plainLyrics":"hi"}"#).unwrap();
        assert_eq!(from_record(&v).unwrap().0, State::Synced);
        let p: Value = serde_json::from_str(r#"{"syncedLyrics":null,"plainLyrics":"line one\nline two"}"#).unwrap();
        let (st, lines) = from_record(&p).unwrap();
        assert_eq!((st, lines.len()), (State::Plain, 2));
        let none: Value = serde_json::from_str(r#"{"syncedLyrics":null,"plainLyrics":null}"#).unwrap();
        assert!(from_record(&none).is_none());
    }
}
