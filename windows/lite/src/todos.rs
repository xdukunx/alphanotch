// The dashboard's to-do list: local, plain JSON in %APPDATA%\Coucou\todos.json.
// Starred items float to the top. Completing an item removes it.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub text: String,
    #[serde(default)]
    pub star: bool,
    /// Google Tasks id once the item is synced.
    #[serde(default)]
    pub gid: Option<String>,
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
