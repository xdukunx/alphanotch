// Google Tasks sync for the dashboard's to-do card, plus today's Google Calendar agenda.
//
// - The OAuth client (a Desktop client the user created in their own Google Cloud project) is
//   imported from the `client_secret_*.json` they downloaded, and lives in the Credential Manager.
// - Sign-in is the loopback flow with PKCE: the browser opens Google's page, Google redirects to
//   127.0.0.1 on a random port, and the app trades the code for a refresh token.
// - Sync: tasks are pulled from the user's first list; adds and completions made in the island
//   are pushed. Nothing here talks to anyone but Google, and nothing happens until the user
//   clicks "connect" on the card.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Notify;
use windows::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};

use crate::{log, secrets};

const SCOPE: &str = "https://www.googleapis.com/auth/tasks https://www.googleapis.com/auth/calendar.readonly";
const KEY_CLIENT: &str = "google-client";
const KEY_REFRESH: &str = "google-refresh";
const API: &str = "https://tasks.googleapis.com/tasks/v1";

#[derive(Clone, PartialEq, Debug)]
pub enum Status {
    NoClient,
    NeedsLogin,
    LoggingIn,
    Linked,
    Error(String),
}

enum Op {
    Add(String),
    Complete(String),
}

/// One agenda entry for today.
#[derive(Clone, Debug)]
pub struct Event {
    /// "14:00", or "Seharian" for an all-day event.
    pub start: String,
    /// "15:30" when the event has an end time today.
    pub end: Option<String>,
    pub title: String,
}

struct Shared {
    events: Vec<Event>,
    /// The signed-in token predates the calendar scope: a new sign-in is needed.
    cal_denied: bool,
    status: Status,
    /// (task id, title) of every open task, as of the last pull. Taken once by the UI.
    remote: Option<Vec<(String, String)>>,
    ops: Vec<Op>,
    access: Option<(String, Instant)>,
    list: Option<String>,
}

static S: Mutex<Shared> = Mutex::new(Shared { events: Vec::new(), cal_denied: false, status: Status::NoClient, remote: None, ops: Vec::new(), access: None, list: None });
static KICK: Notify = Notify::const_new();

fn set_status(st: Status) {
    if let Ok(mut s) = S.lock() {
        s.status = st;
    }
}

pub fn status() -> Status {
    S.lock().map(|s| s.status.clone()).unwrap_or(Status::NoClient)
}

pub fn events() -> Vec<Event> {
    S.lock().map(|s| s.events.clone()).unwrap_or_default()
}

pub fn calendar_needs_login() -> bool {
    S.lock().map(|s| s.cal_denied).unwrap_or(false)
}

pub fn take_remote() -> Option<Vec<(String, String)>> {
    S.lock().ok()?.remote.take()
}

pub fn enqueue_add(text: &str) {
    if status() == Status::Linked {
        if let Ok(mut s) = S.lock() {
            s.ops.push(Op::Add(text.to_string()));
        }
        KICK.notify_one();
    }
}

pub fn enqueue_complete(gid: &str) {
    if let Ok(mut s) = S.lock() {
        s.ops.push(Op::Complete(gid.to_string()));
    }
    KICK.notify_one();
}

fn refresh_status() {
    let st = if !secrets::present(KEY_CLIENT) {
        Status::NoClient
    } else if !secrets::present(KEY_REFRESH) {
        Status::NeedsLogin
    } else {
        Status::Linked
    };
    set_status(st);
}

pub fn start() {
    refresh_status();
    // One permit up front: the first sync runs right away instead of after 90 s.
    KICK.notify_one();
    crate::rt::spawn(async {
        loop {
            let _ = tokio::time::timeout(Duration::from_secs(90), KICK.notified()).await;
            if status() != Status::Linked && !matches!(status(), Status::Error(_)) {
                continue;
            }
            if !secrets::present(KEY_REFRESH) {
                continue;
            }
            match sync_once().await {
                Ok(()) => set_status(Status::Linked),
                Err(e) => {
                    log::line(format!("google tasks: {e}"));
                    if e == "invalid_grant" {
                        let _ = secrets::clear(KEY_REFRESH);
                        set_status(Status::NeedsLogin);
                    } else {
                        set_status(Status::Error(e));
                    }
                }
            }
        }
    });
}

