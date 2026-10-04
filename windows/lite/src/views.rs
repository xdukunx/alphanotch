// Island views — native ports of views/views.ts (itself IslandViewContent.swift).
// Paddings, font sizes, colours and wording are copied from the web version so
// all of them read identically.

pub use crate::ticker::Ticker;

use tiny_skia::Color;

use crate::anim::{clamp, now};
use crate::app::App;
use crate::gfx::{hex, rgba, with_alpha, Gfx};
use crate::icons::Icon;
use crate::layout::{View, Wash};
use crate::mochi::engine::BotState;
use crate::state::{Role, Source, Task, CLAUDE_ID};
use crate::text::{self, Align, Face};
use crate::textfield::TextField;
use crate::ui::{self, id_of, lighten, pal, Rect};

// ── Chat state ────────────────────────────────────────────────────────────────

pub struct ChatUi {
    pub input: TextField,
    pub scroll: f32,
    pub scroll_to_end: bool,
    pub sending: bool,
    max_scroll: f32,
}

impl ChatUi {
    pub fn new() -> Self {
        Self {
            input: TextField::new("Ask me anything…", false),
            scroll: 0.0,
            scroll_to_end: false,
            sending: false,
            max_scroll: 0.0,
        }
    }

    pub fn on_mouse_down(&mut self, _x: f32) {}
}

impl Default for ChatUi {
    fn default() -> Self {
        Self::new()
    }
}

// ── Small drawing helpers ─────────────────────────────────────────────────────

fn t(g: &mut Gfx, s: &str, x: f32, cy: f32, face: Face, size: f32, color: Color) -> f32 {
    text::draw(g, s, x, cy, face, size, color, Align::Left);
    text::measure(s, face, size)
}

/// AgentWho — coloured dot + task name + grey label.
fn agent_who(g: &mut Gfx, x: f32, cy: f32, task: Option<&Task>, label: &str, max_w: f32) {
    let mut x = x;
    if let Some(task) = task {
        ui::dot(g, x + 4.0, cy, 8.0, hex(task.color));
        x += 8.0 + 7.0;
        let name = text::ellipsize(&task.name, Face::Medium, 12.0, max_w * 0.6);
        let w = t(g, &name, x, cy, Face::Medium, 12.0, hex(pal::INK));
        x += w + 7.0;
    }
    t(g, label, x, cy, Face::Regular, 12.0, hex(pal::DIM2));
}

/// Vertical centring of a column of items inside a card (`.stack`).
fn stack_top(card: Rect, heights: &[f32], gap: f32) -> f32 {
    let total: f32 = heights.iter().sum::<f32>() + gap * (heights.len().saturating_sub(1)) as f32;
    card.y + (card.h - total) / 2.0
}

impl App {
    // ── Entry point ───────────────────────────────────────────────────────────

    pub fn draw_content(&mut self, g: &mut Gfx, iw: f32, ih: f32, n: f32) {
        self.ui.begin_frame();
        let confused = self.st.view == View::Confused;
        g.save();
        g.set_alpha(if confused { 0.0 } else { 1.0 });
        self.draw_header(g, iw);
        g.restore();

        let v = Rect::new(10.0, 42.0, iw - 20.0, (ih - 52.0).max(0.0));
        if v.h < 20.0 || self.upload_active() {
            // While the drop sequence runs it paints the whole island body; only
            // the header stays on top of it.
            return;
        }
        let (alpha, scale) = self.view_alpha_scale();
        g.save();
        g.translate(v.cx(), v.cy());
        g.scale_xy(scale, scale);
        g.translate(-v.cx(), -v.cy());
        g.mul_alpha(alpha);

        // A view that is still fading in must not eat a click meant for the old one.
        let live = alpha > 0.55;
        let saved = self.ui.input;
        if !live {
            self.ui.input.pressed = false;
            self.ui.input.released = false;
        }
        match self.st.view {
            View::Overview => self.draw_overview(g, v, n),
            View::Empty => self.draw_empty(g, v),
            View::Approval => self.draw_approval(g, v),
            View::Question => self.draw_question(g, v),
            View::Error => self.draw_error(g, v),
            View::Finished => self.draw_finished(g, v),
            View::Confused => self.draw_confused(g, v),
            View::Note => self.draw_note(g, v),
            View::Settings => self.draw_settings(g, v),
            View::Prompt => self.draw_prompt(g, v),
            View::Upload => self.draw_drop_zone(g, v, n),
            View::Uploading => self.draw_uploading_fallback(g, v),
            View::Choose => self.draw_choose_fallback(g, v),
            View::Greeting => {}
            View::Dashboard => self.draw_dashboard(g, v, n),
            View::Stocks => self.draw_stocks(g, v),
            View::Weather => self.draw_weather(g, v),
            View::Teleprompter => self.draw_teleprompter(g, v),
        }
        if !live {
            self.ui.input = saved;
        }
        g.restore();

        // A small arrow at the bottom centre folds the island back to the bar.
        if !matches!(self.st.view, View::Greeting | View::Upload | View::Uploading | View::Choose | View::Confused) {
            let hit = Rect::new(iw / 2.0 - 22.0, ih - 15.0, 44.0, 15.0);
            let (clicked, hover, _) = self.ui.click_region(id_of("collapse-arrow", 103), hit);
            g.stroke_style(rgba(255, 255, 255, if hover { 0.9 } else { 0.32 }));
            g.line_width(1.6);
            g.line_cap_round();
            g.line_join_round();
            g.begin_path();
            g.move_to(iw / 2.0 - 6.0, ih - 5.0);
            g.line_to(iw / 2.0, ih - 9.0);
            g.line_to(iw / 2.0 + 6.0, ih - 5.0);
            g.stroke();
            if clicked {
                crate::sound::play("close");
                self.collapse_req = true;
            }
        }
    }

