//! coucou-hook — the relay Claude Code runs on every hook event.
//!
//! Reads the hook JSON on stdin, adds a little terminal context, and hands it to
//! Coucou over the named pipe `\\.\pipe\coucou-<sid>` (Windows) or the Unix
//! socket `$XDG_RUNTIME_DIR/coucou.sock` (Linux).
//!
//! Hard rule (docs/CLAUDE.md): **never block Claude Code.**
//! * If the pipe does not exist — Coucou is closed — we exit 0 immediately with
//!   nothing on stdout, and the session carries on untouched.
//! * Every step runs under a deadline enforced by the main thread, so a pipe that
//!   accepts the connection and then stops reading cannot wedge the session
//!   either: we abandon the worker and exit.
//! * Only `PermissionRequest` — and `--ask`, below — waits for an answer, because
//!   approving from the island is the whole point. No answer means empty stdout,
//!   and Claude Code asks in the terminal exactly as if Coucou were not installed.
//!
//! Two additions on top of forwarding:
//! * **Diffs.** For `Edit`, `MultiEdit` and `Write` the relay computes the line
//!   diff itself and forwards it as `coucou_diff`. It has to happen here: every
//!   string is cut to `MAX_FIELD_LEN` before it leaves, which would make the
//!   island's counts wrong for anything but a tiny edit.
//! * **`--ask`.** A dedicated `PreToolUse` hook for `AskUserQuestion`. The island
//!   shows the options; the answer comes back as `updatedInput.answers`, so
//!   Claude Code never prompts in the terminal.
//!
//! Usage: `coucou-hook <EventName>` or `coucou-hook --ask`.

use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::Duration;

/// Budget for getting a pipe connection. Beyond this Claude Code wins, always.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
/// Whole-run budget for an event nobody waits on: connect and write, no more.
const FIRE_AND_FORGET_BUDGET: Duration = Duration::from_secs(2);
/// How long a permission prompt may stay on screen before the terminal takes over.
const DECISION_BUDGET: Duration = Duration::from_secs(110);
/// A question may take longer to answer than a yes/no. The hook entry's timeout in
/// settings.json is 130 s, so we always speak first.
const ASK_BUDGET: Duration = Duration::from_secs(125);

/// Fields that are pointless to forward and can be enormous (a whole file read,
/// a full command output). The island never shows them.
const DROPPED_FIELDS: &[&str] = &["tool_response", "transcript_path"];
/// Longest string forwarded for any single field; the island truncates to far
/// less than this anyway.
const MAX_FIELD_LEN: usize = 2_000;

#[cfg(windows)]
mod win;
#[cfg(windows)]
use win::connect;

#[cfg(target_os = "linux")]
mod unix;
#[cfg(target_os = "linux")]
use unix::connect;

fn main() {
    if std::env::args().any(|a| a == "--statusline") {
        statusline();
    }
    let Some(Event { line: payload, name: event, ask }) = read_event() else { std::process::exit(0) };

    let waits_for_answer = event == "PermissionRequest" || ask.is_some();
    let budget = if ask.is_some() {
        ASK_BUDGET
    } else if waits_for_answer {
        DECISION_BUDGET
    } else {
        FIRE_AND_FORGET_BUDGET
    };

    // The worker owns every blocking call. If it overruns the budget we simply
    // stop listening and exit: the process dying takes the pipe handle with it.
    // (No catch_unwind here — the release profile is panic = "abort", so it would
    // be dead code. `talk` is written to have nothing to panic on instead.)
    let (tx, rx) = mpsc::channel::<Option<String>>();
    std::thread::spawn(move || {
        let _ = tx.send(talk(&payload, waits_for_answer));
    });

    if let Ok(Some(decision)) = rx.recv_timeout(budget) {
        let json = match &ask {
            Some(questions) => ask_json(&decision, questions),
            None => decision_json(&decision),
        };
        if let Some(json) = json {
            let mut out = std::io::stdout();
            let _ = writeln!(out, "{json}");
            let _ = out.flush();
        }
    }
    // Nothing printed: Claude Code asks in the terminal, as if we were not here.
    std::process::exit(0);
}

