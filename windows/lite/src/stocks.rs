// Stocks for the dashboard: intraday quotes and charts for the user's watchlist (IHSG first).
//
// Data comes from Yahoo Finance's public chart endpoint: no key, no account, but also no
// guarantee: it is an unofficial endpoint and can change or throttle. Nothing is sent but the
// ticker symbols the user typed. Polled once a minute while the Stocks page is open, every ten
// minutes otherwise, and never while paused.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::Notify;

use crate::log;

/// One candle: open, high, low, close and volume of a bar that started at `t` (unix seconds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candle {
    pub t: i64,
    pub o: f32,
    pub h: f32,
    pub l: f32,
    pub c: f32,
    pub v: f32,
}

/// (label, Yahoo range, Yahoo bar size) of each chart range.
pub const RANGES: [(&str, &str, &str); 5] = [
    ("1D", "1d", "5m"),
    ("5D", "5d", "15m"),
    ("1M", "1mo", "60m"),
    ("6M", "6mo", "1d"),
    ("1Y", "1y", "1wk"),
];

#[derive(Clone, Debug)]
pub struct Quote {
    pub symbol: String,
    pub price: f32,
    pub prev: f32,
    /// Intraday closes, oldest first (gaps removed).
    pub series: Vec<f32>,
    /// Today's candles (5-minute bars), the same data as `series` with the rest of each bar.
    pub candles: Vec<Candle>,
    /// The exchange's offset from UTC, seconds (25200 for Jakarta).
    pub offset: i32,
    /// When the price was last updated by the exchange (unix seconds), and the day's high,
    /// low and volume as it reports them (0 when it does not).
    pub market_time: i64,
    pub day_high: f32,
    pub day_low: f32,
    pub volume: f32,
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
/// The island is on screen (not hidden, not paused, no game in front): prices are worth fetching.
static ACTIVE: AtomicBool = AtomicBool::new(true);

/// Tells the poller whether anyone could be looking. Coming back from idle fetches at once.
pub fn set_active(on: bool) {
    if ACTIVE.swap(on, Ordering::Relaxed) != on && on {
        KICK.notify_one();
    }
}

/// Seconds until the next refresh. A minute while the market is moving (the free feed is ten
/// minutes behind, so faster only repeats the same numbers and risks being throttled), ten
/// minutes once every price is hours old (night, weekend, holiday), three after a "too many
/// requests".
pub fn next_wait(quotes: &[Quote], now: i64, throttled: bool) -> u64 {
    if throttled {
        return 180;
    }
    let live = quotes.is_empty() || quotes.iter().any(|q| q.market_time > 0 && now - q.market_time < 25 * 60);
    if live { 60 } else { 600 }
}

/// What the chart shows for a symbol and a range, and whether more is on its way.
#[derive(Clone, Debug, Default)]
pub struct Detail {
    pub candles: Vec<Candle>,
    pub offset: i32,
    /// The previous close, for the dashed line (only meaningful on the 1D range).
    pub prev: f32,
    pub loading: bool,
    pub error: Option<String>,
}

struct DetailEntry {
    at: Instant,
    detail: Detail,
}

static DETAILS: Mutex<Option<HashMap<(String, usize), DetailEntry>>> = Mutex::new(None);

/// The candles for `symbol` on range `range` (index into `RANGES`). Starts a fetch when there
/// is nothing yet or the data is a minute old; the answer arrives in a later call.
pub fn detail(symbol: &str, range: usize) -> Detail {
    let range = range.min(RANGES.len() - 1);
    let key = (symbol.to_string(), range);
    let mut start = false;
    let out = {
        let Ok(mut g) = DETAILS.lock() else { return Detail::default() };
        let map = g.get_or_insert_with(HashMap::new);
        match map.get_mut(&key) {
            Some(e) => {
                let stale = e.at.elapsed() > Duration::from_secs(if e.detail.error.is_some() { 20 } else { 60 });
                if stale && !e.detail.loading {
                    e.detail.loading = true;
                    start = true;
                }
                e.detail.clone()
            }
            None => {
                let d = Detail { loading: true, ..Default::default() };
                map.insert(key.clone(), DetailEntry { at: Instant::now(), detail: d.clone() });
                start = true;
                d
            }
        }
    };
    if start {
        crate::rt::spawn(async move {
            let result = fetch_range(&key.0, key.1).await;
            if let Ok(mut g) = DETAILS.lock() {
                let map = g.get_or_insert_with(HashMap::new);
                let old = map.get(&key).map(|e| e.detail.clone()).unwrap_or_default();
                let detail = match result {
                    Ok(d) => d,
                    // Keep what was shown, marked as old.
                    Err(e) => Detail { error: Some(e), loading: false, ..old },
                };
                map.insert(key, DetailEntry { at: Instant::now(), detail });
            }
        });
    }
    out
}

pub fn quotes() -> Vec<Quote> {
    S.lock().map(|s| s.quotes.clone()).unwrap_or_default()
}

/// Snapshot builds only: fixed quotes, so the page can be rendered without a network.
#[cfg(feature = "snapshot")]
pub fn set_demo() {
    let wave = |base: f32, amp: f32, drift: f32| -> Vec<f32> {
        (0..60).map(|i| base + (i as f32 * 0.35).sin() * amp + i as f32 * drift).collect()
    };
    // A day of 5-minute candles around `series`, with a volume that peaks at the open and the close.
    let candles_of = |series: &Vec<f32>| -> Vec<Candle> {
        series
            .iter()
            .enumerate()
            .map(|(i, &c)| {
                let o = if i == 0 { c } else { series[i - 1] };
                let wick = (c - o).abs() * 0.6 + c * 0.0008;
                let edge = ((i as f32 / series.len() as f32) - 0.5).abs() * 2.0;
                Candle { t: 1_792_000_000 + i as i64 * 300, o, h: o.max(c) + wick, l: o.min(c) - wick, c, v: (900.0 + 2200.0 * edge * edge + (i as f32 * 1.7).sin().abs() * 800.0) * 1000.0 }
            })
            .collect()
    };
    let mk = |sym: &str, price: f32, prev: f32, series: Vec<f32>| Quote {
        symbol: sym.into(), price, prev, candles: candles_of(&series), series, offset: 25_200, market_time: 1_792_000_000 + 59 * 300, day_high: price * 1.004, day_low: price * 0.994, volume: 293_260_200.0, currency: "IDR".into(), error: None,
    };
    // Longer ranges too, falling, so the header's change over a range can be told from today's.
    if let Ok(mut g) = DETAILS.lock() {
        let map = g.get_or_insert_with(HashMap::new);
        for sym in ["^JKSE", "BBCA.JK", "BBRI.JK", "BMRI.JK", "TLKM.JK"] {
            for range in 1..RANGES.len() {
                let n = [90usize, 150, 120, 54][range - 1];
                let top = 7_800.0f32;
                let candles: Vec<Candle> = (0..n)
                    .map(|i| {
                        let c = top - (top - 7_234.5) * i as f32 / n as f32 + (i as f32 * 0.9).sin() * 60.0;
                        let o = c + (i as f32 * 1.7).sin() * 25.0;
                        Candle { t: 1_790_000_000 + i as i64 * 3600, o, h: o.max(c) + 30.0, l: o.min(c) - 30.0, c, v: 1.0e6 + (i as f32 * 0.7).sin().abs() * 2.0e6 }
                    })
                    .collect();
                map.insert((sym.to_string(), range), DetailEntry { at: Instant::now(), detail: Detail { candles, offset: 25_200, prev: top, loading: false, error: None } });
            }
        }
    }
    if let Ok(mut s) = S.lock() {
        s.quotes = vec![
            mk("^JKSE", 7234.5, 7190.2, wave(7190.0, 18.0, 0.7)),
            mk("BBCA.JK", 9850.0, 9925.0, wave(9925.0, 40.0, -1.3)),
            mk("BBRI.JK", 4320.0, 4280.0, wave(4280.0, 25.0, 0.7)),
            mk("BMRI.JK", 5775.0, 5800.0, wave(5800.0, 30.0, -0.4)),
            mk("TLKM.JK", 3010.0, 2960.0, wave(2960.0, 15.0, 0.8)),
        ];
    }
}

/// Snapshot builds only: one more row, for a code the data source does not know.
#[cfg(feature = "snapshot")]
pub fn set_demo_missing() {
    if let Ok(mut s) = S.lock() {
        s.quotes.push(Quote { symbol: "ZZZZ.JK".into(), price: 0.0, prev: 0.0, series: Vec::new(), candles: Vec::new(), offset: 0, market_time: 0, day_high: 0.0, day_low: 0.0, volume: 0.0, currency: String::new(), error: Some("HTTP 404".into()) });
    }
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
    let mut t = raw.trim().to_uppercase();
    // "$AAPL" means exactly AAPL (a US or other ticker), without ".JK" added.
    if let Some(rest) = t.strip_prefix('$') {
        let rest = rest.trim().to_string();
        let ok = !rest.is_empty() && rest.len() <= 16 && rest.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '^' | '.' | '-' | '='));
        return ok.then_some(rest);
    }
    t = t.trim().to_string();
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