    // ── Header ────────────────────────────────────────────────────────────────

    fn draw_header(&mut self, g: &mut Gfx, iw: f32) {
        let cy = 25.0;
        let tabs = [
            (Icon::House, "tab-home", View::Overview),
            (Icon::Bubble, "tab-chat", View::Prompt),
            (Icon::Plus, "tab-drop", View::Upload),
            (Icon::Stack, "tab-dash", View::Dashboard),
            (Icon::Chart, "tab-stocks", View::Stocks),
            (Icon::Cloud, "tab-weather", View::Weather),
            (Icon::Lines, "tab-prompter", View::Teleprompter),
        ];
        let v = self.st.view;
        for (i, (icon, id, target)) in tabs.iter().enumerate() {
            let r = Rect::new(14.0 + i as f32 * 35.0, cy - 11.0, 30.0, 22.0);
            let on = match target {
                View::Overview => v == View::Overview || v == View::Empty,
                other => v == *other,
            };
            if self.ui.header_icon(g, id, r, *icon, 13.0, on, true) {
                crate::sound::play("blip");
                let go = if *target == View::Overview { self.st.default_view() } else { *target };
                self.set_view(go);
            }
        }

        let gear = Rect::new(iw - 51.0 - 8.0, cy - 8.0, 16.0, 16.0);
        let on = v == View::Settings;
        if self.ui.header_icon(g, "gear", gear, if on { Icon::GearFill } else { Icon::Gear }, 14.0, on, false) {
            crate::sound::play("blip");
            self.set_view(View::Settings);
        }
        let sound = Rect::new(iw - 23.0 - 8.0, cy - 8.0, 16.0, 16.0);
        let icon = if self.st.settings.sound_enabled { Icon::SpeakerOn } else { Icon::SpeakerOff };
        if self.ui.header_icon(g, "sound", sound, icon, 14.0, false, false) {
            self.st.settings.sound_enabled = !self.st.settings.sound_enabled;
            crate::sound::set_enabled(self.st.settings.sound_enabled);
            self.save_settings();
        }
    }

    // ── Overview ──────────────────────────────────────────────────────────────

    fn draw_overview(&mut self, g: &mut Gfx, v: Rect, n: f32) {
        let left = Rect::new(v.x, v.y, 322.0, v.h);
        let right = Rect::new(v.x + 332.0, v.y, v.w - 332.0, v.h);
        ui::card(g, left, Wash::None, false);
        ui::card(g, right, Wash::None, false);

        // Left: the live ticker for a Claude Code session, otherwise the pill's own card.
        let Some(task) = self.st.focus_task() else { return };
        let id = task.id;
        let session_active = (id == CLAUDE_ID || task.source == Source::Agent)
            && (task.state != BotState::Idle || !task.steps.is_empty());
        let mut jump_visible = true;

        if session_active {
            let (name, color, source_label, count) = (
                task.name.clone(),
                task.color,
                match task.source {
                    Source::ClaudeCode => "Claude Code",
                    Source::N8n => "n8n",
                    Source::Agent => "Agent",
                },
                if task.steps.len() > 1 {
                    Some(format!("{}/{}", (task.step_index + 1).min(task.steps.len()), task.steps.len()))
                } else {
                    None
                },
            );
            let cy = left.y + 18.0;
            let mut x = left.x + 108.0;
            ui::dot(g, x + 3.5, cy, 7.0, hex(color));
            x += 7.0 + 6.0;
            let max = left.w - 108.0 - 36.0 - 70.0;
            let nm = text::ellipsize(&name, Face::Medium, 12.0, max);
            x += t(g, &nm, x, cy, Face::Medium, 12.0, hex(pal::INK)) + 6.0;
            t(g, source_label, x, cy, Face::Regular, 11.0, hex(pal::DIM2));
            if let Some(c) = count {
                text::draw(g, &c, left.x + left.w - 36.0, cy, Face::Regular, 11.0, hex(pal::DIM3), Align::Right);
            }
            self.ticker.draw(g, left.x + 108.0, left.y + 28.0, left.w - 108.0 - 12.0, n);
        } else {
            jump_visible = !self.detail_open;
            self.draw_integration_card(g, left);
        }

        if jump_visible && self.ui.icon_button(g, "jump", left.x + left.w - 18.0, left.y + 16.0, 16.0, Icon::ArrowUpRight, 8.0) {
            self.open_target();
        }

        // Right: the other pills.
        let others: Vec<&'static str> = self.st.other_tasks().iter().take(4).map(|t| t.id).collect();
        let rows = others.len().div_ceil(2) as f32;
        if rows == 0.0 {
            return;
        }
        let block_h = rows * 28.0 + (rows - 1.0) * 4.0;
        let top = right.y + (right.h - block_h) / 2.0;
        let col_w = (right.w - 16.0 - 4.0) / 2.0;
        for (i, id) in others.iter().enumerate() {
            let (col, row) = ((i % 2) as f32, (i / 2) as f32);
            let r = Rect::new(right.x + 8.0 + col * (col_w + 4.0), top + row * 32.0, col_w, 28.0);
            if self.draw_pill(g, r, id) {
                crate::sound::play("blip");
                if let Some(t) = self.st.tasks.iter().find(|t| t.id == *id) {
                    let id = t.id;
                    self.st.set_focus(id);
                    self.detail_open = false;
                }
            }
        }
    }