/// The documented PermissionRequest output. Anything we do not recognise prints
/// nothing at all rather than guessing — silence is the safe answer.
/// See https://code.claude.com/docs/en/hooks
fn decision_json(decision: &str) -> Option<String> {
    let behavior = match decision.trim() {
        // "always" still answers a plain allow; remembering it is the island's
        // business, not Claude Code's.
        "allow" | "always" => r#"{"behavior":"allow"}"#.to_string(),
        "deny" => r#"{"behavior":"deny","message":"Denied from Coucou"}"#.to_string(),
        _ => return None,
    };
    Some(format!(
        r#"{{"hookSpecificOutput":{{"hookEventName":"PermissionRequest","decision":{behavior}}}}}"#
    ))
}

/// The documented PreToolUse output that answers `AskUserQuestion` on the user's
/// behalf. The reply from the island is `{"permissionDecision":"answer","answers":{…}}`;
/// anything else ("ask", garbage) prints nothing and Claude Code asks in the terminal.
fn ask_json(reply: &str, questions: &serde_json::Value) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(reply.trim()).ok()?;
    if v.get("permissionDecision").and_then(|d| d.as_str()) != Some("answer") {
        return None;
    }
    let answers = v.get("answers").filter(|a| a.is_object())?.clone();
    Some(
        serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "allow",
                "updatedInput": { "questions": questions, "answers": answers }
            }
        })
        .to_string(),
    )
}

/// `coucou-hook --statusline`: Claude Code's status line command. Claude Code hands it the
/// session's JSON; only the plan limits (`rate_limits`) go on to the island, in the background,
/// and the line printed back is those limits, short. Never more than 300 ms for the island.
fn statusline() -> ! {
    let mut raw = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut raw);
    if raw.starts_with(&[0xEF, 0xBB, 0xBF]) {
        raw.drain(..3);
    }
    let v: Option<serde_json::Value> = serde_json::from_slice(&raw).ok();
    let mut out = String::new();
    if let Some(limits) = v.as_ref().and_then(|v| v.get("rate_limits")).filter(|l| l.is_object()) {
        let line = serde_json::json!({ "hook_event_name": "StatusLine", "rate_limits": limits }).to_string() + "\n";
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(talk(&line, false));
        });
        out = status_text(limits);
        let _ = rx.recv_timeout(CONNECT_TIMEOUT);
    }
    if out.is_empty() {
        if let Some(m) = v.as_ref().and_then(|v| v.pointer("/model/display_name")).and_then(|m| m.as_str()) {
            out = m.to_string();
        }
    }
    print!("{out}");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    std::process::exit(0);
}

/// "5 jam 35% · minggu 54%".
fn status_text(limits: &serde_json::Value) -> String {
    let pct = |k: &str| limits.get(k).and_then(|w| w.get("used_percentage")).and_then(|p| p.as_f64());
    let mut parts = Vec::new();
    if let Some(p) = pct("five_hour") {
        parts.push(format!("5 jam {:.0}%", p.min(100.0)));
    }
    if let Some(p) = pct("seven_day") {
        parts.push(format!("minggu {:.0}%", p.min(100.0)));
    }
    parts.join(" · ")
}

struct Event {
    line: String,
    name: String,
    /// `Some(questions)` in `--ask` mode: the original, untruncated list, which has
    /// to go back to Claude Code next to the answers.
    ask: Option<serde_json::Value>,
}

// ── Line diff ─────────────────────────────────────────────────────────────────

/// Longest diff body forwarded, in lines, and the longest single line.
const MAX_DIFF_LINES: usize = 160;
const MAX_DIFF_LINE_LEN: usize = 200;
/// Beyond this many cells the quadratic table is not worth it: the changed middle
/// is reported as removed-then-added without matching lines inside it.
const MAX_LCS_CELLS: usize = 1_500_000;
/// Unchanged lines kept around each change.
const CONTEXT: usize = 2;

fn split_lines(s: &str) -> Vec<&str> {
    if s.is_empty() {
        return Vec::new();
    }
    let mut v: Vec<&str> = s.split('\n').collect();
    if v.last() == Some(&"") {
        v.pop();
    }
    v.into_iter().map(|l| l.strip_suffix('\r').unwrap_or(l)).collect()
}

