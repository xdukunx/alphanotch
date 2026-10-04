// Installs Coucou's hooks into the agents the user keeps a pill for, from the command line:
//
//   coucou-lite.exe --agent-hooks <claude|antigravity> <status|preview|install|uninstall>
//                   [--fingerprint <fp>] [--out <file>]
//
// Same rules as the Settings window's Claude Code installer: `preview` prints the exact diff
// and a fingerprint of the file it was computed from; `install` and `uninstall` refuse unless
// they are handed that fingerprint (so only what was looked at is applied), take a dated
// backup first, never touch entries that are not Coucou's, and write through a temp file.
//
// `--out` writes the report to a file instead of stdout: this exe is a GUI-subsystem binary
// and has no console of its own.

use std::io::Write;
use std::path::PathBuf;

use serde_json::{json, Map, Value};

use crate::{hooks, settings};

struct Out(Option<std::fs::File>);

impl Out {
    fn line(&mut self, s: impl AsRef<str>) {
        match &mut self.0 {
            Some(f) => {
                let _ = writeln!(f, "{}", s.as_ref());
            }
            None => println!("{}", s.as_ref()),
        }
    }
}

/// Entry point for `--agent-hooks …`; returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    let mut it = args.iter();
    let agent = it.next().cloned().unwrap_or_default();
    let action = it.next().cloned().unwrap_or_default();
    let (mut fingerprint, mut out_path) = (String::new(), None::<String>);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--fingerprint" => fingerprint = it.next().cloned().unwrap_or_default(),
            "--out" => out_path = it.next().cloned(),
            _ => {}
        }
    }
    let mut out = Out(out_path.and_then(|p| std::fs::File::create(p).ok()));

    let result = match agent.as_str() {
        "claude" => claude(&action, &fingerprint, &mut out),
        "antigravity" => antigravity(&action, &fingerprint, &mut out),
        other => Err(format!("unsupported agent '{other}' (use claude or antigravity)")),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            out.line(format!("error: {e}"));
            1
        }
    }
}

// ── Claude Code (~/.claude/settings.json), through the existing installer ──────────

fn claude(action: &str, fingerprint: &str, out: &mut Out) -> Result<(), String> {
    match action {
        "status" => {
            let s = hooks::status();
            out.line(format!("installed: {}", s.installed));
            out.line(format!("file: {}", s.settings_path));
        }
        "preview" | "install" | "uninstall" => {
            let install = action != "uninstall";
            if action == "preview" {
                let p = hooks::preview(true)?;
                out.line(format!("file: {}", p.settings_path));
                out.line(format!("backup would be: {}", p.backup));
                out.line(format!("fingerprint: {}", p.fingerprint));
                out.line("--- diff ---");
                out.line(p.diff);
            } else {
                require_fingerprint(fingerprint)?;
                let backup = hooks::write(install, fingerprint)?;
                out.line(format!("done. backup: {backup}"));
            }
        }
        _ => return Err("action must be status, preview, install or uninstall".into()),
    }
    Ok(())
}

// ── Antigravity (~/.gemini/config/hooks.json) ─────────────────────────────────────

const AGY_KEY: &str = "coucou";

fn agy_path() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".gemini")
        .join("config")
        .join("hooks.json")
}

fn agy_read(path: &PathBuf) -> Result<Value, String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let text = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
            if text.iter().all(u8::is_ascii_whitespace) {
                return Ok(json!({}));
            }
            match serde_json::from_slice::<Value>(text) {
                Ok(v) if v.is_object() => Ok(v),
                Ok(_) => Err(format!("{} isn't a JSON object, so Coucou won't touch it.", path.display())),
                Err(e) => Err(format!("{} isn't valid JSON ({e}); Coucou won't overwrite it.", path.display())),
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(format!("can't read {}: {e}", path.display())),
    }
}

/// `"<hook exe>" --agent antigravity <Event>`, the exe path with forward slashes like the Claude entries.
fn agy_command(event: &str) -> String {
    let exe = settings::hook_exe_path().to_string_lossy().replace('\\', "/");
    format!("\"{exe}\" --agent antigravity {event}")
}

fn agy_entry() -> Value {
    let hook = |event: &str| json!({ "type": "command", "command": agy_command(event), "timeout": 5 });
    let mut m = Map::new();
    m.insert("enabled".into(), json!(true));
    // Tool events carry a matcher group; lifecycle events are plain handlers (same layout as the
    // other tools' entries already in this file).
    for event in ["PreToolUse", "PostToolUse"] {
        m.insert(event.into(), json!([{ "matcher": "*", "hooks": [hook(event)] }]));
    }
    for event in ["PreInvocation", "PostInvocation", "Stop"] {
        m.insert(event.into(), json!([hook(event)]));
    }
    Value::Object(m)
}

fn agy_next(current: &Value, install: bool) -> Value {
    let mut next = current.clone();
    if let Some(obj) = next.as_object_mut() {
        if install {
            obj.insert(AGY_KEY.into(), agy_entry());
        } else {
            obj.remove(AGY_KEY);
        }
    }
    next
}

fn pretty(v: &Value) -> String {
    let mut s = serde_json::to_string_pretty(v).unwrap_or_default();
    s.push('\n');
    s
}

fn antigravity(action: &str, fingerprint: &str, out: &mut Out) -> Result<(), String> {
    let path = agy_path();
    let current = agy_read(&path)?;
    let on_disk = std::fs::read(&path).unwrap_or_default();
    match action {
        "status" => {
            let installed = current.get(AGY_KEY).map(|v| v.to_string().contains("coucou-hook")).unwrap_or(false);
            out.line(format!("installed: {installed}"));
            out.line(format!("file: {}", path.display()));
            let others: Vec<&String> = current.as_object().map(|o| o.keys().filter(|k| *k != AGY_KEY).collect()).unwrap_or_default();
            out.line(format!("other entries left alone: {others:?}"));
        }
        "preview" => {
            out.line(format!("file: {}", path.display()));
            out.line(format!("fingerprint: {}", hooks::fingerprint(&on_disk)));
            out.line("--- diff (install) ---");
            out.line(hooks::unified_diff(&String::from_utf8_lossy(&on_disk), &pretty(&agy_next(&current, true))));
        }
        "install" | "uninstall" => {
            require_fingerprint(fingerprint)?;
            if hooks::fingerprint(&on_disk) != fingerprint {
                return Err(format!("{} changed since the preview. Nothing was written; preview again.", path.display()));
            }
            let next = agy_next(&current, action == "install");
            if path.exists() {
                let backup = path.with_file_name(format!("hooks.json.bak-{}", hooks::stamp()));
                std::fs::copy(&path, &backup).map_err(|e| format!("backup failed: {e}"))?;
                out.line(format!("backup: {}", backup.display()));
            } else if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let temp = path.with_extension(format!("json.coucou-{}", std::process::id()));
            std::fs::write(&temp, pretty(&next)).map_err(|e| format!("write failed: {e}"))?;
            if let Err(e) = std::fs::rename(&temp, &path) {
                let _ = std::fs::remove_file(&temp);
                return Err(format!("write failed: {e}"));
            }
            out.line(format!("done: {} entry '{AGY_KEY}' in {}", if action == "install" { "added" } else { "removed" }, path.display()));
        }
        _ => return Err("action must be status, preview, install or uninstall".into()),
    }
    Ok(())
}

fn require_fingerprint(fp: &str) -> Result<(), String> {
    if fp.is_empty() {
        return Err("pass --fingerprint <value from preview>: nothing is written without it".into());
    }
    Ok(())
}