    fn draw_pill(&mut self, g: &mut Gfx, r: Rect, id: &'static str) -> bool {
        let (clicked, hover, _) = self.ui.click_region(id_of(id, 41), r);
        let Some(task) = self.st.tasks.iter_mut().find(|t| t.id == id) else { return false };
        let color = task.color;
        let label = if task.id == CLAUDE_ID { "Claude Code".to_string() } else { task.name.clone() };
        let badge = task.pill_badge;

        g.save();
        if hover {
            g.translate(r.cx(), r.cy());
            g.scale_xy(1.04, 1.04);
            g.translate(-r.cx(), -r.cy());
        }
        if hover {
            // soft glow under the pill
            g.fill_style(with_alpha(hex(color), 0.35));
            g.fill_round_rect(r.x - 1.0, r.y + 1.0, r.w + 2.0, r.h + 2.0, 15.0);
        }
        g.fill_style(if hover { with_alpha(hex(color), 0.18) } else { hex(pal::CARD_FLAT) });
        g.fill_round_rect(r.x, r.y, r.w, r.h, 14.0);
        if hover {
            g.fill_style(hex(pal::CARD_FLAT));
            g.fill_round_rect(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0, 13.0);
            g.fill_style(with_alpha(hex(color), 0.18));
            g.fill_round_rect(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0, 13.0);
        }
        g.stroke_style(with_alpha(hex(color), if hover { 0.55 } else { 0.14 }));
        g.line_width(1.0);
        g.round_rect(r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0, 13.5);
        g.stroke();

        // The little Mochi (24 px body, engine canvas is body/0.6).
        let eng = 24.0 / 0.6;
        g.save();
        g.translate(r.x + 7.0 + 12.0 - eng / 2.0, r.cy() - eng / 2.0);
        task.mini.draw(g, eng, eng);
        g.restore();

        let col = if hover { lighten(color, 0.3) } else { hex(pal::DIM3) };
        let l = text::ellipsize(&label, Face::Medium, 10.0, r.w - 48.0);
        text::draw(g, &l, r.cx() + 8.0, r.cy(), Face::Medium, 10.0, col, Align::Center);

        if let Some(b) = badge {
            let (c, icon) = match b {
                crate::state::PillBadge::Approval => ("#F5A524", Icon::Bang),
                crate::state::PillBadge::Finished => ("#22C55E", Icon::Check),
                crate::state::PillBadge::Error => ("#F4505E", Icon::Xmark),
            };
            let (bx, by) = (r.x + r.w - 3.0 - 7.0, r.y - 3.0 + 7.0);
            ui::dot(g, bx, by, 14.0, hex("#0B0C0E"));
            ui::dot(g, bx, by, 12.0, hex(c));
            if b == crate::state::PillBadge::Finished {
                crate::icons::stroke(g, icon, bx, by, 6.0, 3.0, Color::BLACK);
            } else {
                crate::icons::fill(g, icon, bx, by, 8.0, Color::BLACK);
            }
        }
        g.restore();
        clicked
    }

    /// The ↗ button — same targets as openAgentTarget() on macOS.
    pub fn open_target(&mut self) {
        let Some(task) = self.st.focus_task() else { return };
        let id = task.id;
        let cwd = task.session_cwd.clone();
        match id {
            CLAUDE_ID => {
                open_in_vscode(cwd.as_deref());
            }
            "integration_n8n" => {
                if let Some(url) = crate::secrets::get("n8n-url") {
                    open_url(&url);
                }
            }
            _ => {
                if let Some(u) = open_url_for(id) {
                    open_url(u);
                }
            }
        }
    }

    // ── Empty ─────────────────────────────────────────────────────────────────

    fn draw_empty(&mut self, g: &mut Gfx, v: Rect) {
        ui::card(g, v, Wash::None, false);
        let x = v.x + 118.0;
        t(g, "Nothing running right now.", x, v.cy() - 11.0, Face::Medium, 15.0, hex(pal::INK));
        t(g, "Drop a file or window, or ask me anything.", x, v.cy() + 11.0, Face::Regular, 13.0, hex(pal::DIM));
        let w = text::measure("Ask Claude", Face::Medium, 12.5) + 26.0;
        let (clicked, _) = self.ui.button(g, v.x + v.w - 18.0 - w, v.cy() - 15.0, "Ask Claude", true, None);
        if clicked {
            self.set_view(View::Prompt);
        }
    }

