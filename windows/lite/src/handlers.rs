// Claude Code hook events → island state (port of island/hooks.ts, itself
// HookServer.processEvent / processPermissionRequest from the macOS app) and
// integration updates → pills (island/integrations.ts).
//
// Unlike macOS there is no terminal filter: on Windows the hook fires from any
// terminal (Windows Terminal, VS Code, PowerShell…) and all of them are handled.

use serde_json::Value;

use crate::app::{App, Timer};
use crate::integrations::IntegrationUpdate;
use crate::layout::{Mode, View};
use crate::mochi::engine::BotState;
use crate::pipe;
use crate::sound;
use crate::state::{validate_agent, ApprovalInfo, PillBadge, CLAUDE_ID};

fn last_path_component(p: &str) -> String {
    let cleaned = p.trim_end_matches(['\\', '/']);
    cleaned.rsplit(['\\', '/']).next().unwrap_or(cleaned).to_string()
}

fn alias_project(name: &str) -> String {
    match name.to_lowercase().as_str() {
        "notch-buddy" | "notchbuddy" | "notch_buddy" => "Notch Buddy".to_string(),
        _ => name.to_string(),
    }
}

/// frenchStep() — same labels as the macOS app.
fn tool_label(tool: &str) -> &str {
    match tool {
        "Bash" | "PowerShell" => "Exécute",
        "Read" => "Lit",
        "Write" => "Écrit",
        "Edit" | "MultiEdit" => "Modifie",
        "Glob" => "Cherche",
        "Grep" => "Recherche",
        "WebSearch" => "Recherche web",
        "WebFetch" => "Récupère",
        "TodoWrite" => "Tâches",
        "Task" => "Agent",
        "LS" => "Liste",
        "NotebookEdit" => "Notebook",
        other => other,
    }
}

fn str_of<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input.get(key).and_then(Value::as_str)
}