/// `(' ' | '-' | '+', line)` for every line of the merged diff.
fn diff_ops<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(char, &'a str)> {
    let pre = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let suf = old[pre..]
        .iter()
        .rev()
        .zip(new[pre..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (o, n) = (&old[pre..old.len() - suf], &new[pre..new.len() - suf]);

    let mut ops: Vec<(char, &str)> = old[..pre].iter().map(|l| (' ', *l)).collect();
    if o.is_empty() || n.is_empty() || o.len() * n.len() > MAX_LCS_CELLS {
        ops.extend(o.iter().map(|l| ('-', *l)));
        ops.extend(n.iter().map(|l| ('+', *l)));
    } else {
        // Classic LCS table over the changed middle only.
        let w = n.len() + 1;
        let mut t = vec![0u16; (o.len() + 1) * w];
        for i in (0..o.len()).rev() {
            for j in (0..n.len()).rev() {
                t[i * w + j] = if o[i] == n[j] {
                    t[(i + 1) * w + j + 1] + 1
                } else {
                    t[(i + 1) * w + j].max(t[i * w + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < o.len() && j < n.len() {
            if o[i] == n[j] {
                ops.push((' ', o[i]));
                i += 1;
                j += 1;
            } else if t[(i + 1) * w + j] >= t[i * w + j + 1] {
                ops.push(('-', o[i]));
                i += 1;
            } else {
                ops.push(('+', n[j]));
                j += 1;
            }
        }
        ops.extend(o[i..].iter().map(|l| ('-', *l)));
        ops.extend(n[j..].iter().map(|l| ('+', *l)));
    }
    ops.extend(old[old.len() - suf..].iter().map(|l| (' ', *l)));
    ops
}

struct DiffOut {
    added: usize,
    removed: usize,
    lines: Vec<String>,
}

fn diff_text(old: &str, new: &str) -> DiffOut {
    let (o, n) = (split_lines(old), split_lines(new));
    let ops = diff_ops(&o, &n);
    let added = ops.iter().filter(|(c, _)| *c == '+').count();
    let removed = ops.iter().filter(|(c, _)| *c == '-').count();

    // Keep the changes and a little context; elide the rest.
    let mut keep = vec![false; ops.len()];
    for (i, (c, _)) in ops.iter().enumerate() {
        if *c != ' ' {
            let from = i.saturating_sub(CONTEXT);
            let to = (i + CONTEXT + 1).min(ops.len());
            keep[from..to].iter_mut().for_each(|k| *k = true);
        }
    }
    let mut lines = Vec::new();
    let mut gap = false;
    for (i, (c, l)) in ops.iter().enumerate() {
        if !keep[i] {
            gap = !lines.is_empty();
            continue;
        }
        if gap {
            lines.push("@@".to_string());
            gap = false;
        }
        let mut text: String = l.chars().take(MAX_DIFF_LINE_LEN).collect();
        if l.chars().count() > MAX_DIFF_LINE_LEN {
            text.push('…');
        }
        lines.push(format!("{c}{text}"));
        if lines.len() >= MAX_DIFF_LINES {
            lines.push("@@ …".to_string());
            break;
        }
    }
    DiffOut { added, removed, lines }
}

/// `coucou_diff` for an Edit / MultiEdit / Write tool call, if it is one.
fn tool_diff(tool: &str, input: &serde_json::Value) -> Option<serde_json::Value> {
    let file = input.get("file_path").and_then(|v| v.as_str())?;
    let s = |k: &str| input.get(k).and_then(|v| v.as_str());
    let d = match tool {
        "Edit" => diff_text(s("old_string")?, s("new_string")?),
        "MultiEdit" => {
            let mut total = DiffOut { added: 0, removed: 0, lines: Vec::new() };
            for e in input.get("edits")?.as_array()? {
                let old = e.get("old_string").and_then(|v| v.as_str())?;
                let new = e.get("new_string").and_then(|v| v.as_str())?;
                let d = diff_text(old, new);
                total.added += d.added;
                total.removed += d.removed;
                if !total.lines.is_empty() {
                    total.lines.push("@@".into());
                }
                total.lines.extend(d.lines);
            }
            total.lines.truncate(MAX_DIFF_LINES);
            total
        }
        "Write" => {
            // PreToolUse fires before the write, so the file on disk is still the old one.
            let old = match std::fs::metadata(file) {
                Ok(m) if m.len() <= 2_000_000 => std::fs::read_to_string(file).unwrap_or_default(),
                _ => String::new(),
            };
            diff_text(&old, s("content")?)
        }
        _ => return None,
    };
    Some(serde_json::json!({
        "file": file, "added": d.added, "removed": d.removed, "lines": d.lines,
    }))
}

/// Reads stdin and returns the payload to forward plus the event name.
fn read_event() -> Option<Event> {
    let mut raw = Vec::new();
    if std::io::stdin().read_to_end(&mut raw).is_err() || raw.is_empty() {
        return None;
    }
    // Some shells hand us a UTF-8 BOM; serde_json would choke on it.
    if raw.starts_with(&[0xEF, 0xBB, 0xBF]) {
        raw.drain(..3);
    }

    let mut payload = serde_json::from_slice::<serde_json::Value>(&raw).ok()?;
    let map = payload.as_object_mut()?;

    // Parse argv: "coucou-hook.exe [--agent <name>] [<EventName>]"
    // --agent tags the payload with coucou_agent so the app routes to the right pill.
    // Absent or invalid names are validated and discarded by the app, not here.
    let mut agent = String::new();
    let mut arg_event = String::new();
    let mut ask_mode = false;
    {
        let mut it = std::env::args().skip(1);
        while let Some(arg) = it.next() {
            if arg == "--agent" {
                agent = it.next().unwrap_or_default();
            } else if arg == "--ask" {
                ask_mode = true;
            } else if arg_event.is_empty() {
                arg_event = arg;
            }
        }
    }
    // Which agent this hook was installed for. Absent means Claude Code,
    // so existing hook commands keep working unchanged.
    if !agent.is_empty() {
        map.insert("coucou_agent".into(), serde_json::Value::String(agent));
    }
    let event = map
        .get("hook_event_name")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .unwrap_or(arg_event);
    map.insert("hook_event_name".into(), serde_json::Value::String(event.clone()));

    for field in DROPPED_FIELDS {
        map.remove(*field);
    }

    let cwd_missing = map
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(str::is_empty)
        .unwrap_or(true);
    if cwd_missing {
        if let Ok(cwd) = std::env::current_dir() {
            map.insert(
                "cwd".into(),
                serde_json::Value::String(cwd.to_string_lossy().to_string()),
            );
        }
    }

    // Which terminal the session runs in. Unlike macOS, Coucou here accepts
    // events from every terminal, so this is context only — never a filter.
    for (key, var) in [
        ("term_program", "TERM_PROGRAM"),
        ("wt_session", "WT_SESSION"),
        ("term_session_id", "TERM_SESSION_ID"),
        ("vscode_pid", "VSCODE_PID"),
        ("session_pid", "CLAUDE_CODE_SSE_PORT"),
    ] {
        if !map.contains_key(key) {
            let value = std::env::var(var).unwrap_or_default();
            map.insert(key.into(), serde_json::Value::String(value));
        }
    }

    // Computed before the strings are cut: see the module docs.
    let tool = map.get("tool_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if let Some(input) = map.get("tool_input") {
        if let Some(d) = tool_diff(&tool, input) {
            map.insert("coucou_diff".into(), d);
        }
    }

    // `--ask` is its own PreToolUse hook entry, matched on AskUserQuestion by
    // Claude Code; the guard makes it harmless if it ever runs for something else.
    let mut ask = None;
    if ask_mode {
        if tool != "AskUserQuestion" {
            return None;
        }
        ask = map.get("tool_input").and_then(|i| i.get("questions")).cloned();
        ask.as_ref()?;
        map.insert("coucou_kind".into(), serde_json::Value::String("ask_user_question".into()));
    }

    truncate_strings(&mut payload);

    let mut line = payload.to_string();
    line.push('\n');
    Some(Event { line, name: event, ask })
}

/// Caps every string in the payload. A single Write can carry a whole file.
fn truncate_strings(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(s) => {
            if s.len() > MAX_FIELD_LEN {
                // Cut on a char boundary; a lone byte index can split UTF-8.
                let mut end = MAX_FIELD_LEN;
                while end > 0 && !s.is_char_boundary(end) {
                    end -= 1;
                }
                s.truncate(end);
                s.push('…');
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(truncate_strings),
        serde_json::Value::Object(map) => map.values_mut().for_each(truncate_strings),
        _ => {}
    }
}

/// Connect, send, and — for a permission request — wait for the island's word.
fn talk(payload: &str, waits_for_answer: bool) -> Option<String> {
    let mut pipe = connect()?;

    if pipe.write_all(payload.as_bytes()).is_err() {
        return None;
    }
    let _ = pipe.flush();

    if !waits_for_answer {
        return None;
    }

    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.contains(&b'\n') {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let answer = String::from_utf8_lossy(&buf).trim().to_string();
    (!answer.is_empty()).then_some(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_json_matches_the_documented_shape() {
        assert_eq!(
            decision_json("allow").unwrap(),
            r#"{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}"#
        );
        assert_eq!(
            decision_json("deny").unwrap(),
            r#"{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"deny","message":"Denied from Coucou"}}}"#
        );
        // "always" is an island concept; Claude Code just gets an allow.
        assert!(decision_json("always").unwrap().contains(r#""behavior":"allow""#));
    }

    #[test]
    fn the_status_line_shows_both_limits() {
        let l = serde_json::json!({ "five_hour": { "used_percentage": 35.4, "resets_at": 1 }, "seven_day": { "used_percentage": 54, "resets_at": 2 } });
        assert_eq!(status_text(&l), "5 jam 35% · minggu 54%");
        assert_eq!(status_text(&serde_json::json!({})), "");
    }

    #[test]
    fn an_answer_becomes_the_documented_updated_input() {
        let q = serde_json::json!([{ "question": "Which?", "options": [] }]);
        let out = ask_json(r#"{"permissionDecision":"answer","answers":{"Which?":"A"}}"#, &q).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "allow");
        assert_eq!(v["hookSpecificOutput"]["updatedInput"]["answers"]["Which?"], "A");
        assert_eq!(v["hookSpecificOutput"]["updatedInput"]["questions"], q);
        // "ask", garbage and a missing answers object all stay silent.
        assert!(ask_json(r#"{"permissionDecision":"ask"}"#, &q).is_none());
        assert!(ask_json("allow", &q).is_none());
        assert!(ask_json(r#"{"permissionDecision":"answer"}"#, &q).is_none());
    }

    #[test]
    fn an_edit_is_counted_line_by_line() {
        let input = serde_json::json!({
            "file_path": "src/a.rs",
            "old_string": "fn a() {\n    one();\n    two();\n}\n",
            "new_string": "fn a() {\n    one();\n    three();\n    four();\n}\n",
        });
        let d = tool_diff("Edit", &input).unwrap();
        assert_eq!(d["added"], 2);
        assert_eq!(d["removed"], 1);
        let lines: Vec<&str> = d["lines"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect();
        assert!(lines.contains(&"-    two();") && lines.contains(&"+    three();") && lines.contains(&" fn a() {"));
        // Other tools carry no diff.
        assert!(tool_diff("Bash", &serde_json::json!({ "command": "ls" })).is_none());
    }

    #[test]
    fn a_new_file_is_all_additions_and_an_untouched_one_has_no_changes() {
        let path = std::env::temp_dir().join(format!("coucou-hook-diff-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let input = serde_json::json!({ "file_path": path.to_string_lossy(), "content": "a\nb\nc\n" });
        let d = tool_diff("Write", &input).unwrap();
        assert_eq!((d["added"].as_u64(), d["removed"].as_u64()), (Some(3), Some(0)));

        std::fs::write(&path, "a\nb\nc\n").unwrap();
        let d = tool_diff("Write", &input).unwrap();
        assert_eq!((d["added"].as_u64(), d["removed"].as_u64()), (Some(0), Some(0)));

        std::fs::write(&path, "a\nX\nc\n").unwrap();
        let d = tool_diff("Write", &input).unwrap();
        assert_eq!((d["added"].as_u64(), d["removed"].as_u64()), (Some(1), Some(1)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_huge_rewrite_stays_bounded() {
        let old: String = (0..4000).map(|i| format!("old {i}\n")).collect();
        let new: String = (0..4000).map(|i| format!("new {i}\n")).collect();
        let d = diff_text(&old, &new);
        assert_eq!((d.added, d.removed), (4000, 4000));
        assert!(d.lines.len() <= MAX_DIFF_LINES + 1);
    }

    #[test]
    fn anything_unrecognised_prints_nothing() {
        assert!(decision_json("").is_none());
        assert!(decision_json("maybe").is_none());
        // The shape the app used to send must not be mistaken for a decision.
        assert!(decision_json(r#"{"permissionDecision":"allow"}"#).is_none());
    }

    #[test]
    fn long_strings_are_cut_on_a_char_boundary() {
        let mut v = serde_json::json!({ "tool_input": { "content": "é".repeat(4000) } });
        truncate_strings(&mut v);
        let s = v["tool_input"]["content"].as_str().unwrap();
        assert!(s.len() <= MAX_FIELD_LEN + 4);
        assert!(s.ends_with('…'));
    }
}