    // ── Approval ──────────────────────────────────────────────────────────────

    fn draw_approval(&mut self, g: &mut Gfx, v: Rect) {
        ui::card(g, v, Wash::Amber, false);
        let x = v.x + 116.0;
        let w = v.w - 116.0 - 16.0;
        let top = stack_top(v, &[16.0, 29.0, 30.0], 5.0);
        agent_who(g, x, top + 8.0, self.st.focus_task(), "needs permission", w);

        // The whole point of approving here rather than in the terminal: this
        // line is the command, the file path or the URL being authorised.
        let command = self
            .st
            .pending_approval
            .as_ref()
            .map(|p| if p.command.is_empty() { p.tool.clone() } else { p.command.clone() })
            .unwrap_or_else(|| "…".into());
        let cy = top + 16.0 + 5.0;
        g.fill_style(rgba(255, 255, 255, 0.07));
        g.fill_round_rect(x, cy, w, 29.0, 10.0);
        g.stroke_style(rgba(255, 255, 255, 0.06));
        g.line_width(1.0);
        g.round_rect(x + 0.5, cy + 0.5, w - 1.0, 28.0, 9.5);
        g.stroke();
        let line = text::ellipsize(&command.replace('\n', " "), Face::Mono, 12.0, w - 20.0);
        t(g, &line, x + 10.0, cy + 14.5, Face::Mono, 12.0, hex("#E8E9EC"));

        let by = cy + 29.0 + 5.0;
        let (deny, dw) = self.ui.button(g, x, by, "Deny", false, Some("N"));
        let (allow, _) = self.ui.button(g, x + dw + 8.0, by, "Allow", true, Some("Y"));
        if deny {
            self.decide(false);
        } else if allow {
            self.decide(true);
        }
    }

    // ── Question ──────────────────────────────────────────────────────────────

    fn draw_question(&mut self, g: &mut Gfx, v: Rect) {
        ui::card(g, v, Wash::Cyan, false);
        let x = v.x + 116.0;
        let w = v.w - 116.0 - 16.0;
        let top = stack_top(v, &[16.0, 20.0, 18.0], 5.0);
        let task = self.st.focus_task();
        agent_who(g, x, top + 8.0, task, "Claude Code is asking a question", w);
        let title = task.and_then(|t| t.steps.last().cloned()).unwrap_or_else(|| "Claude needs an answer.".into());
        let title = text::ellipsize(&title, Face::Medium, 15.0, w);
        t(g, &title, x, top + 16.0 + 5.0 + 10.0, Face::Medium, 15.0, hex(pal::INK));
        t(g, "Answer in your terminal — Coucou can't reply for you yet.", x, top + 16.0 + 20.0 + 10.0 + 9.0, Face::Regular, 13.0, hex(pal::DIM));
    }

    // ── Error ─────────────────────────────────────────────────────────────────

    fn draw_error(&mut self, g: &mut Gfx, v: Rect) {
        ui::card(g, v, Wash::Red, false);
        let x = v.x + 116.0;
        let w = v.w - 116.0 - 16.0;
        let task = self.st.focus_task();
        let n8n = task.map(|t| t.source == Source::N8n).unwrap_or(false);
        let agent_label = match task.map(|t| t.source) {
            Some(Source::Agent) => "Agent",
            _ => "Claude Code",
        };
        let top = stack_top(v, &[16.0, 20.0, 16.0, 30.0], 5.0);
        agent_who(g, x, top + 8.0, task, if n8n { "n8n" } else { agent_label }, w);
        let title = if n8n { "Workflow stopped." } else { "Session stopped on an error." };
        t(g, title, x, top + 16.0 + 5.0 + 10.0, Face::Medium, 15.0, hex(pal::INK));
        let detail = task.and_then(|t| t.steps.last().cloned()).unwrap_or_else(|| "No detail available.".into());
        let detail = text::ellipsize(&detail.replace('\n', " "), Face::Regular, 12.0, w);
        t(g, &detail, x, top + 16.0 + 20.0 + 10.0 + 8.0 + 5.0, Face::Regular, 12.0, hex(pal::RED_TEXT));

        let by = top + 16.0 + 20.0 + 16.0 + 15.0;
        let (retry, rw) = self.ui.button(g, x, by, "Retry", true, None);
        let second = if n8n { "Open in n8n" } else { "Open terminal" };
        let (other, _) = self.ui.button(g, x + rw + 8.0, by, second, false, None);
        if retry {
            let v = self.st.default_view();
            self.set_view(v);
        } else if other {
            if n8n {
                if let Some(url) = crate::secrets::get("n8n-url") {
                    open_url(&url);
                }
            } else {
                self.open_terminal();
            }
        }
    }

    pub fn open_terminal(&mut self) {
        let cwd = self.st.focus_task().and_then(|t| t.session_cwd.clone());
        open_in_vscode(cwd.as_deref());
    }

    // ── Finished ──────────────────────────────────────────────────────────────