fn take(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn step_label(tool: &str, input: &Value) -> String {
    let label = tool_label(tool);
    if let Some(cmd) = str_of(input, "command") {
        return format!("{label} · {}", take(cmd, 40));
    }
    if let Some(p) = str_of(input, "path") {
        return format!("{label} · {}", last_path_component(p));
    }
    if let Some(p) = str_of(input, "file_path") {
        return format!("{label} · {}", last_path_component(p));
    }
    if let Some(q) = str_of(input, "query") {
        return format!("{label} · {}", take(q, 40));
    }
    label.to_string()
}

/// What the Allow button actually authorises. Approving "Write" tells you nothing
/// — approving `Write · C:\…\.env` tells you everything.
fn approval_target(tool: &str, input: &Value) -> String {
    for field in ["command", "file_path", "path", "url", "query", "pattern", "prompt"] {
        if let Some(v) = str_of(input, field) {
            if !v.trim().is_empty() {
                return format!("{tool} · {}", v.trim());
            }
        }
    }
    tool.to_string()
}

impl App {
    pub fn handle_hook(&mut self, payload: Value) {
        let request_id = payload.get("request_id").and_then(Value::as_str).unwrap_or("").to_string();
        if self.st.paused {
            if !request_id.is_empty() {
                pipe::decline(&self.ctx, &request_id);
            }
            return;
        }

        let name = payload.get("hook_event_name").and_then(Value::as_str).unwrap_or("").to_string();
        // `{"hook_event_name":"CoucouTimer","minutes":25}` starts a countdown in the bar; 0 cancels.
        if name == "CoucouTimer" {
            let minutes = payload.get("minutes").and_then(Value::as_f64).unwrap_or(0.0) as f32;
            if minutes > 0.0 {
                crate::activity::start_timer(minutes);
            } else {
                crate::activity::cancel_timer();
            }
            self.ensure_running();
            return;
        }
        let cwd = payload.get("cwd").and_then(Value::as_str).unwrap_or("").to_string();
        let raw = last_path_component(&cwd);
        let project = alias_project(if raw.is_empty() { "Session" } else { &raw });
        // Route to the right pill: a valid `coucou_agent` gets its own agent_<name>
        // pill; absent or invalid means Claude Code, so existing hooks keep working.
        let external = payload.get("coucou_agent").and_then(Value::as_str).and_then(validate_agent);
        let is_external = external.is_some();
        let standing = external.as_deref().map(|n| self.st.settings.agent_pills.iter().any(|p| p == n)).unwrap_or(false);
        let home_name = external.as_deref().map(crate::state::agent_label).unwrap_or_else(|| "Claude Code".to_string());
        let target: &'static str = match &external {
            Some(name) => self.st.upsert_external_agent(name),
            None => CLAUDE_ID,
        };
        let focused = self.st.focus_id == Some(target);
        let message = payload.get("message").and_then(Value::as_str).unwrap_or("").to_string();
        let tool = payload.get("tool_name").and_then(Value::as_str).unwrap_or("Tool").to_string();
        let input = payload.get("tool_input").cloned().unwrap_or(Value::Null);

        let upsert = |app: &mut App| {
            if is_external {
                return; // agent pills keep the agent's own name
            }
            if let Some(t) = app.st.task_mut(target) {
                t.name = project.clone();
                if !cwd.is_empty() {
                    t.session_cwd = Some(cwd.clone());
                }
            }
        };

        match name.as_str() {
            "SessionStart" => {
                upsert(self);
                self.surface(View::Overview, false);
                sound::play("work");
            }
            "UserPromptSubmit" | "PreInvocation" => {
                upsert(self);
                self.st.update_task(target, BotState::Thinking);
                let asked = payload
                    .get("prompt")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| (!message.is_empty()).then(|| message.clone()));
                if let Some(a) = asked {
                    self.st.append_step(target, take(&a, 60));
                }
                self.surface(View::Overview, false);
            }
            "PreToolUse" => {
                upsert(self);
                self.st.update_task(target, BotState::Working);
                self.st.append_step(target, step_label(&tool, &input));
                self.surface(View::Overview, false);
            }
            "PostToolUse" | "PostInvocation" => self.st.update_task(target, BotState::Working),
            "PostToolUseFailure" => {
                self.st.update_task(target, BotState::Working);
                self.st.append_step(target, "⚠ failed".into());
            }
            "Notification" => {
                let lower = message.to_lowercase();
                if lower.contains("rate limit") || lower.contains("limite d") {
                    self.st.update_task(target, BotState::RateLimit);
                    sound::play("rate");
                } else if message.ends_with('?') {
                    self.st.update_task(target, BotState::Question);
                    self.st.append_step(target, message.clone());
                    // Claude is waiting on a human: show the question, as the card exists for.
                    if focused {
                        self.surface(View::Question, true);
                    }
                }
            }
            "Stop" => {
                self.st.update_task(target, BotState::Finished);
                if !message.is_empty() {
                    self.st.append_step(target, take(&message, 60));
                }
                sound::play("finish");
                if focused {
                    self.surface(View::Finished, true);
                } else {
                    self.st.set_pill_badge(target, Some(PillBadge::Finished));
                }
                if is_external && !standing {
                    self.later(5.2, Timer::RemoveTask(target));
                } else {
                    self.later(5.2, Timer::TaskIdle(target));
                }
            }
            "StopFailure" => {
                self.st.update_task(target, BotState::Error);
                sound::play("error");
                if focused {
                    self.surface(View::Error, true);
                } else {
                    self.st.set_pill_badge(target, Some(PillBadge::Error));
                }
            }
            "SessionEnd" if is_external && !standing => self.st.remove_task(target),
            "SessionEnd" => {
                self.st.update_task(target, BotState::Idle);
                if let Some(t) = self.st.task_mut(target) {
                    t.steps.clear();
                    t.step_index = 0;
                    t.name = home_name.clone();
                    t.pill_badge = None;
                }
            }
            "SubagentStart" => self.st.append_step(target, "+ subagent".into()),
            "SubagentStop" => self.st.append_step(target, "• subagent done".into()),
            "PermissionRequest" if is_external => {
                // An external agent gets no approval card — it would look like a Claude
                // Code request. Hand it back so the agent asks in its own terminal.
                if !request_id.is_empty() {
                    pipe::decline(&self.ctx, &request_id);
                }
            }
            "PermissionRequest" => {
                // One card, one request. A second one must never quietly replace the
                // first — that would leave a human staring at request B while
                // request A waits for a decision nobody can give.
                if let Some(p) = &self.st.pending_approval {
                    if p.request_id != request_id {
                        if !request_id.is_empty() {
                            pipe::decline(&self.ctx, &request_id);
                        }
                        return;
                    }
                }
                upsert(self);
                self.cancel_timer(|t| matches!(t, Timer::ApprovalTimeout));
                self.st.pending_approval = Some(ApprovalInfo {
                    request_id: request_id.clone(),
                    tool: tool.clone(),
                    command: approval_target(&tool, &input),
                });
                // The relay's short ack window closes in 800 ms; everything below
                // this line is synchronous, so the card really is up by then.
                if !request_id.is_empty() {
                    pipe::acknowledge(&self.ctx, &request_id);
                }
                self.st.update_task(target, BotState::Approval);
                self.st.is_pinned = true;
                sound::play("approval");
                if focused {
                    self.alert(View::Approval);
                } else {
                    // Another agent holds the view, so the card would yank it away.
                    // The badge is the signal instead.
                    self.st.set_pill_badge(target, Some(PillBadge::Approval));
                    self.reveal();
                }
                // Coucou answers within 108 s or not at all; after that the
                // terminal has taken over and the card would be lying.
                self.later(110.0, Timer::ApprovalTimeout);
            }
            _ => {}
        }
        self.ensure_running();
    }

    /// Alerts force the island open; work events only reveal the compact island.
    fn surface(&mut self, view: View, is_alert: bool) {
        if self.st.mode == Mode::Expanded {
            if is_alert {
                self.set_view(view);
            }
        } else if is_alert {
            self.alert(view);
        } else if self.st.mode == Mode::Hidden {
            self.reveal();
        }
    }

    pub fn approval_timeout(&mut self) {
        if self.st.pending_approval.is_none() {
            return;
        }
        self.st.pending_approval = None;
        self.st.is_pinned = false;
        self.drop_pin();
        self.st.update_task(CLAUDE_ID, BotState::Working);
        self.st.set_pill_badge(CLAUDE_ID, None);
        if self.st.view == View::Approval {
            let v = self.st.default_view();
            self.set_view(v);
        }
    }

    /// The Allow / Deny buttons.
    pub fn decide(&mut self, allow: bool) {
        let Some(req) = self.st.pending_approval.take() else { return };
        sound::play(if allow { "approve" } else { "blip" });
        pipe::answer(&self.ctx, &req.request_id, if allow { "allow" } else { "deny" });
        self.cancel_timer(|t| matches!(t, Timer::ApprovalTimeout));
        self.st.is_pinned = false;
        self.fsm.pinned = false;
        self.st.update_task(CLAUDE_ID, BotState::Working);
        self.st.set_pill_badge(CLAUDE_ID, None);
        let v = self.st.default_view();
        self.set_view(v);
    }

    // ── Integrations ──────────────────────────────────────────────────────────

    pub fn handle_integration(&mut self, update: IntegrationUpdate) {
        if self.st.paused {
            return;
        }
        let id = update.id;
        let previous = self.st.integrations.get(id).cloned().unwrap_or_default();
        let had_error = update.error.is_some();
        let info = crate::state::IntegrationInfo {
            data: if had_error { previous.data.clone() } else { update.data.clone() },
            error: update.error.clone(),
            loaded: if had_error { previous.loaded } else { true },
            configured: previous.configured || !had_error,
        };
        self.st.integrations.insert(id, info);

        if let Some(event) = update.event {
            let focus_id = self.st.focus_id;
            if let Some(task) = self.st.task_mut(id) {
                let state = if event.success { BotState::Finished } else { BotState::Error };
                task.state = state;
                task.mini.set_state(state, false);
                task.steps = match &event.detail {
                    Some(d) => vec![event.label.clone(), d.clone()],
                    None => vec![event.label.clone()],
                };
                task.step_index = task.steps.len() - 1;
                if focus_id != Some(id) {
                    task.pill_badge = Some(if event.success { PillBadge::Finished } else { PillBadge::Error });
                }
                sound::play(if event.success { "finish" } else { "error" });
                // Show the compact island so the badge is seen, but never steal the
                // screen for a successful deploy.
                self.reveal();
                self.cancel_timer(|t| matches!(t, Timer::IntegrationClear(i) if *i == id));
                self.later(60.0, Timer::IntegrationClear(id));
            }
        }
        self.ensure_running();
    }
}
