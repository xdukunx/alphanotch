// Live activities for the compact bar and the dashboard: what is going on in the background.
//
// Three sources, each polled on its own cheap thread and published through one
// small shared struct the UI thread reads on every frame:
//   - media     Windows' system media session (Spotify, browsers, players…), with its
//               artwork, timeline and transport controls
//   - timer     a countdown started over the pipe (`CoucouTimer`), the CLI or the dashboard
//   - download  a browser's partial file growing in the Downloads folder
// Nothing here talks to the network, and nothing is polled while the app is paused.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapBounds, BitmapDecoder, BitmapPixelFormat, BitmapTransform,
    ColorManagementMode, ExifOrientationMode,
};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Playback,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

#[derive(Clone)]
pub struct Media {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    /// Seconds into the track when it was sampled, and the track length (0 = unknown).
    pub position: f32,
    pub duration: f32,
    pub sampled: Instant,
}

impl PartialEq for Media {
    fn eq(&self, o: &Self) -> bool {
        self.title == o.title && self.artist == o.artist && self.playing == o.playing
    }
}

impl Media {
    /// Position now, extrapolated from the last sample while it plays.
    pub fn position_now(&self) -> f32 {
        let p = if self.playing { self.position + self.sampled.elapsed().as_secs_f32() } else { self.position };
        if self.duration > 0.0 { p.min(self.duration) } else { p }
    }
}

/// Album art, square, premultiplied RGBA.
pub struct Art {
    pub key: u64,
    pub size: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, PartialEq)]
pub struct Download {
    pub name: String,
    pub bytes: u64,
}

struct Shared {
    media: Option<Media>,
    art: Option<Arc<Art>>,
    download: Option<Download>,
    timer: Option<(Instant, f32)>,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared { media: None, art: None, download: None, timer: None });
static STARTED: AtomicBool = AtomicBool::new(false);
static PAUSED: AtomicBool = AtomicBool::new(false);

/// What the compact bar should show right now. One activity at a time, in this order:
/// a running timer, then music that is playing, then a download in progress.
#[derive(Clone, PartialEq)]
pub enum Activity {
    Timer { remaining: f32, total: f32 },
    Media(Media),
    Download(Download),
}

pub fn current() -> Option<Activity> {
    let s = SHARED.lock().ok()?;
    if let Some((end, total)) = s.timer {
        let remaining = end.saturating_duration_since(Instant::now()).as_secs_f32();
        if remaining > 0.0 {
            return Some(Activity::Timer { remaining, total });
        }
    }
    if let Some(m) = &s.media {
        if m.playing {
            return Some(Activity::Media(m.clone()));
        }
    }
    s.download.clone().map(Activity::Download)
}

/// The current media session even when it is paused (the dashboard keeps showing it).
pub fn media() -> Option<Media> {
    SHARED.lock().ok()?.media.clone()
}

pub fn art() -> Option<Arc<Art>> {
    SHARED.lock().ok()?.art.clone()
}

/// (seconds left, total) of a running timer.
pub fn timer() -> Option<(f32, f32)> {
    let s = SHARED.lock().ok()?;
    let (end, total) = s.timer?;
    let left = end.saturating_duration_since(Instant::now()).as_secs_f32();
    (left > 0.0).then_some((left, total))
}

/// A finished timer, handed out once so the island can celebrate it.
pub fn take_finished_timer() -> bool {
    let Ok(mut s) = SHARED.lock() else { return false };
    match s.timer {
        Some((end, _)) if Instant::now() >= end => {
            s.timer = None;
            true
        }
        _ => false,
    }
}

pub fn start_timer(minutes: f32) {
    let secs = (minutes * 60.0).clamp(1.0, 6.0 * 3600.0);
    if let Ok(mut s) = SHARED.lock() {
        s.timer = Some((Instant::now() + Duration::from_secs_f32(secs), secs));
    }
}

pub fn cancel_timer() {
    if let Ok(mut s) = SHARED.lock() {
        s.timer = None;
    }
}