    fn draw_finished(&mut self, g: &mut Gfx, v: Rect) {
        ui::card(g, v, Wash::Green, false);
        let x = v.x + 116.0;
        let w = v.w - 116.0 - 16.0;
        let top = stack_top(v, &[16.0, 20.0, 30.0], 5.0);
        let task = self.st.focus_task();
        agent_who(g, x, top + 8.0, task, "Claude Code finished", w);
        let title = task.and_then(|t| t.steps.last().cloned()).unwrap_or_else(|| "Session finished".into());
        let title = text::ellipsize(&title, Face::Medium, 15.0, w);
        t(g, &title, x, top + 16.0 + 5.0 + 10.0, Face::Medium, 15.0, hex(pal::INK));
        let by = top + 16.0 + 20.0 + 10.0;
        let (term, tw) = self.ui.button(g, x, by, "Open terminal", true, None);
        let (ok, _) = self.ui.button(g, x + tw + 8.0, by, "OK", false, None);
        if term {
            self.open_terminal();
        } else if ok {
            self.collapse();
        }
    }

    // ── Confused / note ───────────────────────────────────────────────────────

    fn draw_confused(&mut self, g: &mut Gfx, v: Rect) {
        ui::card(g, v, Wash::Pink, false);
        let x = v.x + 128.0;
        t(g, "Too many hits at once.", x, v.cy() - 9.0, Face::Medium, 15.0, hex(pal::INK));
        t(g, "Give me a sec — back to work in three seconds.", x, v.cy() + 11.0, Face::Regular, 13.0, hex(pal::DIM));
    }

    fn draw_note(&mut self, g: &mut Gfx, v: Rect) {
        ui::card(g, v, Wash::None, false);
        let msg = self.st.note_message.clone().unwrap_or_default();
        let w = v.w - 98.0 - 18.0;
        let lines = text::wrap(&msg, Face::Medium, 15.0, w);
        let n = lines.len().min(3) as f32;
        let mut y = v.cy() - (n - 1.0) * 10.0;
        for l in lines.iter().take(3) {
            t(g, l, v.x + 98.0, y, Face::Medium, 15.0, hex(pal::INK));
            y += 20.0;
        }
    }

    // ── In-island settings ────────────────────────────────────────────────────

    fn draw_settings(&mut self, g: &mut Gfx, v: Rect) {
        ui::card(g, v, Wash::None, false);
        let x = v.x + 84.0;
        let top = v.y + 14.0 + 4.0;

        // Row 1: sound switch + volume.
        let on = self.st.settings.sound_enabled;
        if self.ui.switch(g, "sound-switch", x, top, on) {
            self.st.settings.sound_enabled = !on;
            crate::sound::set_enabled(!on);
            self.save_settings();
        }
        t(g, "Sound", x + 42.0, top + 9.0, Face::Regular, 12.5, hex("#C5C8CD"));
        // Volume slider (0 – 0.2).
        let sx = x + 42.0 + 52.0;
        let sw = 72.0;
        let sr = Rect::new(sx - 4.0, top + 1.0, sw + 8.0, 16.0);
        let (_, hover, pressed) = self.ui.click_region(id_of("volume", 17), sr);
        if pressed || (self.slider_drag && self.ui.input.down) {
            self.slider_drag = true;
            let k = clamp((self.ui.input.mouse.0 - sx) / sw, 0.0, 1.0);
            let vol = (k * 0.2 / 0.005).round() as f64 * 0.005;
            if (vol - self.st.settings.sound_volume).abs() > 1e-6 {
                self.st.settings.sound_volume = vol;
                crate::sound::set_volume(vol as f32);
            }
        }
        if !self.ui.input.down && self.slider_drag {
            self.slider_drag = false;
            self.save_settings();
            crate::sound::play("blip");
        }
        let dim = if on { 1.0 } else { 0.4 };
        g.save();
        g.mul_alpha(dim);
        g.fill_style(rgba(255, 255, 255, 0.2));
        g.fill_round_rect(sx, top + 7.5, sw, 3.0, 1.5);
        let k = (self.st.settings.sound_volume / 0.2) as f32;
        g.fill_style(rgba(255, 255, 255, 0.55));
        g.fill_round_rect(sx, top + 7.5, sw * k, 3.0, 1.5);
        g.fill_style(Color::WHITE);
        g.circle(sx + sw * k, top + 9.0, if hover || pressed { 6.5 } else { 5.5 });
        g.fill();
        g.restore();

        // Row 2: auto-close.
        let y2 = top + 18.0 + 10.0;
        crate::icons::fill(g, Icon::Timer, x + 6.0, y2 + 9.0, 12.0, hex(pal::DIM2));
        let secs = self.st.settings.auto_close_interval.round() as i32;
        t(g, &format!("Auto-close · {secs}s"), x + 18.0, y2 + 9.0, Face::Regular, 12.5, hex("#C5C8CD"));
        let right_edge = v.x + v.w - 16.0;
        let mut bx = right_edge;
        for s in [30, 15, 10] {
            let label = format!("{s}s");
            let w = text::measure(&label, Face::Regular, 11.0) + 14.0;
            bx -= w;
            let r = Rect::new(bx, y2 + 9.0 - 10.0, w, 20.0);
            let (clicked, _, _) = self.ui.click_region(id_of(&label, 19), r);
            if secs == s {
                g.fill_style(hex("#252830"));
                g.fill_round_rect(r.x, r.y, r.w, r.h, 10.0);
            }
            text::draw(g, &label, r.cx(), r.cy(), Face::Regular, 11.0, hex(if secs == s { pal::INK } else { pal::DIM3 }), Align::Center);
            bx -= 6.0;
            if clicked {
                self.st.settings.auto_close_interval = s as f64;
                self.fsm.home_to_petit = s as f32;
                self.save_settings();
            }
        }

        // Row 3: status badges + link.
        let y3 = y2 + 18.0 + 10.0;
        let hooks = self.st.hooks_installed;
        ui::dot(g, x + 3.0, y3 + 9.0, 6.0, hex(if hooks { pal::GREEN } else { pal::RED }));
        let w1 = t(g, "Claude Code", x + 10.0, y3 + 9.0, Face::Regular, 11.0, hex(pal::DIM2));
        let ax = x + 10.0 + w1 + 14.0;
        let key = self.st.has_api_key;
        ui::dot(g, ax + 3.0, y3 + 9.0, 6.0, hex(if key { pal::GREEN } else { pal::RED }));
        t(g, "API", ax + 10.0, y3 + 9.0, Face::Regular, 11.0, hex(pal::DIM2));
        let lw = text::measure("Settings…", Face::Medium, 11.5);
        let (open, _) = self.ui.link(g, v.x + v.w - 16.0 - lw, y3 + 9.0, "Settings…", hex(pal::DIM2));
        if open {
            crate::settings_win::show();
        }
    }