/// What a click on the card's status line does.
pub fn connect() {
    log::line(format!("google tasks: connect clicked, status {:?}", status()));
    match status() {
        Status::NoClient => import_client(),
        Status::NeedsLogin => {
            set_status(Status::LoggingIn);
            crate::rt::spawn(async {
                match login().await {
                    Ok(()) => {
                        set_status(Status::Linked);
                        KICK.notify_one();
                    }
                    Err(e) => {
                        log::line(format!("google login: {e}"));
                        refresh_status();
                    }
                }
            });
        }
        Status::Linked | Status::Error(_) => {
            if calendar_needs_login() {
                // The old token has no calendar permission: sign in again to grant it.
                let _ = secrets::clear(KEY_REFRESH);
                if let Ok(mut s) = S.lock() {
                    s.access = None;
                    s.cal_denied = false;
                }
                set_status(Status::NeedsLogin);
                connect();
                return;
            }
            if secrets::present(KEY_REFRESH) {
                set_status(Status::Linked);
                KICK.notify_one();
            } else {
                refresh_status();
            }
        }
        Status::LoggingIn => {}
    }
}

/// Reads the newest `client_secret*.json` in Downloads into the Credential Manager,
/// then deletes the file so the secret does not sit in a plain file.
fn import_client() {
    let Some(dir) = std::env::var_os("USERPROFILE").map(|p| std::path::PathBuf::from(p).join("Downloads")) else { return };
    let mut best: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().to_lowercase();
        if name.starts_with("client_secret") && name.ends_with(".json") {
            let t = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            if best.as_ref().map(|(b, _)| t > *b).unwrap_or(true) {
                best = Some((t, e.path()));
            }
        }
    }
    let Some((_, path)) = best else {
        log::line("google import: no client_secret*.json in Downloads");
        return;
    };
    let parsed = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
    let creds = parsed.as_ref().and_then(|v| v.get("installed")).and_then(|i| {
        Some((i.get("client_id")?.as_str()?.to_string(), i.get("client_secret")?.as_str()?.to_string()))
    });
    match creds {
        Some((id, secret)) => {
            match secrets::set(KEY_CLIENT, &json!({ "id": id, "secret": secret }).to_string()) {
                Ok(()) => {
                    let _ = std::fs::remove_file(&path);
                    log::line("google import: client stored in the Credential Manager, file removed");
                }
                Err(e) => log::line(format!("google import: could not store the client: {e}")),
            }
        }
        None => log::line("google import: the file is not a Desktop OAuth client"),
    }
    refresh_status();
}

fn client() -> Result<(String, String), String> {
    let raw = secrets::get(KEY_CLIENT).ok_or("no client")?;
    let v: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    Ok((v["id"].as_str().unwrap_or_default().to_string(), v["secret"].as_str().unwrap_or_default().to_string()))
}

// ── OAuth ────────────────────────────────────────────────────────────────────

async fn login() -> Result<(), String> {
    let (id, secret) = client()?;
    let verifier = b64url(&random(32));
    let challenge = b64url(&sha256(verifier.as_bytes()));
    let state = b64url(&random(16));

    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}");
    let url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256&state={}&access_type=offline&prompt=consent",
        enc(&id), enc(&redirect), enc(SCOPE), challenge, state
    );
    crate::views::open_url(&url);

    let (mut sock, _) = tokio::time::timeout(Duration::from_secs(240), listener.accept())
        .await
        .map_err(|_| "login timed out".to_string())?
        .map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 4096];
    let n = tokio::time::timeout(Duration::from_secs(10), sock.read(&mut buf))
        .await
        .map_err(|_| "no request".to_string())?
        .map_err(|e| e.to_string())?;
    let req = String::from_utf8_lossy(&buf[..n]).to_string();
    let query = req.lines().next().and_then(|l| l.split_whitespace().nth(1)).and_then(|p| p.split_once('?')).map(|(_, q)| q.to_string()).unwrap_or_default();
    let param = |k: &str| query.split('&').find_map(|kv| kv.split_once('=').filter(|(a, _)| *a == k).map(|(_, v)| dec(v)));

    let ok = param("state").as_deref() == Some(state.as_str()) && param("code").is_some();
    let page = if ok {
        "<html><body style=\"font:16px system-ui;background:#111;color:#eee;display:grid;place-items:center;height:100vh\"><div>AlphaNotch is connected to Google Tasks. You can close this tab.</div></body></html>"
    } else {
        "<html><body style=\"font:16px system-ui;background:#111;color:#eee;display:grid;place-items:center;height:100vh\"><div>Sign-in was cancelled. You can close this tab.</div></body></html>"
    };
    let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", page.len(), page);
    let _ = sock.write_all(resp.as_bytes()).await;
    let _ = sock.shutdown().await;
    if !ok {
        return Err("cancelled".into());
    }

    let code = param("code").unwrap_or_default();
    let body = format!(
        "client_id={}&client_secret={}&code={}&code_verifier={}&grant_type=authorization_code&redirect_uri={}",
        enc(&id), enc(&secret), enc(&code), enc(&verifier), enc(&redirect)
    );
    let v = post_form("https://oauth2.googleapis.com/token", body).await?;
    let refresh = v["refresh_token"].as_str().ok_or("no refresh token")?;
    secrets::set(KEY_REFRESH, refresh)?;
    if let (Some(tok), Some(exp)) = (v["access_token"].as_str(), v["expires_in"].as_u64()) {
        if let Ok(mut s) = S.lock() {
            s.access = Some((tok.to_string(), Instant::now() + Duration::from_secs(exp.saturating_sub(60))));
        }
    }
    Ok(())
}

