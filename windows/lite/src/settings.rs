// Preferences, stored as plain JSON in %APPDATA%\Coucou\settings.json.
// No secret ever lands here — API keys live in the Windows Credential Manager.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub sound_enabled: bool,
    pub sound_volume: f64,
    pub auto_close_interval: f64,
    pub absence_interval: f64,
    pub active_integrations: Vec<String>,
    /// "primary" = the main display, "cursor" = whichever display the mouse is on.
    pub screen: String,
    pub autostart: bool,
    pub hooks_installed: bool,
    /// Keep the compact bar on screen when idle instead of hiding it (docked in a status bar).
    #[serde(default)]
    pub stay_visible: bool,
    /// Agent pills kept on the island even when idle: names as sent with `--agent <name>`
    /// ("antigravity", "opencode", …). Claude Code always has its own pill.
    #[serde(default)]
    pub agent_pills: Vec<String>,
    /// "auto" (default) estimates the place from the IP address; or a city name, e.g. "Surabaya".
    #[serde(default = "default_city")]
    pub weather_city: String,
    /// Watchlist for the stocks page. `^JKSE` is the IHSG; plain codes are Jakarta (BBCA → BBCA.JK).
    #[serde(default = "default_stocks")]
    pub stocks: Vec<String>,
    /// Claude model used by the chat. Changeable in the settings window.
    /// Defaulted explicitly so a settings.json written by an older build still loads.
    #[serde(default = "default_model")]
    pub model: String,
}

fn default_city() -> String {
    "auto".into()
}

fn default_stocks() -> Vec<String> {
    ["^JKSE", "BBCA.JK", "BBRI.JK", "BMRI.JK", "TLKM.JK"].iter().map(|s| s.to_string()).collect()
}

fn default_model() -> String {
    crate::claude::DEFAULT_MODEL.to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sound_enabled: true,
            sound_volume: 0.12,
            auto_close_interval: 15.0,
            absence_interval: 180.0,
            active_integrations: vec![
                "integration_resend".into(),
                "integration_n8n".into(),
                "integration_vercel".into(),
                "integration_github".into(),
            ],
            screen: "primary".into(),
            autostart: false,
            hooks_installed: false,
            stay_visible: false,
            agent_pills: Vec::new(),
            weather_city: default_city(),
            stocks: default_stocks(),
            model: default_model(),
        }
    }
}

/// %APPDATA%\Coucou
pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("Coucou")
}

/// %LOCALAPPDATA%\Coucou — where coucou-hook.exe and the log live.
pub fn local_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("Coucou")
}

pub fn hook_exe_path() -> PathBuf {
    local_dir().join("bin").join("coucou-hook.exe")
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn load() -> Settings {
    match std::fs::read(settings_path()) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

pub fn save(settings: &Settings) -> std::io::Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_vec_pretty(settings)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(settings_path(), json)
}

/// "Launch at startup": a value under HKCU\...\Run, no installer involved.
pub fn set_autostart(on: bool) {
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_SZ,
    };
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Run"),
            Some(0),
            KEY_SET_VALUE,
            &mut key,
        )
        .is_err()
        {
            return;
        }
        if on {
            if let Ok(exe) = std::env::current_exe() {
                let cmd = format!("\"{}\"", exe.display());
                let wide: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
                let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
                let _ = RegSetValueExW(key, w!("Coucou"), Some(0), REG_SZ, Some(bytes));
            }
        } else {
            let _ = RegDeleteValueW(key, w!("Coucou"));
        }
        let _ = RegCloseKey(key);
    }
}