    // ── Chat ──────────────────────────────────────────────────────────────────

    fn draw_prompt(&mut self, g: &mut Gfx, v: Rect) {
        ui::card(g, v, Wash::Indigo, false);
        let x = v.x + 84.0;
        let w = v.w - 84.0 - 16.0;
        let mut y = v.y + 12.0;

        // Chip: what the question is about.
        if let Some(f) = self.st.dropped_file.clone() {
            let name = text::ellipsize(&f.name, Face::Regular, 11.5, 260.0);
            let cw = text::measure(&name, Face::Regular, 11.5) + 10.0 + 7.0 + 6.0 + 10.0;
            let age = clamp((now() - self.view_t0 - 0.45) / 0.3, 0.0, 1.0);
            g.fill_style(rgba(255, 255, 255, 0.1));
            g.fill_round_rect(x, y, cw, 22.0, 11.0);
            if age < 1.0 {
                g.stroke_style(rgba(255, 255, 255, 0.75 * (1.0 - age)));
                g.line_width(1.5);
                g.round_rect(x + 0.75, y + 0.75, cw - 1.5, 20.5, 10.0);
                g.stroke();
            }
            let grad = g.linear(x + 10.0, y + 7.0, x + 17.0, y + 14.0, &[
                (0.0, hex("#FF6B5B")), (0.3, hex("#F7B32B")), (0.55, hex("#2DD4A7")), (0.8, hex("#38BDF8")), (1.0, hex("#A78BFA")),
            ]);
            g.fill_style(grad);
            g.circle(x + 13.5, y + 11.0, 3.5);
            g.fill();
            t(g, &name, x + 10.0 + 7.0 + 6.0, y + 11.0, Face::Regular, 11.5, hex(pal::INK2));
            y += 22.0 + 6.0;
        }

        let bar_h = 40.0;
        let bar_y = v.y + v.h - 14.0 - bar_h;
        let log = Rect::new(x, y, w, (bar_y - 6.0 - y).max(0.0));
        self.draw_chat_log(g, log);

        // Input bar.
        g.fill_style(rgba(255, 255, 255, 0.07));
        g.fill_round_rect(x, bar_y, w, bar_h, 12.0);
        let send_r = Rect::new(x + w - 6.0 - 28.0, bar_y + (bar_h - 28.0) / 2.0, 28.0, 28.0);
        let field = Rect::new(x + 10.0, bar_y + 6.0, w - 10.0 - 6.0 - 28.0 - 8.0 - 6.0, bar_h - 12.0);

        // Click in the field focuses it.
        if self.ui.input.pressed && field.contains(self.ui.input.mouse.0, self.ui.input.mouse.1) {
            self.chat.input.focused = true;
            self.platform.set_activating(true);
            let extend = crate::platform::shift_down();
            self.chat.input.click_at(self.ui.input.mouse.0, 13.0, extend);
        }
        if field.contains(self.ui.input.mouse.0, self.ui.input.mouse.1) {
            self.ui.hovering_text = true;
        }
        self.chat.input.placeholder = if self.st.chat_history.is_empty() { "Ask me anything…".into() } else { "Continue…".into() };
        self.chat.input.draw(g, field, 13.0, pal::INK);

        let (clicked, hover, pressed) = self.ui.click_region(id_of("send", 23), send_r);
        g.save();
        let k = if pressed { 0.94 } else { 1.0 };
        g.translate(send_r.cx(), send_r.cy());
        g.scale_xy(k, k);
        g.translate(-send_r.cx(), -send_r.cy());
        g.fill_style(if self.chat.sending { rgba(245, 246, 248, 0.5) } else { hex(pal::INK) });
        g.circle(send_r.cx(), send_r.cy(), 14.0);
        g.fill();
        let _ = hover;
        crate::icons::fill(g, Icon::ArrowUp, send_r.cx(), send_r.cy(), 11.0, hex("#0B0C0E"));
        g.restore();
        if clicked {
            self.submit_chat();
        }
        // Keep typing possible after the user clicked elsewhere inside the chat card.
        if self.ui.input.pressed && !field.contains(self.ui.input.mouse.0, self.ui.input.mouse.1) && !send_r.contains(self.ui.input.mouse.0, self.ui.input.mouse.1) {
            self.chat.input.focused = false;
        }
    }

