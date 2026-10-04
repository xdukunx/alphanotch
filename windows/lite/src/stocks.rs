// Stocks for the dashboard: intraday quotes and charts for the user's watchlist (IHSG first).
//
// Data comes from Yahoo Finance's public chart endpoint: no key, no account, but also no
// guarantee: it is an unofficial endpoint and can change or throttle. Nothing is sent but the
// ticker symbols the user typed. Polled once a minute while the Stocks page is open, every ten
// minutes otherwise, and never while paused.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::Notify;

use crate::log;

#[derive(Clone, Debug)]
pub struct Quote {
    pub symbol: String,
    pub price: f32,
    pub prev: f32,
    /// Intraday closes, oldest first (gaps removed).
    pub series: Vec<f32>,
    pub currency: String,
    pub error: Option<String>,
}

impl Quote {
    pub fn change(&self) -> f32 {
        self.price - self.prev
    }
    pub fn pct(&self) -> f32 {
        if self.prev.abs() < 1e-9 { 0.0 } else { self.change() / self.prev * 100.0 }
    }
}

struct Shared {
    symbols: Vec<String>,
    quotes: Vec<Quote>,
    touched: Option<Instant>,
}

static S: Mutex<Shared> = Mutex::new(Shared { symbols: Vec::new(), quotes: Vec::new(), touched: None });
static KICK: Notify = Notify::const_new();

pub fn quotes() -> Vec<Quote> {
    S.lock().map(|s| s.quotes.clone()).unwrap_or_default()
}

/// The UI calls this every frame the Stocks page is on screen: it keeps the fast poll going.
pub fn touch() {
    if let Ok(mut s) = S.lock() {
        let was_idle = s.touched.map(|t| t.elapsed() > Duration::from_secs(90)).unwrap_or(true);
        s.touched = Some(Instant::now());
        if was_idle {
            KICK.notify_one();
        }
    }
}

pub fn set_symbols(list: &[String]) {
    if let Ok(mut s) = S.lock() {
        if s.symbols != list {
            s.symbols = list.to_vec();
            let keep: Vec<Quote> = s.quotes.iter().filter(|q| list.contains(&q.symbol)).cloned().collect();
            s.quotes = keep;
            KICK.notify_one();
        }
    }
}

/// "bbca" → "BBCA.JK" (Jakarta); "^jkse" stays an index; anything with a dot is left alone.
pub fn normalize(raw: &str) -> Option<String> {
    let t = raw.trim().to_uppercase();
    if t.is_empty() || t.len() > 16 || !t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '^' | '.' | '-' | '=')) {
        return None;
    }
    if t == "IHSG" {
        return Some("^JKSE".into());
    }
    if t.starts_with('^') || t.contains('.') || t.contains('=') || t.contains('-') {
        return Some(t);
    }
    Some(format!("{t}.JK"))
}

/// What the list shows for a symbol.
pub fn display(symbol: &str) -> String {
    if symbol == "^JKSE" {
        return "IHSG".into();
    }
    symbol.strip_suffix(".JK").unwrap_or(symbol).to_string()
}

pub fn start(symbols: &[String]) {
    set_symbols(symbols);
    KICK.notify_one();
    crate::rt::spawn(async {
        loop {
            let fast = S.lock().map(|s| s.touched.map(|t| t.elapsed() < Duration::from_secs(90)).unwrap_or(false)).unwrap_or(false);
            let wait = if fast { 60 } else { 600 };
            let _ = tokio::time::timeout(Duration::from_secs(wait), KICK.notified()).await;
            let symbols = S.lock().map(|s| s.symbols.clone()).unwrap_or_default();
            if symbols.is_empty() {
                continue;
            }
            let mut out = Vec::new();
            for sym in &symbols {
                match fetch(sym).await {
                    Ok(q) => out.push(q),
                    Err(e) => {
                        log::line(format!("stocks {sym}: {e}"));
                        // Keep the last good quote and mark it, rather than dropping the row.
                        let old = S.lock().ok().and_then(|s| s.quotes.iter().find(|q| &q.symbol == sym).cloned());
                        out.push(match old {
                            Some(mut q) => {
                                q.error = Some(e);
                                q
                            }
                            None => Quote { symbol: sym.clone(), price: 0.0, prev: 0.0, series: Vec::new(), currency: String::new(), error: Some(e) },
                        });
                    }
                }
            }
            if let Ok(mut s) = S.lock() {
                // The list may have changed while we were fetching.
                out.retain(|q| s.symbols.contains(&q.symbol));
                s.quotes = out;
            }
        }
    });
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

async fn fetch(symbol: &str) -> Result<Quote, String> {
    let url = format!("https://query1.finance.yahoo.com/v8/finance/chart/{}?range=1d&interval=5m", enc(symbol));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AlphaNotch")
        .build()
        .map_err(|e| e.to_string())?;
    let r = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("HTTP {}", r.status().as_u16()));
    }
    let v: Value = r.json().await.map_err(|e| e.to_string())?;
    parse(symbol, &v)
}

fn parse(symbol: &str, v: &Value) -> Result<Quote, String> {
    let res = v["chart"]["result"].get(0).ok_or_else(|| v["chart"]["error"]["description"].as_str().unwrap_or("no data").to_string())?;
    let meta = &res["meta"];
    let price = meta["regularMarketPrice"].as_f64().ok_or("no price")? as f32;
    let prev = meta["chartPreviousClose"].as_f64().or_else(|| meta["previousClose"].as_f64()).unwrap_or(price as f64) as f32;
    let series: Vec<f32> = res["indicators"]["quote"][0]["close"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect())
        .unwrap_or_default();
    Ok(Quote {
        symbol: symbol.to_string(),
        price,
        prev,
        series,
        currency: meta["currency"].as_str().unwrap_or("").to_string(),
        error: None,
    })
}

/// 7234.56 → "7,234.56" (grouping with commas, two decimals under 1000 or for indices).
pub fn fmt_price(p: f32) -> String {
    let dec = if p >= 1000.0 && p < 100_000.0 && p.fract().abs() > 0.004 { 2 } else if p >= 1000.0 { 0 } else { 2 };
    let s = format!("{p:.dec$}");
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i.to_string(), format!(".{f}")),
        None => (s, String::new()),
    };
    let mut out = String::new();
    for (i, c) in int.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 && c != '-' {
            out.push(',');
        }
        out.push(c);
    }
    format!("{}{frac}", out.chars().rev().collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_tickers() {
        assert_eq!(normalize("bbca").as_deref(), Some("BBCA.JK"));
        assert_eq!(normalize("ihsg").as_deref(), Some("^JKSE"));
        assert_eq!(normalize("^gspc").as_deref(), Some("^GSPC"));
        assert_eq!(normalize("AAPL.US").as_deref(), Some("AAPL.US"));
        assert_eq!(normalize("bad ticker!"), None);
    }

    #[test]
    fn formats_prices() {
        assert_eq!(fmt_price(7234.56), "7,234.56");
        assert_eq!(fmt_price(9825.0), "9,825");
        assert_eq!(fmt_price(65.5), "65.50");
    }

    #[test]
    fn parses_a_chart_payload() {
        let v: Value = serde_json::from_str(r#"{"chart":{"result":[{"meta":{"regularMarketPrice":7250.5,"chartPreviousClose":7200.0,"currency":"IDR"},"indicators":{"quote":[{"close":[7210.0,null,7230.5,7250.5]}]}}]}}"#).unwrap();
        let q = parse("^JKSE", &v).unwrap();
        assert_eq!(q.series.len(), 3);
        assert!((q.pct() - 0.7).abs() < 0.05);
    }
}