pub fn set_paused(on: bool) {
    PAUSED.store(on, Ordering::Relaxed);
}

#[derive(Clone, Copy)]
pub enum Transport {
    PlayPause,
    Next,
    Previous,
    /// Seek to this many seconds.
    Seek(f32),
}

/// Sends a transport command to the current media session. Fire and forget.
pub fn transport(cmd: Transport) {
    // Optimistic: flip the cached state now so the button answers before the next poll.
    if let Ok(mut s) = SHARED.lock() {
        if let Some(m) = s.media.as_mut() {
            match cmd {
                Transport::PlayPause => {
                    m.position = m.position_now();
                    m.sampled = Instant::now();
                    m.playing = !m.playing;
                }
                Transport::Seek(t) => {
                    m.position = t;
                    m.sampled = Instant::now();
                }
                _ => {}
            }
        }
    }
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(mgr) = Manager::RequestAsync().and_then(|o| o.get()) else { return };
        let Ok(s) = mgr.GetCurrentSession() else { return };
        let _ = match cmd {
            Transport::PlayPause => s.TryTogglePlayPauseAsync().and_then(|o| o.get()),
            Transport::Next => s.TrySkipNextAsync().and_then(|o| o.get()),
            Transport::Previous => s.TrySkipPreviousAsync().and_then(|o| o.get()),
            Transport::Seek(t) => s.TryChangePlaybackPositionAsync((t as f64 * 1.0e7) as i64).and_then(|o| o.get()),
        };
    });
}

pub fn toggle_media() {
    transport(Transport::PlayPause);
}

pub fn start() {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new().name("coucou-media".into()).spawn(media_loop);
    let _ = std::thread::Builder::new().name("coucou-downloads".into()).spawn(download_loop);
}