    fn draw_chat_log(&mut self, g: &mut Gfx, area: Rect) {
        if area.h < 10.0 {
            return;
        }
        let n_thinking = self.st.state_override == Some(BotState::Thinking);
        // Layout: measure every row.
        struct Row {
            lines: Vec<String>,
            user: bool,
            h: f32,
            w: f32,
        }
        let mut rows: Vec<Row> = Vec::new();
        for m in &self.st.chat_history {
            let user = m.role == Role::User;
            let max_w = if user { area.w - 32.0 - 20.0 } else { area.w - 8.0 };
            let lines = text::wrap(&m.content, Face::Regular, 12.5, max_w);
            let lw = lines.iter().map(|l| text::measure(l, Face::Regular, 12.5)).fold(0.0, f32::max);
            let h = lines.len() as f32 * 17.0 + if user { 12.0 } else { 0.0 };
            rows.push(Row { lines, user, h, w: lw + if user { 20.0 } else { 0.0 } });
        }
        let gap = 6.0;
        let mut total: f32 = rows.iter().map(|r| r.h).sum::<f32>() + gap * rows.len().saturating_sub(1) as f32 + 4.0;
        if n_thinking {
            total += 18.0 + gap;
        }
        self.chat.max_scroll = (total - area.h).max(0.0);
        if self.chat.scroll_to_end {
            self.chat.scroll = self.chat.max_scroll;
            self.chat.scroll_to_end = false;
        }
        if area.contains(self.ui.input.mouse.0, self.ui.input.mouse.1) && self.ui.input.wheel != 0.0 {
            self.chat.scroll -= self.ui.input.wheel * 36.0;
        }
        self.chat.scroll = clamp(self.chat.scroll, 0.0, self.chat.max_scroll);

        g.save();
        g.clip_rect_xywh(area.x, area.y, area.w, area.h);
        let mut y = area.y + 2.0 - self.chat.scroll;
        for r in &rows {
            if r.user {
                let bw = r.w;
                let bx = area.x + area.w - bw;
                g.fill_style(rgba(255, 255, 255, 0.13));
                g.fill_round_rect(bx, y, bw, r.h, 12.0);
                for (i, l) in r.lines.iter().enumerate() {
                    t(g, l, bx + 10.0, y + 6.0 + 8.5 + i as f32 * 17.0, Face::Regular, 12.5, hex(pal::INK2));
                }
            } else {
                for (i, l) in r.lines.iter().enumerate() {
                    t(g, l, area.x, y + 8.5 + i as f32 * 17.0, Face::Regular, 12.5, hex("#B0B5BE"));
                }
            }
            y += r.h + gap;
        }
        if n_thinking {
            let tt = now();
            for i in 0..3 {
                let k = 0.6 + 0.6 * (0.5 + 0.5 * ((tt - i as f32 * 0.14) * std::f32::consts::TAU / 0.9).sin());
                ui::dot(g, area.x + 4.0 + i as f32 * 9.0, y + 9.0, 5.0 * k, hex(pal::DIM3));
            }
        }
        g.restore();
        // Scroll fade at the top edge when scrolled.
        if self.chat.scroll > 1.0 {
            let grad = g.linear(0.0, area.y, 0.0, area.y + 14.0, &[(0.0, hex(pal::CARD)), (1.0, rgba(20, 21, 24, 0.0))]);
            g.save();
            g.fill_style(grad);
            g.fill_rect(area.x, area.y, area.w, 14.0);
            g.restore();
        }
    }

    // ── Drop zone fallbacks (the sequence canvas normally draws these) ───────

