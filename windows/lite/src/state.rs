// App state — port of core/state.ts (the parts the island needs).

use std::collections::HashMap;

use serde_json::Value;

use crate::layout::{Mode, View};
use crate::mochi::engine::{BotEngine, BotState};
use crate::settings::Settings;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PillBadge {
    Approval,
    Finished,
    Error,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    ClaudeCode,
    N8n,
    /// A third-party agent that tags its hook payloads with `coucou_agent`.
    Agent,
}

pub struct Task {
    pub id: &'static str,
    pub name: String,
    pub color: &'static str,
    pub state: BotState,
    pub step_index: usize,
    pub steps: Vec<String>,
    pub source: Source,
    pub pill_badge: Option<PillBadge>,
    pub session_cwd: Option<String>,
    /// The little Mochi shown on this pill.
    pub mini: BotEngine,
}

impl Task {
    fn new(id: &'static str, name: &str, color: &'static str, source: Source) -> Self {
        let mut mini = BotEngine::new();
        mini.is_mini = true;
        mini.body_color = Some(crate::mochi::engine::hex_to_rgb(color));
        Self {
            id,
            name: name.to_string(),
            color,
            state: BotState::Idle,
            step_index: 0,
            steps: Vec::new(),
            source,
            pill_badge: None,
            session_cwd: None,
            mini,
        }
    }
}

pub const CLAUDE_ID: &str = "integration_claude";

/// Same palette as the web island's `agentColor`.
const AGENT_COLORS: [&str; 4] = ["#22C55E", "#EAB308", "#60A5FA", "#E879F9"];

/// Task ids are `&'static str`; dynamic agent ids are leaked once per distinct
/// name (a handful, ever) so every other API can stay copy-cheap.
fn intern(s: &str) -> &'static str {
    use std::cell::RefCell;
    thread_local! {
        static NAMES: RefCell<HashMap<String, &'static str>> = RefCell::new(HashMap::new());
    }
    NAMES.with(|n| {
        *n.borrow_mut()
            .entry(s.to_string())
            .or_insert_with(|| Box::leak(s.to_string().into_boxed_str()))
    })
}

/// "claude" is reserved; lowercase, digits and hyphens, at most 24 characters.
/// Same rule as `HookServer.validateAgent` on macOS.
pub fn validate_agent(raw: &str) -> Option<String> {
    if raw.is_empty() || raw.len() > 24 || raw == "claude" {
        return None;
    }
    raw.bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        .then(|| raw.to_string())
}

fn agent_color(name: &str) -> &'static str {
    let mut h: i32 = 0;
    for c in name.chars() {
        h = h.wrapping_mul(31).wrapping_add(c as i32);
    }
    AGENT_COLORS[(h.unsigned_abs() as usize) % AGENT_COLORS.len()]
}

/// AgentTask.integrationAgents — same ids, names and colours as macOS.
pub const INTEGRATIONS: &[(&str, &str, &str, Source)] = &[
    (CLAUDE_ID, "Claude Code", "#F5F6F8", Source::ClaudeCode),
    ("integration_resend", "Resend", "#22C55E", Source::N8n),
    ("integration_n8n", "n8n", "#F29B38", Source::N8n),
    ("integration_vercel", "Vercel", "#7C5CFF", Source::N8n),
    ("integration_github", "GitHub", "#F4505E", Source::N8n),
    ("integration_notion", "Notion", "#8C8C8C", Source::N8n),
    ("integration_calcom", "Cal.com", "#C9956A", Source::N8n),
    ("integration_stripe", "Stripe", "#0570DE", Source::N8n),
];

#[derive(Clone, Debug)]
pub struct ApprovalInfo {
    pub request_id: String,
    pub tool: String,
    pub command: String,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

#[derive(Clone)]
pub struct DroppedInfo {
    pub name: String,
    pub path: String,
}

#[derive(Default, Clone)]
pub struct IntegrationInfo {
    pub data: Value,
    pub error: Option<String>,
    pub loaded: bool,
    pub configured: bool,
}

pub struct State {
    pub mode: Mode,
    pub view: View,
    pub tasks: Vec<Task>,
    pub focus_id: Option<&'static str>,
    pub state_override: Option<BotState>,

    /// Cursor in logical window pixels (origin: top-left of the 720×320 window).
    pub mouse: (f32, f32),
    /// Cursor relative to the island's top-left corner.
    pub mouse_in_island: (f32, f32),

    pub is_pinned: bool,
    pub paused: bool,

    pub upload_progress: f32,
    pub upload_duration: f32,
    pub file_drag_over: bool,

    pub dropped_file: Option<DroppedInfo>,
    pub note_message: Option<String>,
    pub chat_history: Vec<ChatMessage>,
    pub pending_approval: Option<ApprovalInfo>,
    pub integrations: HashMap<&'static str, IntegrationInfo>,

