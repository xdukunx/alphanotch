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
            input: TextField::new("Tanya apa saja…", false),
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
        let mut lap = std::time::Instant::now();
        self.draw_header(g, iw);
        crate::app::prof_lap("header", &mut lap);
        g.restore();

        let mut v = Rect::new(10.0, 42.0, iw - 20.0, (ih - 52.0).max(0.0));
        if crate::nusapages::is_page(self.st.view) {
            v.h -= crate::nusapages::PAGER_H;
        }
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
            View::Diff => self.draw_diff(g, v),
            View::Reminder => self.draw_reminder(g, v),
            View::Quests => self.draw_quests(g, v),
            View::Focus => self.draw_focus(g, v),
            View::Water => self.draw_water(g, v),
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

        // The pages of the dashboard group page between themselves with chevrons and dots; their
        // fold arrow moves to the bottom right corner so both fit.
        let paged = crate::nusapages::is_page(self.st.view);
        if paged {
            self.draw_pager(g, iw, ih);
        }
        // A small arrow folds the island back to the bar.
        if !matches!(self.st.view, View::Greeting | View::Upload | View::Uploading | View::Choose | View::Confused) {
            let ax = if paged { iw - 40.0 } else { iw / 2.0 };
            let hit = Rect::new(ax - 24.0, ih - 16.0, 48.0, 16.0);
            let (clicked, hover, _) = self.ui.click_region(id_of("collapse-arrow", 103), hit);
            g.stroke_style(rgba(255, 255, 255, if hover { 0.9 } else { 0.32 }));
            g.line_width(1.6);
            g.line_cap_round();
            g.line_join_round();
            g.begin_path();
            let ay = if paged { ih - 9.0 } else { ih - 3.5 };
            g.move_to(ax - 6.0, ay);
            g.line_to(ax, ay - 4.0);
            g.line_to(ax + 6.0, ay);
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
        // (icon, id, view, the feature switch that can hide it)
        let all = [
            (Icon::House, "tab-home", View::Overview, ""),
            (Icon::Bubble, "tab-chat", View::Prompt, "chat"),
            (Icon::Plus, "tab-drop", View::Upload, "drop"),
            (Icon::Stack, "tab-dash", View::Dashboard, "dashboard"),
            (Icon::Chart, "tab-stocks", View::Stocks, "stocks"),
            (Icon::Cloud, "tab-weather", View::Weather, "weather"),
            (Icon::Lines, "tab-prompter", View::Teleprompter, "teleprompter"),
            (Icon::Star, "tab-quests", View::Quests, "quests"),
        ];
        let tabs: Vec<(Icon, &str, View)> = all
            .iter()
            .filter(|(_, _, _, f)| f.is_empty() || self.st.settings.feature_on(f))
            .map(|(i, id, t, _)| (*i, *id, *t))
            .collect();
        let v = self.st.view;
        for (i, (icon, id, target)) in tabs.iter().enumerate() {
            let r = Rect::new(14.0 + i as f32 * 31.0, cy - 11.0, 27.0, 22.0);
            let on = match target {
                View::Overview => v == View::Overview || v == View::Empty,
                other => v == *other,
            };
            if self.ui.header_icon(g, id, r, *icon, 13.0, on, true) {
                crate::sound::play("blip");
                // Home is the sessions view, always: with a docked bar `default_view` is the
                // dashboard, which made Home look dead whenever the dashboard was open.
                let go = if *target == View::Overview {
                    if self.st.tasks.is_empty() { View::Empty } else { View::Overview }
                } else {
                    *target
                };
                self.set_view(go);
            }
        }

        // Date and time, in the free stretch of the header (the dashboard group has its own greeting).
        if !crate::nusapages::is_page(v) && !matches!(v, View::Stocks | View::Weather | View::Teleprompter | View::Greeting) {
            let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
            let (_, _, short) = crate::dashboard::greeting(&self.st.settings.display_name);
            let label = format!("{short}  {:02}:{:02}", t.wHour, t.wMinute);
            let right = iw - 51.0 - 8.0 - 16.0;
            text::draw(g, &label, right, cy, Face::Regular, 11.0, hex(pal::DIM2), Align::Right);
            // Claude's plan, when Claude Code has reported it: "5j 35% · mgg 54%" with a dot.
            let now_s = crate::util::unix_now();
            if let Some(p) = crate::coach::plan(now_s) {
                let five = p.five.map(|w| crate::coach::effective(w, now_s));
                let week = p.week.map(|w| crate::coach::effective(w, now_s));
                let mut parts = Vec::new();
                if let Some(f) = five {
                    parts.push(format!("5j {:.0}%", f));
                }
                if let Some(w) = week {
                    parts.push(format!("mgg {:.0}%", w));
                }
                let top = five.unwrap_or(0.0).max(week.unwrap_or(0.0));
                let col = if top < 50.0 { "#22C55E" } else if top < 80.0 { "#F5A524" } else { "#F4505E" };
                let usage = parts.join(" · ");
                let lw = text::measure(&label, Face::Regular, 11.0);
                let uw = text::measure(&usage, Face::Medium, 10.5);
                let pill = Rect::new(right - lw - 14.0 - uw - 24.0, cy - 9.0, uw + 24.0, 18.0);
                if pill.x > 14.0 + 8.0 * 31.0 {
                    g.fill_style(rgba(255, 255, 255, 0.06));
                    g.fill_round_rect(pill.x, pill.y, pill.w, pill.h, 9.0);
                    ui::dot(g, pill.x + 10.0, pill.cy(), 6.0, hex(col));
                    text::draw(g, &usage, pill.x + 18.0, pill.cy(), Face::Medium, 10.5, hex("#C5C8CD"), Align::Left);
                }
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
        let diff_step = task.diffs.iter().rposition(|d| d.is_some());
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
            // The latest edit's diff is one click away, on the ticker or on the chip.
            if let Some(di) = diff_step {
                let (open, _) = self.ui.link(g, left.x + 108.0 + 18.0, left.y + left.h - 18.0, "Lihat diff", hex("#7EE2B8"));
                let r = Rect::new(left.x + 108.0, left.y + 28.0, left.w - 108.0 - 40.0, 44.0);
                let (clicked, _, _) = self.ui.click_region(id_of("ticker-diff", 83), r);
                if open || clicked {
                    self.open_diff(di);
                }
            }
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
        t(g, "Belum ada yang berjalan.", x, v.cy() - 11.0, Face::Medium, 15.0, hex(pal::INK));
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

        let (tool, detail) = match self.st.pending_approval.as_ref() {
            Some(p) => {
                let d = p.command.strip_prefix(&format!("{} · ", p.tool)).unwrap_or(&p.command).to_string();
                (p.tool.clone(), d)
            }
            None => ("…".to_string(), String::new()),
        };
        let name = self.session_name(22);
        let expanded = self.detail_open;

        // Headline in plain words; the exact command is one tap away, never gone: approving
        // blind is the thing this card exists to prevent.
        let head = format!("Claude Code sesi “{name}” minta izin");
        let detail_lines: Vec<String> = if expanded {
            text::wrap(&detail.replace('\n', " "), Face::Mono, 11.0, w - 8.0).into_iter().take(3).collect()
        } else {
            Vec::new()
        };
        let body_h = if expanded { detail_lines.len().max(1) as f32 * 14.0 } else { 18.0 + 4.0 + 14.0 };
        let top = stack_top(v, &[20.0, body_h, 30.0], 6.0);

        t(g, &text::ellipsize(&head, Face::Medium, 14.0, w), x, top + 10.0, Face::Medium, 14.0, hex(pal::INK));
        let mut y = top + 20.0 + 6.0;
        if expanded {
            for (i, l) in detail_lines.iter().enumerate() {
                t(g, l, x, y + 7.0 + i as f32 * 14.0, Face::Mono, 11.0, hex("#E8E9EC"));
            }
        } else {
            t(g, &text::ellipsize(&approval_summary(&tool, &detail), Face::Regular, 13.0, w), x, y + 9.0, Face::Regular, 13.0, hex(pal::DIM));
            y += 22.0;
            let r = Rect::new(x - 4.0, y - 2.0, w, 18.0);
            let (clicked, hover, _) = self.ui.click_region(id_of("approval-detail", 61), r);
            let line = text::ellipsize(&detail.replace('\n', " "), Face::Mono, 11.0, w - 24.0);
            let col = if hover { hex("#E8E9EC") } else { hex(pal::DIM3) };
            t(g, &line, x, y + 7.0, Face::Mono, 11.0, col);
            let lw = text::measure(&line, Face::Mono, 11.0);
            crate::icons::stroke(g, Icon::ChevronRight, x + lw + 12.0, y + 7.0, 8.0, 2.2, col);
            if clicked {
                self.detail_open = true;
            }
        }
        if expanded {
            let r = Rect::new(x - 4.0, top + 20.0, w, body_h + 8.0);
            let (clicked, _, _) = self.ui.click_region(id_of("approval-detail-close", 67), r);
            if clicked {
                self.detail_open = false;
            }
        }

        let by = top + 20.0 + 6.0 + body_h + 6.0;
        let (deny, dw) = self.ui.button(g, x, by, "Tolak", false, Some("N"));
        let (allow, _) = self.ui.button(g, x + dw + 8.0, by, "Izinkan", true, Some("Y"));
        if deny {
            self.decide(false);
        } else if allow {
            self.decide(true);
        }
    }

    /// The focused session's project name, shortened for a headline.
    fn session_name(&self, max: usize) -> String {
        let n = self.st.focus_task().map(|t| t.name.clone()).unwrap_or_else(|| "ini".into());
        if n.chars().count() > max {
            n.chars().take(max - 1).collect::<String>() + "…"
        } else {
            n
        }
    }

    // ── Question ──────────────────────────────────────────────────────────────

    fn draw_question(&mut self, g: &mut Gfx, v: Rect) {
        ui::card(g, v, Wash::Cyan, false);
        let x = v.x + 116.0;
        let w = v.w - 116.0 - 16.0;
        if let Some(p) = self.st.pending_question.clone() {
            if let Some(q) = p.questions.get(p.index).cloned() {
                self.draw_ask(g, v, x, w, &p, &q);
                return;
            }
        }
        let top = stack_top(v, &[20.0, 18.0, 18.0], 6.0);
        let head = format!("Claude Code sesi “{}” butuh jawabanmu", self.session_name(22));
        t(g, &text::ellipsize(&head, Face::Medium, 14.0, w), x, top + 10.0, Face::Medium, 14.0, hex(pal::INK));
        let q = self.st.focus_task().and_then(|t| t.steps.last().cloned()).unwrap_or_else(|| "Ada pertanyaan untukmu.".into());
        t(g, &text::ellipsize(&q.replace('\n', " "), Face::Regular, 13.0, w), x, top + 20.0 + 6.0 + 9.0, Face::Regular, 13.0, hex(pal::DIM));
        t(g, "Jawab di terminal.", x, top + 20.0 + 6.0 + 18.0 + 6.0 + 9.0, Face::Regular, 12.0, hex(pal::DIM3));
    }

    /// The interactive question: options as chips, an own-answer field, "Di terminal".
    fn draw_ask(&mut self, g: &mut Gfx, v: Rect, x: f32, w: f32, p: &crate::state::PendingQuestion, q: &crate::state::AskQuestion) {
        let mut y = v.y + 12.0;
        let head = format!("Claude Code sesi “{}” bertanya", self.session_name(18));
        let room = w - if p.questions.len() > 1 { 40.0 } else { 0.0 };
        t(g, &text::ellipsize(&head, Face::Medium, 14.0, room), x, y + 10.0, Face::Medium, 14.0, hex(pal::INK));
        if p.questions.len() > 1 {
            let c = format!("{}/{}", p.index + 1, p.questions.len());
            text::draw(g, &c, x + w, y + 10.0, Face::Regular, 11.0, hex(pal::DIM3), Align::Right);
        }
        y += 22.0;
        let lines: Vec<String> = text::wrap(&q.question.replace('\n', " "), Face::Regular, 13.0, w).into_iter().take(2).collect();
        for l in &lines {
            t(g, l, x, y + 8.0, Face::Regular, 13.0, hex(pal::DIM));
            y += 16.0;
        }
        y += 6.0;

        // Options.
        let cw = (w - 6.0) / 2.0;
        let mut hit = None;
        for (i, o) in q.options.iter().enumerate() {
            let r = Rect::new(x + (i % 2) as f32 * (cw + 6.0), y + (i / 2) as f32 * 32.0, cw, 26.0);
            let (clicked, hover, pressed) = self.ui.click_region(id_of(&format!("ask-opt-{i}"), 71), r);
            let on = p.picked.get(i).copied().unwrap_or(false);
            g.save();
            let k = if pressed { 0.97 } else { 1.0 };
            g.translate(r.cx(), r.cy());
            g.scale_xy(k, k);
            g.translate(-r.cx(), -r.cy());
            g.fill_style(if on { rgba(34, 211, 238, 0.3) } else { rgba(255, 255, 255, if hover { 0.16 } else { 0.09 }) });
            g.fill_round_rect(r.x, r.y, r.w, r.h, 13.0);
            if on {
                g.stroke_style(rgba(34, 211, 238, 0.8));
                g.line_width(1.0);
                g.round_rect(r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0, 12.5);
                g.stroke();
            }
            let lab = text::ellipsize(&o.label, Face::Medium, 12.0, r.w - 20.0);
            let lw = t(g, &lab, r.x + 10.0, r.cy(), Face::Medium, 12.0, hex(pal::INK));
            let left = r.w - 20.0 - lw - 8.0;
            if !o.description.is_empty() && left > 40.0 {
                let d = text::ellipsize(&o.description.replace('\n', " "), Face::Regular, 10.5, left);
                t(g, &d, r.x + 10.0 + lw + 8.0, r.cy(), Face::Regular, 10.5, hex(pal::DIM3));
            }
            g.restore();
            if clicked {
                hit = Some(i);
            }
        }
        y += q.options.len().div_ceil(2) as f32 * 32.0 + 4.0;

        // Own answer + send + hand back.
        let kw = text::measure("Kirim", Face::Medium, 12.5) + 26.0;
        let tw = text::measure("Di terminal", Face::Medium, 12.5) + 26.0;
        let field = Rect::new(x, y, w - kw - tw - 16.0, 30.0);
        g.fill_style(rgba(255, 255, 255, 0.07));
        g.fill_round_rect(field.x, field.y, field.w, field.h, 15.0);
        let inner = Rect::new(field.x + 12.0, field.y + 4.0, field.w - 24.0, 22.0);
        if self.ui.input.pressed && field.contains(self.ui.input.mouse.0, self.ui.input.mouse.1) {
            self.ask_input.focused = true;
            self.platform.set_activating(true);
            let extend = crate::platform::shift_down();
            self.ask_input.click_at(self.ui.input.mouse.0, 13.0, extend);
        } else if self.ui.input.pressed && self.ask_input.focused {
            self.ask_input.focused = false;
            self.platform.set_activating(false);
        }
        self.ui.note_region(id_of("ask-field", 113), field);
        if field.contains(self.ui.input.mouse.0, self.ui.input.mouse.1) {
            self.ui.hovering_text = true;
        }
        self.ask_input.draw(g, inner, 13.0, pal::INK);
        let (send, _) = self.ui.button(g, x + field.w + 8.0, y, "Kirim", true, None);
        let (back, _) = self.ui.button(g, x + field.w + 8.0 + kw + 8.0, y, "Di terminal", false, None);

        if let Some(i) = hit {
            if q.multi {
                self.toggle_option(i);
            } else if let Some(o) = q.options.get(i) {
                self.answer_question(o.label.clone());
            }
        } else if send {
            self.submit_question_text();
        } else if back {
            self.hand_back_question();
        }
    }

    // ── Quests ────────────────────────────────────────────────────────────────

    fn draw_quests(&mut self, g: &mut Gfx, v: Rect) {
        use crate::buddy::crown_tier;
        ui::card(g, v, Wash::Amber, false);
        let x = v.x + 116.0;
        let w = v.w - 116.0 - 16.0;
        let care_on = {
            let s = &self.st.settings;
            s.water_every_min > 0 || s.stretch_every_min > 0 || s.eyes_every_min > 0
        };
        let quests = self.buddy.quests(care_on);
        let (_, crown) = crown_tier(self.buddy.crowns);
        let day = ((crate::util::unix_now() + crate::util::local_offset_secs() as f64) / 86_400.0).floor() as i64;
        let streak = self.buddy.streak_on(day);

        t(g, "Misi hari ini", x, v.y + 16.0, Face::Medium, 14.0, hex(pal::INK));
        let tag = format!("Mahkota {crown} · {} hari tuntas", self.buddy.crowns);
        text::draw(g, &tag, x + w, v.y + 16.0, Face::Regular, 11.0, hex(pal::DIM2), Align::Right);

        for (i, q) in quests.iter().enumerate() {
            let y = v.y + 34.0 + i as f32 * 28.0;
            let done = q.complete();
            let shown = q.done.min(q.target);
            let label = text::ellipsize(&q.title, Face::Regular, 12.5, w - 56.0);
            t(g, &label, x, y + 8.0, Face::Regular, 12.5, hex(if done { pal::DIM3 } else { pal::INK }));
            if done {
                crate::icons::stroke(g, Icon::Check, x + w - 8.0, y + 8.0, 9.0, 2.2, hex(pal::GREEN));
            } else {
                text::draw(g, &format!("{shown}/{}", q.target), x + w, y + 8.0, Face::Medium, 11.5, hex(pal::DIM), Align::Right);
            }
            g.fill_style(rgba(255, 255, 255, 0.09));
            g.fill_round_rect(x, y + 19.0, w, 4.0, 2.0);
            let k = if q.target == 0 { 1.0 } else { (shown as f32 / q.target as f32).clamp(0.0, 1.0) };
            if k > 0.0 {
                g.fill_style(hex(if done { pal::GREEN } else { "#F5A524" }));
                g.fill_round_rect(x, y + 19.0, (w * k).max(4.0), 4.0, 2.0);
            }
        }
        let foot = if streak > 0 {
            format!("Beruntun {streak} hari menjaga diri. Selesaikan ketiganya untuk mahkota baru.")
        } else {
            "Konfirmasi satu pengingat (Sudah) untuk memulai beruntun.".to_string()
        };
        t(g, &text::ellipsize(&foot, Face::Regular, 11.0, w), x, v.y + v.h - 12.0, Face::Regular, 11.0, hex(pal::DIM3));
    }

    // ── Reminder ──────────────────────────────────────────────────────────────

    fn draw_reminder(&mut self, g: &mut Gfx, v: Rect) {
        use crate::reminders::RemKind;
        let Some(card) = self.reminder.clone() else {
            ui::card(g, v, Wash::None, false);
            return;
        };
        let wash = match card.kind {
            RemKind::Water => Wash::Cyan,
            RemKind::Stretch => Wash::Green,
            RemKind::Eyes | RemKind::Suggest => Wash::Indigo,
            RemKind::LimitPlenty => Wash::Green,
            RemKind::LimitFast | RemKind::Task => Wash::Amber,
            RemKind::Prayer => Wash::Cyan,
        };
        ui::card(g, v, wash, false);
        let x = v.x + 116.0;
        let w = v.w - 116.0 - 16.0;
        if card.kind == RemKind::Prayer {
            if let Some((where_, now_idx, rows)) = crate::prayer::parse_card(&card.body) {
                let top = stack_top(v, &[22.0, 42.0, 30.0], 10.0);
                t(g, &text::ellipsize(&card.title, Face::Medium, 15.0, w * 0.55), x, top + 11.0, Face::Medium, 15.0, hex(pal::INK));
                text::draw(g, &where_, x + w, top + 12.0, Face::Regular, 11.0, hex(pal::DIM2), Align::Right);
                let gap = 6.0;
                let cw = (w - gap * 4.0) / 5.0;
                for (i, (name, time)) in rows.iter().enumerate() {
                    let r = Rect::new(x + i as f32 * (cw + gap), top + 22.0 + 10.0, cw, 42.0);
                    let on = i == now_idx;
                    g.fill_style(if on { rgba(45, 91, 255, 0.34) } else { rgba(255, 255, 255, 0.06) });
                    g.fill_round_rect(r.x, r.y, r.w, r.h, 10.0);
                    if on {
                        g.stroke_style(rgba(120, 160, 255, 0.55));
                        g.line_width(1.0);
                        g.round_rect(r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0, 9.5);
                        g.stroke();
                    }
                    text::draw(g, name, r.cx(), r.y + 13.0, Face::Regular, 10.5, hex(if on { pal::INK } else { pal::DIM2 }), Align::Center);
                    text::draw(g, time, r.cx(), r.y + 29.0, Face::Bold, 15.0, hex(if on { pal::INK } else { "#C5C8CD" }), Align::Center);
                }
                let by = top + 22.0 + 10.0 + 42.0 + 10.0;
                let (ok, ow) = self.ui.button(g, x, by, "Oke", true, None);
                let (later, _) = self.ui.button(g, x + ow + 8.0, by, "Ingatkan 10 menit", false, None);
                if ok {
                    self.dismiss_reminder(false);
                } else if later {
                    self.dismiss_reminder(true);
                }
                return;
            }
        }
        // Suggestions arrive one per line; everything else is a paragraph.
        let body: Vec<String> = if card.body.contains('\n') {
            card.body
                .lines()
                .take(3)
                .map(|l| text::ellipsize(l, Face::Regular, 12.5, w))
                .collect()
        } else {
            text::wrap(&card.body, Face::Regular, 12.5, w).into_iter().take(3).collect()
        };
        let top = stack_top(v, &[22.0, body.len() as f32 * 17.0, 30.0], 8.0);
        t(g, &text::ellipsize(&card.title, Face::Medium, 15.0, w), x, top + 11.0, Face::Medium, 15.0, hex(pal::INK));
        let mut y = top + 22.0 + 8.0;
        for l in &body {
            t(g, l, x, y + 8.0, Face::Regular, 12.5, hex(pal::DIM));
            y += 17.0;
        }
        let by = y + 8.0;
        let interval = card.kind.is_interval();
        let task = card.kind == RemKind::Task;
        let prayer = card.kind == RemKind::Prayer;
        let (ok, ow) = self.ui.button(g, x, by, if interval { "Sudah" } else if task { "Selesai" } else { "Oke" }, true, None);
        let later = if interval || task || prayer {
            self.ui.button(g, x + ow + 8.0, by, if prayer { "Ingatkan 10 menit" } else { "Tunda 10 menit" }, false, None).0
        } else {
            false
        };
        if ok {
            self.dismiss_reminder(false);
        } else if later {
            self.dismiss_reminder(true);
        }
    }

    // ── Diff ──────────────────────────────────────────────────────────────────

    fn draw_diff(&mut self, g: &mut Gfx, v: Rect) {
        ui::card(g, v, Wash::Indigo, false);
        let x = v.x + 116.0;
        let w = v.w - 116.0 - 16.0;
        let Some(d) = self.diff_view.as_ref() else { return };
        let file = d.file.rsplit(['\\', '/']).next().unwrap_or(&d.file).to_string();
        let (added, removed) = (d.added, d.removed);

        let hy = v.y + 12.0;
        let close = "Tutup";
        let cw = text::measure(close, Face::Medium, 11.0);
        let a = format!("+{added}");
        let r = format!("\u{2212}{removed}");
        let (aw, rw) = (text::measure(&a, Face::Medium, 12.0), text::measure(&r, Face::Medium, 12.0));
        let counts_w = aw + 6.0 + rw;
        let name = text::ellipsize(&file, Face::Mono, 12.0, w - counts_w - cw - 28.0);
        let nw = t(g, &name, x, hy + 10.0, Face::Mono, 12.0, hex(pal::INK));
        let cx = x + nw + 12.0;
        text::draw(g, &a, cx, hy + 10.0, Face::Medium, 12.0, hex(pal::GREEN), Align::Left);
        text::draw(g, &r, cx + aw + 6.0, hy + 10.0, Face::Medium, 12.0, hex(pal::RED), Align::Left);
        let (close_clicked, _) = self.ui.link(g, x + w - cw, hy + 10.0, close, hex(pal::DIM2));

        // The lines, clipped to their area and scrollable.
        let area = Rect::new(x - 4.0, hy + 28.0, w + 8.0, (v.y + v.h - 10.0 - (hy + 28.0)).max(0.0));
        let lh = 15.0;
        let n = d.lines.len() as f32;
        let max_scroll = (n * lh - area.h).max(0.0);
        if area.contains(self.ui.input.mouse.0, self.ui.input.mouse.1) && self.ui.input.wheel != 0.0 {
            self.diff_scroll -= self.ui.input.wheel * 36.0;
        }
        self.diff_scroll = self.diff_scroll.clamp(0.0, max_scroll);
        let scroll = self.diff_scroll;
        g.save();
        g.clip_rect_xywh(area.x, area.y, area.w, area.h);
        for (i, l) in d.lines.iter().enumerate() {
            let ly = area.y + i as f32 * lh - scroll;
            if ly + lh < area.y || ly > area.y + area.h {
                continue;
            }
            let (kind, body) = match l.chars().next() {
                Some(c @ ('+' | '-' | ' ')) => (c, &l[1..]),
                _ => ('@', l.as_str()),
            };
            let (bg, fg) = match kind {
                '+' => (Some(rgba(52, 211, 153, 0.14)), hex("#7EE2B8")),
                '-' => (Some(rgba(244, 80, 94, 0.16)), hex("#FF8A95")),
                '@' => (None, hex(pal::DIM3)),
                _ => (None, hex(pal::DIM)),
            };
            if let Some(bg) = bg {
                g.fill_style(bg);
                g.fill_rect(area.x, ly, area.w, lh);
            }
            let sign = match kind {
                '+' => "+",
                '-' => "\u{2212}",
                _ => " ",
            };
            if kind != '@' {
                t(g, sign, area.x + 4.0, ly + lh / 2.0, Face::Mono, 11.0, fg);
            }
            let shown = text::ellipsize(&body.replace('\t', "    "), Face::Mono, 11.0, area.w - 24.0);
            t(g, &shown, area.x + if kind == '@' { 4.0 } else { 16.0 }, ly + lh / 2.0, Face::Mono, 11.0, fg);
        }
        g.restore();
        if close_clicked {
            self.diff_view = None;
            let v = self.st.default_view();
            self.set_view(v);
        }
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
        let title_owned = if n8n {
            "Workflow berhenti.".to_string()
        } else {
            format!("Claude Code sesi “{}” berhenti karena error", self.session_name(18))
        };
        let title = title_owned.as_str();
        t(g, title, x, top + 16.0 + 5.0 + 10.0, Face::Medium, 15.0, hex(pal::INK));
        let detail = task.and_then(|t| t.steps.last().cloned()).unwrap_or_else(|| "Tidak ada detail.".into());
        let detail = text::ellipsize(&detail.replace('\n', " "), Face::Regular, 12.0, w);
        t(g, &detail, x, top + 16.0 + 20.0 + 10.0 + 8.0 + 5.0, Face::Regular, 12.0, hex(pal::RED_TEXT));

        let by = top + 16.0 + 20.0 + 16.0 + 15.0;
        let (retry, rw) = self.ui.button(g, x, by, "Coba lagi", true, None);
        let second = if n8n { "Buka n8n" } else { "Buka terminal" };
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
        let task = self.st.focus_task();
        let last = task.and_then(|t| t.steps.last().cloned()).unwrap_or_default();
        let has_last = !last.trim().is_empty();
        let top = stack_top(v, &[22.0, if has_last { 18.0 } else { 0.0 }, 30.0], 6.0);

        let head = format!("Claude Code sesi “{}” sudah beres, silakan dicek.", self.session_name(18));
        t(g, &text::ellipsize(&head, Face::Medium, 15.0, w), x, top + 11.0, Face::Medium, 15.0, hex(pal::INK));
        let mut y = top + 22.0 + 6.0;
        if has_last {
            t(g, &text::ellipsize(&last.replace('\n', " "), Face::Regular, 12.0, w), x, y + 9.0, Face::Regular, 12.0, hex(pal::DIM));
            y += 18.0 + 6.0;
        }
        let (term, tw) = self.ui.button(g, x, y, "Buka terminal", true, None);
        let (ok, _) = self.ui.button(g, x + tw + 8.0, y, "OK", false, None);
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
        t(g, "Aduh, kebanyakan dipukul.", x, v.cy() - 9.0, Face::Medium, 15.0, hex(pal::INK));
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
        self.ui.note_region(id_of("chat-field", 112), field);
        if field.contains(self.ui.input.mouse.0, self.ui.input.mouse.1) {
            self.ui.hovering_text = true;
        }
        self.chat.input.placeholder = if self.st.chat_history.is_empty() { "Tanya apa saja…".into() } else { "Lanjutkan…".into() };
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
        t(g, "Lepaskan file di sini", x, v.cy() - 12.0, Face::Medium, 13.0, if over { hex(pal::GREEN2) } else { hex("#D5D7DB") });
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
        t(g, "Mau diapakan file ini?", x, top + 20.0 + 5.0 + 9.0, Face::Regular, 13.0, hex(pal::DIM));
        let by = top + 20.0 + 18.0 + 10.0;
        let (ask, aw) = self.ui.button(g, x, by, "Tanyakan sesuatu", true, None);
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

/// What an approval is about, in a sentence (the command itself stays one tap away).
fn approval_summary(tool: &str, detail: &str) -> String {
    let base = detail.rsplit(['\\', '/']).next().unwrap_or(detail).trim();
    let host = detail
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(detail);
    match tool {
        "Bash" | "PowerShell" => "Menjalankan perintah di terminal".to_string(),
        "Write" => format!("Menulis file {base}"),
        "Edit" | "MultiEdit" | "NotebookEdit" => format!("Mengubah file {base}"),
        "Read" | "LS" => format!("Membaca {base}"),
        "Glob" | "Grep" => "Mencari di dalam proyek".to_string(),
        "WebFetch" => format!("Membuka {host}"),
        "WebSearch" => "Mencari di web".to_string(),
        "Task" => "Menjalankan sub-agent".to_string(),
        other => format!("Memakai alat {other}"),
    }
}