    fn draw_drop_zone(&mut self, g: &mut Gfx, v: Rect, n: f32) {
        let over = self.st.file_drag_over;
        ui::card(g, v, Wash::None, true);
        if over {
            ui::glow(g, v, 50.0, 100.0, 200.0, rgba(34, 197, 94, 0.13));
        }
        // Marching dashed frame.
        g.save();
        g.stroke_style(if over { rgba(34, 197, 94, 0.65) } else { rgba(255, 255, 255, 0.13) });
        g.line_width(1.5);
        let r = v.inset(0.75);
        g.round_rect(r.x, r.y, r.w, r.h, 20.0);
        if let Some(p) = g.take_path() {
            let sw = tiny_skia::Stroke {
                width: 1.5,
                dash: tiny_skia::StrokeDash::new(vec![6.0, 5.0], -(n * 10.0) % 11.0),
                ..tiny_skia::Stroke::default()
            };
            g.stroke_path_with(&p, &sw);
        }
        g.restore();
        let x = v.x + 196.0;
        t(g, "Drop your files here", x, v.cy() - 12.0, Face::Medium, 13.0, if over { hex(pal::GREEN2) } else { hex("#D5D7DB") });
        let mut tx = x;
        for tag in ["PDF", "Images", "Code", "Docs"] {
            let w = text::measure(tag, Face::Regular, 11.0) + 16.0;
            g.fill_style(rgba(255, 255, 255, 0.07));
            g.fill_round_rect(tx, v.cy() + 2.0, w, 20.0, 10.0);
            text::draw(g, tag, tx + w / 2.0, v.cy() + 12.0, Face::Regular, 11.0, hex("#B9BDC4"), Align::Center);
            tx += w + 6.0;
        }
        self.ensure_running();
    }

    fn draw_uploading_fallback(&mut self, g: &mut Gfx, v: Rect) {
        ui::card(g, v, Wash::None, false);
        let name = self.st.dropped_file.as_ref().map(|f| f.name.clone()).unwrap_or_else(|| "file".into());
        let p = self.st.upload_progress;
        let done = p >= 0.999;
        let label = if done { format!("✓  {name}") } else { format!("Uploading {name}") };
        t(g, &label, v.x + 16.0, v.y + 30.0, Face::Regular, 12.5, hex(if done { pal::GREEN2 } else { "#A9ADB5" }));
        g.fill_style(rgba(255, 255, 255, 0.08));
        g.fill_round_rect(v.x + 16.0, v.y + 50.0, v.w - 32.0, 4.0, 2.0);
        g.fill_style(hex(pal::GREEN2));
        g.fill_round_rect(v.x + 16.0, v.y + 50.0, (v.w - 32.0) * p, 4.0, 2.0);
    }

    fn draw_choose_fallback(&mut self, g: &mut Gfx, v: Rect) {
        ui::card(g, v, Wash::None, false);
        let name = self.st.dropped_file.as_ref().map(|f| f.name.clone()).unwrap_or_else(|| "file".into());
        let x = v.x + 98.0;
        let top = stack_top(v, &[20.0, 18.0, 30.0], 5.0);
        let nm = text::ellipsize(&name, Face::Medium, 15.0, v.w - 98.0 - 120.0);
        let w = t(g, &nm, x, top + 10.0, Face::Medium, 15.0, hex(pal::INK));
        t(g, " is ready.", x + w, top + 10.0, Face::Regular, 15.0, hex(pal::INK));
        t(g, "What do you want to do with it?", x, top + 20.0 + 5.0 + 9.0, Face::Regular, 13.0, hex(pal::DIM));
        let by = top + 20.0 + 18.0 + 10.0;
        let (ask, aw) = self.ui.button(g, x, by, "Ask a question", true, None);
        let (cancel, _) = self.ui.button(g, x + aw + 8.0, by, "Cancel", false, None);
        if ask {
            self.set_view(View::Prompt);
        } else if cancel {
            let v = self.st.default_view();
            self.set_view(v);
        }
    }

}

// ── Shell helpers ─────────────────────────────────────────────────────────────

pub fn open_url_for(id: &str) -> Option<&'static str> {
    Some(match id {
        "integration_resend" => "https://resend.com/emails",
        "integration_vercel" => "https://vercel.com/dashboard",
        "integration_github" => "https://github.com",
        "integration_stripe" => "https://dashboard.stripe.com/payments",
        "integration_notion" => "https://notion.so",
        "integration_calcom" => "https://app.cal.com/bookings",
        _ => return None,
    })
}

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn open_url(url: &str) {
    use std::os::windows::process::CommandExt;
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return;
    }
    let _ = std::process::Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", url])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

/// Our own `where`: walks %PATH% against %PATHEXT%, no shell involved.
fn find_on_path(stem: &str) -> Option<std::path::PathBuf> {
    let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    let dirs = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&dirs) {
        for ext in exts.split(';').filter(|e| !e.is_empty()) {
            let candidate = dir.join(format!("{stem}{}", ext.to_lowercase()));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// "Open terminal" opens the working folder in VS Code when `code` is on PATH,
/// and falls back to Explorer otherwise. No `cmd /C` anywhere near the path.
pub fn open_in_vscode(path: Option<&str>) -> bool {
    use std::os::windows::process::CommandExt;
    if let Some(code) = find_on_path("code") {
        let mut cmd = std::process::Command::new(code);
        if let Some(p) = path.filter(|p| !p.is_empty()) {
            cmd.arg(p);
        }
        if cmd.creation_flags(CREATE_NO_WINDOW).spawn().is_ok() {
            return true;
        }
    }
    if let Some(p) = path.filter(|p| !p.is_empty()) {
        let _ = std::process::Command::new("explorer").arg(p).spawn();
    }
    false
}