/// The data source's way of saying it does not know the symbol (a 404, or its own wording).
pub fn is_not_found(error: &str) -> bool {
    let e = error.to_lowercase();
    e.contains("404") || e.contains("not found") || e.contains("no data") || e.contains("delisted") || e.contains("no price")
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
        let mut wait = 60u64;
        loop {
            let _ = tokio::time::timeout(Duration::from_secs(wait), KICK.notified()).await;
            // Nobody can see the page (island hidden, paused, a game in front): do not fetch.
            if !ACTIVE.load(Ordering::Relaxed) {
                wait = 600;
                continue;
            }
            let symbols = S.lock().map(|s| s.symbols.clone()).unwrap_or_default();
            if symbols.is_empty() {
                wait = 60;
                continue;
            }
            let mut out = Vec::new();
            let mut throttled = false;
            for sym in &symbols {
                match fetch(sym).await {
                    Ok(q) => out.push(q),
                    Err(e) => {
                        throttled |= e.contains("429");
                        log::line(format!("stocks {sym}: {e}"));
                        // Keep the last good quote and mark it, rather than dropping the row.
                        let old = S.lock().ok().and_then(|s| s.quotes.iter().find(|q| &q.symbol == sym).cloned());
                        out.push(match old {
                            Some(mut q) => {
                                q.error = Some(e);
                                q
                            }
                            None => Quote { symbol: sym.clone(), price: 0.0, prev: 0.0, series: Vec::new(), candles: Vec::new(), offset: 0, market_time: 0, day_high: 0.0, day_low: 0.0, volume: 0.0, currency: String::new(), error: Some(e) },
                        });
                    }
                }
            }
            if let Ok(mut s) = S.lock() {
                // The list may have changed while we were fetching.
                out.retain(|q| s.symbols.contains(&q.symbol));
                s.quotes = out;
            }
            let (quotes, now) = (quotes(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0));
            wait = next_wait(&quotes, now, throttled);
            if std::env::var_os("COUCOU_DEBUG_STOCKS").is_some() {
                log::line(format!("stocks: {} refreshed, next in {wait} s (throttled: {throttled})", quotes.len()));
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

/// Yahoo's parallel arrays (`timestamp`, `open`, `high`, `low`, `close`, `volume`) as candles.
/// A bar without a close is dropped; a missing open, high or low falls back on the close.
pub fn parse_candles(res: &Value) -> Vec<Candle> {
    let q = &res["indicators"]["quote"][0];
    let stamps = res["timestamp"].as_array().cloned().unwrap_or_default();
    let col = |k: &str| q[k].as_array().cloned().unwrap_or_default();
    let (open, high, low, close, vol) = (col("open"), col("high"), col("low"), col("close"), col("volume"));
    let mut out = Vec::with_capacity(stamps.len());
    for (i, t) in stamps.iter().enumerate() {
        let Some(t) = t.as_i64() else { continue };
        let Some(c) = close.get(i).and_then(|x| x.as_f64()) else { continue };
        let c = c as f32;
        let get = |a: &Vec<Value>| a.get(i).and_then(|x| x.as_f64()).map(|x| x as f32);
        let (o, h, l) = (get(&open).unwrap_or(c), get(&high).unwrap_or(c), get(&low).unwrap_or(c));
        out.push(Candle { t, o, h: h.max(o).max(c), l: l.min(o).min(c), c, v: get(&vol).unwrap_or(0.0).max(0.0) });
    }
    out
}

async fn fetch_range(symbol: &str, range: usize) -> Result<Detail, String> {
    let (_, r, interval) = RANGES[range.min(RANGES.len() - 1)];
    let url = format!("https://query1.finance.yahoo.com/v8/finance/chart/{}?range={r}&interval={interval}", enc(symbol));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AlphaNotch")
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    let res = v["chart"]["result"].get(0).ok_or_else(|| v["chart"]["error"]["description"].as_str().unwrap_or("no data").to_string())?;
    let meta = &res["meta"];
    Ok(Detail {
        candles: parse_candles(res),
        offset: meta["gmtoffset"].as_i64().unwrap_or(0) as i32,
        prev: meta["chartPreviousClose"].as_f64().or_else(|| meta["previousClose"].as_f64()).unwrap_or(0.0) as f32,
        loading: false,
        error: None,
    })
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
        candles: parse_candles(res),
        offset: meta["gmtoffset"].as_i64().unwrap_or(0) as i32,
        market_time: meta["regularMarketTime"].as_i64().unwrap_or(0),
        day_high: meta["regularMarketDayHigh"].as_f64().unwrap_or(0.0) as f32,
        day_low: meta["regularMarketDayLow"].as_f64().unwrap_or(0.0) as f32,
        volume: meta["regularMarketVolume"].as_f64().unwrap_or(0.0) as f32,
        currency: meta["currency"].as_str().unwrap_or("").to_string(),
        error: None,
    })
}

/// How old the price is, as the header says it: "Tertunda 10 mnt · 11:49" while the market is
/// open (every free IDX feed is ten minutes behind), "Penutupan · Rab 15:50" once it is not.
/// `offset` is the exchange's offset from UTC.
pub fn freshness(market_time: i64, now: i64, offset: i32) -> Option<String> {
    if market_time <= 0 {
        return None;
    }
    let lag = (now - market_time).max(0);
    let clock = |with_day: bool| {
        const DAYS: [&str; 7] = ["Kam", "Jum", "Sab", "Min", "Sen", "Sel", "Rab"];
        let local = market_time + offset as i64;
        let days = local.div_euclid(86_400);
        let secs = local.rem_euclid(86_400);
        let hm = format!("{:02}:{:02}", secs / 3600, (secs % 3600) / 60);
        if with_day { format!("{} {hm}", DAYS[days.rem_euclid(7) as usize]) } else { hm }
    };
    Some(if lag < 20 * 60 {
        format!("Tertunda {} mnt · {}", (lag as f32 / 60.0).round() as i64, clock(false))
    } else if lag < 6 * 3600 {
        format!("Data {}", clock(false))
    } else {
        format!("Penutupan · {}", clock(true))
    })
}

/// 12_345_678 → "12.3M", 845_000 → "845K".
pub fn fmt_volume(v: f32) -> String {
    if v >= 1.0e9 {
        format!("{:.1}B", v / 1.0e9)
    } else if v >= 1.0e6 {
        format!("{:.1}M", v / 1.0e6)
    } else if v >= 1.0e3 {
        format!("{:.0}K", v / 1.0e3)
    } else {
        format!("{v:.0}")
    }
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
        // "$" leaves the code as typed: a US ticker, not a Jakarta one.
        assert_eq!(normalize("$aapl").as_deref(), Some("AAPL"));
        assert_eq!(normalize("$ msft").as_deref(), Some("MSFT"));
        assert_eq!(normalize("$"), None);
    }

    #[test]
    fn the_refresh_is_a_minute_while_the_market_moves_and_slower_otherwise() {
        let q = |t: i64| Quote { symbol: "X".into(), price: 1.0, prev: 1.0, series: Vec::new(), candles: Vec::new(), offset: 0, market_time: t, day_high: 0.0, day_low: 0.0, volume: 0.0, currency: String::new(), error: None };
        let now = 1_792_000_000;
        assert_eq!(next_wait(&[], now, false), 60); // first fetch
        assert_eq!(next_wait(&[q(now - 601)], now, false), 60); // ten minutes behind is normal
        assert_eq!(next_wait(&[q(now - 3 * 3600), q(now - 120)], now, false), 60); // one live is enough
        assert_eq!(next_wait(&[q(now - 8 * 3600)], now, false), 600); // closed
        assert_eq!(next_wait(&[q(now - 60)], now, true), 180); // told to slow down
    }

    #[test]
    fn a_missing_ticker_is_recognised_by_what_the_source_says() {
        assert!(is_not_found("HTTP 404"));
        assert!(is_not_found("No data found, symbol may be delisted"));
        assert!(is_not_found("no price"));
        assert!(!is_not_found("HTTP 429"));
        assert!(!is_not_found("error sending request: timed out"));
    }

    #[test]
    fn formats_prices() {
        assert_eq!(fmt_price(7234.56), "7,234.56");
        assert_eq!(fmt_price(9825.0), "9,825");
        assert_eq!(fmt_price(65.5), "65.50");
    }

    /// Manual: `cargo test -p coucou-lite candles_from_yahoo -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn candles_from_yahoo() {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        for sym in ["BBCA.JK", "^JKSE"] {
            for (i, (label, _, _)) in RANGES.iter().enumerate() {
                match rt.block_on(fetch_range(sym, i)) {
                    Ok(d) => {
                        let (first, last) = (d.candles.first(), d.candles.last());
                        println!("{sym} {label}: {} candles, offset {}, prev {}, first {:?}, last {:?}", d.candles.len(), d.offset, d.prev, first.map(|c| (c.t, c.c)), last.map(|c| (c.t, c.c, c.v)));
                    }
                    Err(e) => println!("{sym} {label}: ERROR {e}"),
                }
            }
        }
    }

    #[test]
    fn parses_candles_and_fills_what_is_missing() {
        let v: Value = serde_json::from_str(r#"{"chart":{"result":[{"meta":{"regularMarketPrice":10.0,"gmtoffset":25200,"regularMarketTime":1792558171,"regularMarketDayHigh":10.8,"regularMarketDayLow":8.8,"regularMarketVolume":1800},
            "timestamp":[100,200,300,400],
            "indicators":{"quote":[{"open":[9.0,null,10.0,10.5],"high":[9.5,10.0,10.8,10.6],"low":[8.8,9.5,9.9,10.2],
            "close":[9.4,9.9,null,10.4],"volume":[1000,null,300,500]}]}}]}}"#).unwrap();
        let q = parse("X", &v).unwrap();
        // The bar without a close is dropped.
        assert_eq!(q.candles.len(), 3);
        assert_eq!(q.offset, 25_200);
        assert_eq!((q.market_time, q.day_high, q.day_low, q.volume), (1_792_558_171, 10.8, 8.8, 1800.0));
        assert_eq!(q.candles[0], Candle { t: 100, o: 9.0, h: 9.5, l: 8.8, c: 9.4, v: 1000.0 });
        // No open: the close stands in; no volume: zero.
        assert_eq!((q.candles[1].o, q.candles[1].v), (9.9, 0.0));
        // A high or low that the open or close would break is widened.
        assert!(q.candles.iter().all(|c| c.h >= c.o.max(c.c) && c.l <= c.o.min(c.c)));
    }

    #[test]
    fn the_header_says_how_old_the_price_is() {
        // 11:49 Jakarta on a Wednesday is 04:49 UTC; 1_792_558_171 s is that moment.
        let t = 1_792_558_171;
        assert_eq!(freshness(t, t + 601, 25_200).as_deref(), Some("Tertunda 10 mnt · 11:49"));
        assert_eq!(freshness(t, t + 2 * 3600, 25_200).as_deref(), Some("Data 11:49"));
        assert_eq!(freshness(t, t + 9 * 3600, 25_200).as_deref(), Some("Penutupan · Rab 11:49"));
        assert_eq!(freshness(0, t, 25_200), None);
    }

    #[test]
    fn volumes_and_ranges_read_well() {
        assert_eq!(fmt_volume(845_000.0), "845K");
        assert_eq!(fmt_volume(12_345_678.0), "12.3M");
        assert_eq!(fmt_volume(2.5e9), "2.5B");
        assert_eq!(RANGES.len(), 5);
        assert!(RANGES.iter().all(|r| !r.1.is_empty() && !r.2.is_empty()));
    }

    #[test]
    fn parses_a_chart_payload() {
        let v: Value = serde_json::from_str(r#"{"chart":{"result":[{"meta":{"regularMarketPrice":7250.5,"chartPreviousClose":7200.0,"currency":"IDR"},"indicators":{"quote":[{"close":[7210.0,null,7230.5,7250.5]}]}}]}}"#).unwrap();
        let q = parse("^JKSE", &v).unwrap();
        assert_eq!(q.series.len(), 3);
        assert!((q.pct() - 0.7).abs() < 0.05);
    }
}
