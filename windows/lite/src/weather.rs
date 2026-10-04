// Weather for the dashboard: Open-Meteo, no key and no account.
//
// The only thing sent out is the city name from the settings (once, to find its coordinates) and
// those coordinates (to get the forecast). Refreshed every 20 minutes, and right away when the city changes.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::Notify;

use crate::log;

#[derive(Clone, Debug)]
pub struct Now {
    pub temp: f32,
    pub feels: f32,
    pub humidity: u32,
    pub wind: f32,
    pub code: u32,
    pub is_day: bool,
}

#[derive(Clone, Debug)]
pub struct Day {
    pub weekday: usize, // 0 = Sunday
    pub code: u32,
    pub max: f32,
    pub min: f32,
    pub rain: u32, // chance of precipitation, %
}

#[derive(Clone, Debug)]
pub struct Report {
    pub place: String,
    pub now: Now,
    pub days: Vec<Day>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Clear,
    Partly,
    Cloudy,
    Fog,
    Rain,
    Storm,
    Snow,
}

static REPORT: Mutex<Option<Report>> = Mutex::new(None);
static CITY: Mutex<String> = Mutex::new(String::new());
static KICK: Notify = Notify::const_new();

pub fn report() -> Option<Report> {
    REPORT.lock().ok()?.clone()
}

/// Sets the city (settings) and refreshes when it differs from the current one.
pub fn set_city(city: &str) {
    let city = city.trim();
    if city.is_empty() {
        return;
    }
    if let Ok(mut c) = CITY.lock() {
        if *c != city {
            *c = city.to_string();
            KICK.notify_one();
        }
    }
}

pub fn start(city: &str) {
    set_city(city);
    KICK.notify_one();
    crate::rt::spawn(async {
        let mut coords: Option<(String, f64, f64, String)> = None; // (city asked, lat, lon, display name)
        loop {
            let _ = tokio::time::timeout(Duration::from_secs(20 * 60), KICK.notified()).await;
            let city = CITY.lock().map(|c| c.clone()).unwrap_or_default();
            if city.is_empty() {
                continue;
            }
            if coords.as_ref().map(|c| c.0 != city).unwrap_or(true) {
                match geocode(&city).await {
                    Ok((lat, lon, name)) => coords = Some((city.clone(), lat, lon, name)),
                    Err(e) => {
                        log::line(format!("weather geocode: {e}"));
                        continue;
                    }
                }
            }
            let Some((_, lat, lon, name)) = coords.clone() else { continue };
            match forecast(lat, lon, &name).await {
                Ok(r) => {
                    if let Ok(mut s) = REPORT.lock() {
                        *s = Some(r);
                    }
                }
                Err(e) => log::line(format!("weather forecast: {e}")),
            }
        }
    });
}

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder().timeout(Duration::from_secs(15)).build().map_err(|e| e.to_string())
}

async fn get(url: &str) -> Result<Value, String> {
    let r = http()?.get(url).send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("HTTP {}", r.status().as_u16()));
    }
    r.json().await.map_err(|e| e.to_string())
}

fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

async fn geocode(city: &str) -> Result<(f64, f64, String), String> {
    let v = get(&format!("https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=id", enc(city))).await?;
    let r = v["results"].get(0).ok_or("city not found")?;
    Ok((
        r["latitude"].as_f64().ok_or("no latitude")?,
        r["longitude"].as_f64().ok_or("no longitude")?,
        r["name"].as_str().unwrap_or(city).to_string(),
    ))
}

async fn forecast(lat: f64, lon: f64, place: &str) -> Result<Report, String> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &current=temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,wind_speed_10m,is_day\
         &daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max\
         &timezone=auto&forecast_days=7"
    );
    let v = get(&url).await?;
    let c = &v["current"];
    let now = Now {
        temp: c["temperature_2m"].as_f64().ok_or("no temperature")? as f32,
        feels: c["apparent_temperature"].as_f64().unwrap_or(0.0) as f32,
        humidity: c["relative_humidity_2m"].as_u64().unwrap_or(0) as u32,
        wind: c["wind_speed_10m"].as_f64().unwrap_or(0.0) as f32,
        code: c["weather_code"].as_u64().unwrap_or(0) as u32,
        is_day: c["is_day"].as_u64().unwrap_or(1) == 1,
    };
    let d = &v["daily"];
    let n = d["time"].as_array().map(|a| a.len()).unwrap_or(0);
    let mut days = Vec::new();
    for i in 0..n {
        let date = d["time"][i].as_str().unwrap_or("");
        days.push(Day {
            weekday: weekday_of(date).unwrap_or(0),
            code: d["weather_code"][i].as_u64().unwrap_or(0) as u32,
            max: d["temperature_2m_max"][i].as_f64().unwrap_or(0.0) as f32,
            min: d["temperature_2m_min"][i].as_f64().unwrap_or(0.0) as f32,
            rain: d["precipitation_probability_max"][i].as_u64().unwrap_or(0) as u32,
        });
    }
    Ok(Report { place: place.to_string(), now, days })
}

/// 0 = Sunday … 6 = Saturday for "YYYY-MM-DD" (Sakamoto's method).
pub fn weekday_of(date: &str) -> Option<usize> {
    let mut p = date.split('-');
    let (y, m, d): (i32, usize, i32) = (p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.parse().ok()?);
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    Some(((y + y / 4 - y / 100 + y / 400 + T[m.checked_sub(1)?.min(11)] + d) % 7) as usize)
}

pub fn kind_of(code: u32) -> Kind {
    match code {
        0 => Kind::Clear,
        1 | 2 => Kind::Partly,
        3 => Kind::Cloudy,
        45 | 48 => Kind::Fog,
        51..=67 | 80..=82 => Kind::Rain,
        71..=77 | 85 | 86 => Kind::Snow,
        95..=99 => Kind::Storm,
        _ => Kind::Cloudy,
    }
}

pub fn label_of(code: u32) -> &'static str {
    match code {
        0 => "Cerah",
        1 => "Cerah berawan",
        2 => "Berawan sebagian",
        3 => "Berawan",
        45 | 48 => "Berkabut",
        51..=57 => "Gerimis",
        61..=65 => "Hujan",
        66 | 67 => "Hujan beku",
        71..=77 | 85 | 86 => "Salju",
        80..=82 => "Hujan lokal",
        95..=99 => "Badai petir",
        _ => "Berawan",
    }
}

pub const WEEKDAYS: [&str; 7] = ["Min", "Sen", "Sel", "Rab", "Kam", "Jum", "Sab"];

pub fn _unused(_: Instant) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekday_known_dates() {
        assert_eq!(weekday_of("2026-10-04"), Some(0)); // Sunday
        assert_eq!(weekday_of("2026-10-05"), Some(1)); // Monday
        assert_eq!(weekday_of("2000-01-01"), Some(6)); // Saturday
    }
}