    pub settings: Settings,
    pub hooks_installed: bool,
    pub has_api_key: bool,
}

impl State {
    pub fn new(settings: Settings) -> Self {
        let mut s = Self {
            mode: Mode::Hidden,
            view: View::Overview,
            tasks: Vec::new(),
            focus_id: None,
            state_override: None,
            mouse: (0.0, 0.0),
            mouse_in_island: (0.0, 0.0),
            is_pinned: false,
            paused: false,
            upload_progress: 0.0,
            upload_duration: 2.4,
            file_drag_over: false,
            dropped_file: None,
            note_message: None,
            chat_history: Vec::new(),
            pending_approval: None,
            integrations: HashMap::new(),
            hooks_installed: settings.hooks_installed,
            settings,
            has_api_key: false,
        };
        s.load_integration_tasks();
        s
    }

    pub fn focus_task(&self) -> Option<&Task> {
        self.focus_id
            .and_then(|id| self.tasks.iter().find(|t| t.id == id))
            .or_else(|| self.tasks.first())
    }

    pub fn task_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn effective_state(&self) -> BotState {
        self.state_override
            .or_else(|| self.focus_task().map(|t| t.state))
            .unwrap_or(BotState::Idle)
    }

    pub fn other_tasks(&self) -> Vec<&Task> {
        let focus = self.focus_task().map(|t| t.id);
        self.tasks.iter().filter(|t| Some(t.id) != focus).collect()
    }

    pub fn set_focus(&mut self, id: &'static str) {
        if let Some(t) = self.task_mut(id) {
            t.pill_badge = None;
            self.focus_id = Some(id);
        }
    }

    pub fn update_task(&mut self, id: &str, state: BotState) {
        if let Some(t) = self.task_mut(id) {
            t.state = state;
            t.mini.set_state(state, false);
        }
    }

    pub fn append_step(&mut self, id: &str, step: String) {
        if let Some(t) = self.task_mut(id) {
            t.steps.push(step);
            if t.steps.len() > 20 {
                t.steps.remove(0);
            }
            t.step_index = t.steps.len() - 1;
        }
    }

    pub fn set_pill_badge(&mut self, id: &str, badge: Option<PillBadge>) {
        if let Some(t) = self.task_mut(id) {
            t.pill_badge = badge;
        }
    }

    /// loadIntegrationTasks() — Claude Code always on, the rest opt-in (max 4).
    pub fn load_integration_tasks(&mut self) {
        for (id, name, color, source) in INTEGRATIONS {
            let should = *id == CLAUDE_ID || self.settings.active_integrations.iter().any(|x| x == id);
            let present = self.tasks.iter().any(|t| t.id == *id);
            if should && !present {
                self.tasks.push(Task::new(id, name, color, *source));
            }
            if !should && present {
                self.tasks.retain(|t| t.id != *id);
            }
        }
        // Claude Code first, then agent pills (so they land in the visible four),
        // then the other integrations in declaration order — pills never shuffle.
        let order = |id: &str| INTEGRATIONS.iter().position(|(i, ..)| *i == id).unwrap_or(99);
        let rank = |id: &str| -> (u8, usize) {
            if id == CLAUDE_ID {
                (0, 0)
            } else if id.starts_with("agent_") {
                (1, 0)
            } else {
                (2, order(id))
            }
        };
        self.tasks.sort_by_key(|t| rank(t.id));
        if self.focus_id.is_none() || !self.tasks.iter().any(|t| Some(t.id) == self.focus_id) {
            self.focus_id = Some(CLAUDE_ID);
        }
    }

    /// Creates the `agent_<name>` pill on its first event and returns its id.
    pub fn upsert_external_agent(&mut self, name: &str) -> &'static str {
        let id = intern(&format!("agent_{name}"));
        if !self.tasks.iter().any(|t| t.id == id) {
            let at = self.tasks.iter().position(|t| t.id == CLAUDE_ID).map(|i| i + 1).unwrap_or(0);
            self.tasks.insert(at, Task::new(id, name, agent_color(name), Source::Agent));
        }
        id
    }

    pub fn remove_task(&mut self, id: &str) {
        self.tasks.retain(|t| t.id != id);
        if self.focus_id == Some(id) || !self.tasks.iter().any(|t| Some(t.id) == self.focus_id) {
            self.focus_id = Some(self.tasks.first().map(|t| t.id).unwrap_or(CLAUDE_ID));
        }
    }

    pub fn default_view(&self) -> View {
        // Docked: the dashboard is home unless a session is actually doing something.
        let active = self.tasks.iter().any(|t| t.state != BotState::Idle || !t.steps.is_empty());
        if self.settings.stay_visible && !active {
            View::Dashboard
        } else if self.tasks.is_empty() {
            View::Empty
        } else {
            View::Overview
        }
    }

    pub fn integration(&self, id: &str) -> Option<&IntegrationInfo> {
        self.integrations.get(id)
    }
}
