// SoundEngine — the 28 WAVs are embedded in the exe (paged in by the OS only when
// a sound plays) and mixed by Windows through one short-lived waveOut device per
// sound, so they overlap like they do on the Mac. Volume is applied to the PCM
// before it is handed over: the slider range is 0–0.2 like SoundEngine.swift.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows::Win32::Media::Audio::{
    waveOutClose, waveOutOpen, waveOutPrepareHeader, waveOutUnprepareHeader, waveOutWrite,
    CALLBACK_EVENT, HWAVEOUT, WAVEFORMATEX, WAVEHDR, WAVE_FORMAT_PCM, WAVE_MAPPER,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::Foundation::CloseHandle;

macro_rules! wavs {
    ($($n:literal),* $(,)?) => {
        fn data(name: &str) -> Option<&'static [u8]> {
            match name {
                $($n => Some(include_bytes!(concat!("../../../NotchBuddy/Resources/sounds/", $n, ".wav")) as &[u8]),)*
                _ => None,
            }
        }
    };
}

wavs!(
    "peek", "open", "close", "hover", "blip", "slap", "annoyed", "dizzy", "greet", "work", "finish",
    "error", "approval", "question", "approve", "gulp", "tick", "send", "love", "pop", "proud", "wink",
    "yawn", "attach", "think", "search", "rate", "sleep",
);

static ENABLED: AtomicBool = AtomicBool::new(true);
static VOLUME: AtomicU32 = AtomicU32::new(0x3DF5_C28F); // 0.12

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn set_volume(v: f32) {
    VOLUME.store(v.clamp(0.0, 0.2).to_bits(), Ordering::Relaxed);
}

fn volume() -> f32 {
    f32::from_bits(VOLUME.load(Ordering::Relaxed))
}

pub fn play(name: &str) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let Some(bytes) = data(name) else { return };
    let vol = volume();
    if vol <= 0.0 {
        return;
    }
    let _ = std::thread::Builder::new().name("coucou-snd".into()).spawn(move || unsafe {
        let Some(pcm) = find_data(bytes) else { return };
        let mut buf: Vec<u8> = Vec::with_capacity(pcm.len());
        for s in pcm.chunks_exact(2) {
            let v = i16::from_le_bytes([s[0], s[1]]) as f32 * vol;
            buf.extend_from_slice(&(v as i16).to_le_bytes());
        }
        // fmt chunk of these files: PCM, 2 channels, 48 kHz, 16 bit.
        let fmt = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: 2,
            nSamplesPerSec: 48_000,
            nAvgBytesPerSec: 48_000 * 4,
            nBlockAlign: 4,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        let Ok(event) = CreateEventW(None, false, false, None) else { return };
        let mut hwo = HWAVEOUT::default();
        if waveOutOpen(Some(&mut hwo), WAVE_MAPPER, &fmt, Some(event.0 as usize), Some(0), CALLBACK_EVENT) != 0 {
            let _ = CloseHandle(event);
            return;
        }
        let mut hdr = WAVEHDR {
            lpData: windows::core::PSTR(buf.as_mut_ptr()),
            dwBufferLength: buf.len() as u32,
            ..Default::default()
        };
        let sz = std::mem::size_of::<WAVEHDR>() as u32;
        if waveOutPrepareHeader(hwo, &mut hdr, sz) == 0 {
            if waveOutWrite(hwo, &mut hdr, sz) == 0 {
                let ms = (buf.len() as u64 * 1000 / (48_000 * 4)) as u32 + 400;
                let mut waited = 0;
                // WHDR_DONE = 1
                while waited < ms && (hdr.dwFlags & 1) == 0 {
                    WaitForSingleObject(event, 50);
                    waited += 50;
                }
            }
            let _ = waveOutUnprepareHeader(hwo, &mut hdr, sz);
        }
        let _ = waveOutClose(hwo);
        let _ = CloseHandle(event);
    });
}

/// The `data` chunk of a canonical WAV file.
fn find_data(b: &[u8]) -> Option<&[u8]> {
    let mut i = 12;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        if id == b"data" {
            return b.get(i + 8..(i + 8 + len).min(b.len()));
        }
        i += 8 + len + (len & 1);
    }
    None
}