async fn post_form(url: &str, body: String) -> Result<Value, String> {
    let r = http()?
        .post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let v: Value = r.json().await.map_err(|e| e.to_string())?;
    if let Some(e) = v["error"].as_str() {
        return Err(e.to_string());
    }
    Ok(v)
}

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder().timeout(Duration::from_secs(15)).build().map_err(|e| e.to_string())
}

async fn access_token() -> Result<String, String> {
    if let Some((t, until)) = S.lock().ok().and_then(|s| s.access.clone()) {
        if Instant::now() < until {
            return Ok(t);
        }
    }
    let (id, secret) = client()?;
    let refresh = secrets::get(KEY_REFRESH).ok_or("no refresh token")?;
    let body = format!("client_id={}&client_secret={}&refresh_token={}&grant_type=refresh_token", enc(&id), enc(&secret), enc(&refresh));
    let v = post_form("https://oauth2.googleapis.com/token", body).await?;
    let tok = v["access_token"].as_str().ok_or("no access token")?.to_string();
    let exp = v["expires_in"].as_u64().unwrap_or(3000);
    if let Ok(mut s) = S.lock() {
        s.access = Some((tok.clone(), Instant::now() + Duration::from_secs(exp.saturating_sub(60))));
    }
    Ok(tok)
}

// ── Sync ─────────────────────────────────────────────────────────────────────

async fn api(method: reqwest::Method, path: &str, tok: &str, body: Option<Value>) -> Result<Value, String> {
    api_at(API, method, path, tok, body).await
}

async fn api_at(base: &str, method: reqwest::Method, path: &str, tok: &str, body: Option<Value>) -> Result<Value, String> {
    let mut rb = http()?.request(method, format!("{base}{path}")).bearer_auth(tok);
    if let Some(b) = body {
        rb = rb.json(&b);
    }
    let r = rb.send().await.map_err(|e| e.to_string())?;
    let status = r.status();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        if status.as_u16() == 401 {
            if let Ok(mut s) = S.lock() {
                s.access = None;
            }
        }
        let msg: String = v["error"]["message"].as_str().map(|m| m.chars().take(80).collect()).unwrap_or_default();
        return Err(format!("HTTP {} {msg}", status.as_u16()).trim().to_string());
    }
    Ok(v)
}

