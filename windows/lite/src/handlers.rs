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
use crate::state::{
    validate_agent, ApprovalInfo, AskOption, AskQuestion, PendingQuestion, PillBadge, StepDiff, CLAUDE_ID,
};

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

/// `coucou_diff` from coucou-hook, when the edit actually changes something.
fn parse_diff(payload: &Value) -> Option<StepDiff> {
    let d = payload.get("coucou_diff")?;
    let added = d.get("added").and_then(Value::as_u64).unwrap_or(0) as u32;
    let removed = d.get("removed").and_then(Value::as_u64).unwrap_or(0) as u32;
    if added == 0 && removed == 0 {
        return None;
    }
    Some(StepDiff {
        file: d.get("file").and_then(Value::as_str).unwrap_or("").to_string(),
        added,
        removed,
        lines: d
            .get("lines")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|l| l.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
    })
}

/// The `questions` of an AskUserQuestion call. Anything cut by the relay's string
/// limit would no longer match the key Claude Code expects, so it is not offered.
fn parse_questions(input: &Value) -> Vec<AskQuestion> {
    let mut out = Vec::new();
    for q in input.get("questions").and_then(Value::as_array).into_iter().flatten().take(4) {
        let question = str_of(q, "question").unwrap_or("").trim().to_string();
        if question.is_empty() || question.chars().count() >= 1500 {
            return Vec::new();
        }
        let options: Vec<AskOption> = q
            .get("options")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(4)
            .filter_map(|o| {
                let label = str_of(o, "label")?.trim().to_string();
                (!label.is_empty()).then(|| AskOption {
                    label,
                    description: str_of(o, "description").unwrap_or("").trim().to_string(),
                })
            })
            .collect();
        out.push(AskQuestion {
            header: str_of(q, "header").unwrap_or("").trim().to_string(),
            question,
            options,
            multi: q.get("multiSelect").and_then(Value::as_bool).unwrap_or(false),
        });
    }
    out
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
        self.note_activity();
        if payload.get("coucou_agent").is_none() {
            self.buddy_session_event(&name);
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
        let is_ask = payload.get("coucou_kind").and_then(Value::as_str) == Some("ask_user_question");

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
            // The generic hook also sees AskUserQuestion; the dedicated one speaks for it.
            "PreToolUse" if tool == "AskUserQuestion" && !is_ask => {}
            "PreToolUse" if is_ask => {
                let questions = parse_questions(&input);
                let busy = self.st.pending_approval.is_some() || self.st.pending_question.is_some();
                if is_external || request_id.is_empty() || questions.is_empty() || busy {
                    // Nobody can answer it here: the terminal asks, as if we were not installed.
                    if !request_id.is_empty() {
                        pipe::decline(&self.ctx, &request_id);
                    }
                    return;
                }
                upsert(self);
                self.cancel_timer(|t| matches!(t, Timer::QuestionTimeout));
                let first = questions[0].clone();
                let picked = vec![false; first.options.len()];
                self.st.pending_question = Some(PendingQuestion {
                    request_id: request_id.clone(),
                    questions,
                    index: 0,
                    answers: Vec::new(),
                    picked,
                });
                self.ask_input.clear();
                pipe::acknowledge(&self.ctx, &request_id);
                self.st.update_task(target, BotState::Question);
                let label = if first.header.is_empty() { take(&first.question, 40) } else { take(&first.header, 40) };
                self.st.append_step(target, format!("Bertanya · {label}"));
                self.st.is_pinned = true;
                sound::play("approval");
                if focused {
                    self.alert(View::Question);
                } else {
                    self.st.set_pill_badge(target, Some(PillBadge::Approval));
                    self.reveal();
                }
                self.later(122.0, Timer::QuestionTimeout);
            }
            "PreToolUse" => {
                upsert(self);
                self.st.update_task(target, BotState::Working);
                let label = step_label(&tool, &input);
                match parse_diff(&payload) {
                    Some(d) => self.st.append_step_diff(target, label, d),
                    None => self.st.append_step(target, label),
                }
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
                self.drop_question();
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

    /// Forgets a question whose hook is gone (the session stopped under it).
    fn drop_question(&mut self) {
        if self.st.pending_question.take().is_some() {
            self.cancel_timer(|t| matches!(t, Timer::QuestionTimeout));
            self.st.is_pinned = false;
            self.fsm.pinned = false;
            self.ask_input.focused = false;
        }
    }

    pub fn question_timeout(&mut self) {
        if self.st.pending_question.is_none() {
            return;
        }
        self.st.pending_question = None;
        self.finish_question();
    }

    /// After a question is answered, handed back or timed out: back to work.
    fn finish_question(&mut self) {
        self.cancel_timer(|t| matches!(t, Timer::QuestionTimeout));
        self.ask_input.clear();
        self.ask_input.focused = false;
        self.platform.set_activating(false);
        self.st.is_pinned = false;
        self.fsm.pinned = false;
        self.st.update_task(CLAUDE_ID, BotState::Working);
        self.st.set_pill_badge(CLAUDE_ID, None);
        if self.st.view == View::Question {
            let v = self.st.default_view();
            self.set_view(v);
        }
    }

    /// Records the answer to the current question and moves on, or sends them all.
    pub fn answer_question(&mut self, text: String) {
        let Some(p) = self.st.pending_question.as_mut() else { return };
        let q = p.questions.get(p.index).map(|q| q.question.clone()).unwrap_or_default();
        p.answers.push((q, text));
        p.index += 1;
        self.ask_input.clear();
        if let Some(next) = p.questions.get(p.index) {
            p.picked = vec![false; next.options.len()];
            sound::play("blip");
            return;
        }
        let Some(p) = self.st.pending_question.take() else { return };
        let mut answers = serde_json::Map::new();
        for (k, v) in p.answers {
            answers.insert(k, Value::String(v));
        }
        let reply = serde_json::json!({ "permissionDecision": "answer", "answers": answers }).to_string();
        sound::play("approve");
        pipe::answer_raw(&self.ctx, &p.request_id, &reply);
        self.finish_question();
    }

    /// Ticks or unticks one option of a multi-select question.
    pub fn toggle_option(&mut self, i: usize) {
        if let Some(p) = self.st.pending_question.as_mut() {
            if let Some(b) = p.picked.get_mut(i) {
                *b = !*b;
            }
        }
    }

    /// "Kirim": the typed text if there is any, otherwise the ticked options.
    pub fn submit_question_text(&mut self) {
        let typed = self.ask_input.text().trim().to_string();
        if !typed.is_empty() {
            self.answer_question(typed);
            return;
        }
        let picks: Vec<String> = match self.st.pending_question.as_ref() {
            Some(p) => p
                .questions
                .get(p.index)
                .map(|q| {
                    q.options
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| p.picked.get(*i).copied().unwrap_or(false))
                        .map(|(_, o)| o.label.clone())
                        .collect()
                })
                .unwrap_or_default(),
            None => return,
        };
        if !picks.is_empty() {
            self.answer_question(picks.join(", "));
        }
    }

    /// "Di terminal": say nothing, and Claude Code asks there as usual.
    pub fn hand_back_question(&mut self) {
        let Some(p) = self.st.pending_question.take() else { return };
        pipe::answer_raw(&self.ctx, &p.request_id, r#"{"permissionDecision":"ask"}"#);
        sound::play("blip");
        self.finish_question();
    }

    /// Opens the diff of step `step` of the focused session.
    pub fn open_diff(&mut self, step: usize) {
        let d = self.st.focus_task().and_then(|t| t.diffs.get(step).cloned().flatten());
        if let Some(d) = d {
            self.diff_view = Some(d);
            self.diff_scroll = 0.0;
            sound::play("blip");
            self.set_view(View::Diff);
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