fn media_loop() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let mut mgr: Option<Manager> = None;
    let mut art_for = (String::new(), String::new());
    let mut art_tries = 0u32;
    loop {
        if !PAUSED.load(Ordering::Relaxed) {
            if mgr.is_none() {
                mgr = Manager::RequestAsync().and_then(|o| o.get()).ok();
            }
            let session = mgr.as_ref().and_then(|m| m.GetCurrentSession().ok());
            let now = session.as_ref().and_then(read_media);
            if let Ok(mut s) = SHARED.lock() {
                s.media = now.clone();
                if now.is_none() {
                    s.art = None;
                }
            }
            if let (Some(m), Some(sess)) = (&now, &session) {
                let id = (m.title.clone(), m.artist.clone());
                if id != art_for {
                    art_for = id;
                    art_tries = 0;
                    if let Ok(mut s) = SHARED.lock() {
                        s.art = None;
                    }
                }
                // The artwork often arrives a moment after the title: try a few times.
                let missing = SHARED.lock().map(|s| s.art.is_none()).unwrap_or(false);
                if missing && art_tries < 8 {
                    art_tries += 1;
                    if let Some(a) = read_art(sess, &art_for) {
                        if let Ok(mut s) = SHARED.lock() {
                            s.art = Some(Arc::new(a));
                        }
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(1500));
    }
}

fn read_media(s: &Session) -> Option<Media> {
    let playing = s.GetPlaybackInfo().ok()?.PlaybackStatus().ok()? == Playback::Playing;
    let props = s.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
    let title = props.Title().ok()?.to_string();
    if title.is_empty() {
        return None;
    }
    let (mut position, mut duration) = (0.0f32, 0.0f32);
    if let Ok(t) = s.GetTimelineProperties() {
        let secs = |d: windows::Foundation::TimeSpan| d.Duration as f64 / 1.0e7;
        if let (Ok(p), Ok(start), Ok(end)) = (t.Position(), t.StartTime(), t.EndTime()) {
            position = (secs(p) - secs(start)).max(0.0) as f32;
            duration = (secs(end) - secs(start)).max(0.0) as f32;
        }
    }
    Some(Media {
        title,
        artist: props.Artist().map(|a| a.to_string()).unwrap_or_default(),
        playing,
        position,
        duration,
        sampled: Instant::now(),
    })
}

/// Decodes the session's thumbnail into a 160 px centre-cropped square.
fn read_art(s: &Session, id: &(String, String)) -> Option<Art> {
    match decode_art(s, id) {
        Ok(a) => Some(a),
        Err(e) => {
            crate::log::line(format!("album art: {e}"));
            None
        }
    }
}

fn decode_art(s: &Session, id: &(String, String)) -> windows::core::Result<Art> {
    use windows::core::Error;
    const SIDE: u32 = 160;
    let props = s.TryGetMediaPropertiesAsync()?.get()?;
    let stream = props.Thumbnail()?.OpenReadAsync()?.get()?;
    let dec = BitmapDecoder::CreateAsync(&stream)?.get()?;
    let (w, h) = (dec.PixelWidth()?, dec.PixelHeight()?);
    let side = w.min(h);
    if side == 0 {
        return Err(Error::empty());
    }
    let tf = BitmapTransform::new()?;
    // The transform scales first and crops after: scale the short side to SIDE, then cut the
    // centre square out of the scaled image.
    let (sw, sh) = ((w as u64 * SIDE as u64 / side as u64) as u32, (h as u64 * SIDE as u64 / side as u64) as u32);
    let (sw, sh) = (sw.max(SIDE), sh.max(SIDE));
    tf.SetScaledWidth(sw)?;
    tf.SetScaledHeight(sh)?;
    tf.SetBounds(BitmapBounds { X: (sw - SIDE) / 2, Y: (sh - SIDE) / 2, Width: SIDE, Height: SIDE })?;
    let pixels = dec
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Bgra8,
            BitmapAlphaMode::Premultiplied,
            &tf,
            ExifOrientationMode::IgnoreExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )?
        .get()?;
    let bgra = pixels.DetachPixelData()?;
    if bgra.len() < (SIDE * SIDE * 4) as usize {
        return Err(Error::empty());
    }
    let mut rgba = bgra.to_vec();
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let mut key = 0xcbf2_9ce4_8422_2325u64;
    for b in id.0.bytes().chain(id.1.bytes()) {
        key = (key ^ b as u64).wrapping_mul(0x100_0000_01b3);
    }
    Ok(Art { key, size: SIDE, rgba })
}

fn download_loop() {
    let dir = std::env::var_os("USERPROFILE").map(|p| std::path::PathBuf::from(p).join("Downloads"));
    loop {
        if !PAUSED.load(Ordering::Relaxed) {
            let found = dir.as_deref().and_then(newest_partial);
            if let Ok(mut s) = SHARED.lock() {
                s.download = found;
            }
        }
        std::thread::sleep(Duration::from_millis(2500));
    }
}

/// A browser's in-progress file (`.crdownload`, `.part`, `.download`) touched in the last few seconds.
fn newest_partial(dir: &std::path::Path) -> Option<Download> {
    let mut best: Option<(SystemTime, Download)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let path = e.path();
        let Some(ext) = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()) else { continue };
        if !matches!(ext.as_str(), "crdownload" | "part" | "download" | "opdownload") {
            continue;
        }
        let Ok(meta) = e.metadata() else { continue };
        let Ok(modified) = meta.modified() else { continue };
        if modified.elapsed().map(|d| d > Duration::from_secs(8)).unwrap_or(true) {
            continue;
        }
        let Some(stem) = path.file_stem() else { continue };
        let name = stem.to_string_lossy().to_string();
        if best.as_ref().map(|(t, _)| modified > *t).unwrap_or(true) {
            best = Some((modified, Download { name, bytes: meta.len() }));
        }
    }
    best.map(|(_, d)| d)
}

pub fn format_bytes(b: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let f = b as f64;
    if f >= 1024.0 * MB {
        format!("{:.1} GB", f / (1024.0 * MB))
    } else if f >= MB {
        format!("{:.0} MB", f / MB)
    } else {
        format!("{:.0} KB", f / 1024.0)
    }
}

pub fn format_clock(secs: f32) -> String {
    let s = secs.ceil().max(0.0) as u32;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}