async fn sync_once() -> Result<(), String> {
    let tok = access_token().await?;
    let list = match S.lock().ok().and_then(|s| s.list.clone()) {
        Some(l) => l,
        None => {
            let v = api(reqwest::Method::GET, "/users/@me/lists?maxResults=1", &tok, None).await?;
            let id = v["items"][0]["id"].as_str().ok_or("no task list")?.to_string();
            if let Ok(mut s) = S.lock() {
                s.list = Some(id.clone());
            }
            id
        }
    };

    // Push what was done in the island first, so the pull below already reflects it.
    let ops: Vec<Op> = S.lock().map(|mut s| std::mem::take(&mut s.ops)).unwrap_or_default();
    for op in ops {
        match op {
            Op::Add(title) => {
                api(reqwest::Method::POST, &format!("/lists/{list}/tasks"), &tok, Some(json!({ "title": title }))).await?;
            }
            Op::Complete(gid) => {
                api(reqwest::Method::PATCH, &format!("/lists/{list}/tasks/{gid}"), &tok, Some(json!({ "status": "completed" }))).await?;
            }
        }
    }

    let v = api(reqwest::Method::GET, &format!("/lists/{list}/tasks?showCompleted=false&showHidden=false&maxResults=100"), &tok, None).await?;
    let items: Vec<(String, String)> = v["items"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|t| t["status"].as_str() == Some("needsAction"))
                .filter_map(|t| Some((t["id"].as_str()?.to_string(), t["title"].as_str()?.trim().to_string())))
                .filter(|(_, title)| !title.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if let Ok(mut s) = S.lock() {
        s.remote = Some(items);
    }

    // Agenda: failures here never fail the to-do sync.
    match fetch_events(&tok).await {
        Ok(ev) => {
            if let Ok(mut s) = S.lock() {
                s.events = ev;
                s.cal_denied = false;
            }
        }
        Err(e) => {
            let denied = e.contains("insufficient") || e.contains("403") || e.contains("Forbidden") || e.contains("not been used") || e.contains("disabled");
            log::line(format!("google calendar: {e}"));
            if let Ok(mut s) = S.lock() {
                s.cal_denied = denied;
            }
        }
    }
    Ok(())
}

/// Today's events from the primary calendar, in the user's local day.
async fn fetch_events(tok: &str) -> Result<Vec<Event>, String> {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    let t = unsafe { GetLocalTime() };
    let mut tz = TIME_ZONE_INFORMATION::default();
    let id = unsafe { GetTimeZoneInformation(&mut tz) };
    // UTC = local + bias, so the offset from UTC is the negative of the bias.
    let bias = tz.Bias + match id { 1 => tz.StandardBias, 2 => tz.DaylightBias, _ => 0 };
    let off = -bias;
    let (sign, a) = if off < 0 { ('-', -off) } else { ('+', off) };
    let day = format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay);
    let zone = format!("{sign}{:02}:{:02}", a / 60, a % 60);
    let path = format!(
        "/calendars/primary/events?timeMin={}&timeMax={}&singleEvents=true&orderBy=startTime&maxResults=15",
        enc(&format!("{day}T00:00:00{zone}")),
        enc(&format!("{day}T23:59:59{zone}"))
    );
    let v = api_at("https://www.googleapis.com/calendar/v3", reqwest::Method::GET, &path, tok, None).await?;
    let hm = |s: &str| s.get(11..16).map(|x| x.to_string());
    let mut out = Vec::new();
    for e in v["items"].as_array().cloned().unwrap_or_default() {
        if e["status"].as_str() == Some("cancelled") {
            continue;
        }
        let title = e["summary"].as_str().unwrap_or("(tanpa judul)").trim().to_string();
        let (start, end) = match e["start"]["dateTime"].as_str() {
            Some(s) => (hm(s).unwrap_or_default(), e["end"]["dateTime"].as_str().and_then(hm)),
            None => ("Seharian".to_string(), None),
        };
        out.push(Event { start, end, title });
    }
    Ok(out)
}

// ── Small helpers (no crypto or encoding crates in this app) ────────────────

fn random(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    unsafe {
        let _ = BCryptGenRandom(None, &mut b, BCRYPT_USE_SYSTEM_PREFERRED_RNG);
    }
    b
}

fn b64url(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len() * 4 / 3 + 2);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        if c.len() > 1 {
            out.push(T[(n >> 6) as usize & 63] as char);
        }
        if c.len() > 2 {
            out.push(T[n as usize & 63] as char);
        }
    }
    out
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

fn dec(s: &str) -> String {
    let b = s.as_bytes();
    let (mut o, mut i) = (Vec::new(), 0);
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    o.push(v);
                    i += 3;
                    continue;
                }
                o.push(b'%');
            }
            b'+' => o.push(b' '),
            c => o.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&o).into_owned()
}

fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01,
        0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08,
        0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    let mut out = [0u8; 32];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vector() {
        let h = sha256(b"abc");
        let hex: String = h.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn pkce_rfc7636_vector() {
        // RFC 7636 appendix B.
        let c = b64url(&sha256(b"dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"));
        assert_eq!(c, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn percent_round_trip() {
        assert_eq!(dec(&enc("a b/c?d=é&")), "a b/c?d=é&");
    }
}
