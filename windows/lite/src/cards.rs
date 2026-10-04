// Integration cards shown in the overview's left card — native ports of
// views/integrations.ts (IntegrationCardView and friends from
// IslandViewContent.swift).
//
// Cal.com is the one simplification: macOS shows a three-level calendar
// (month → day → booking); here it is the list of upcoming bookings.

use serde_json::Value;
use tiny_skia::Color;

use crate::app::App;
use crate::gfx::{hex, rgba, with_alpha, Gfx};
use crate::icons::{self, Icon};
use crate::mochi::engine::BotState;
use crate::state::{IntegrationInfo, CLAUDE_ID};
use crate::text::{self, Align, Face};
use crate::ui::{self, id_of, pal, Rect};
use crate::util::{as_epoch, local_day_time, time_ago};
use crate::views::{open_in_vscode, open_url, open_url_for};

const LEFT_PAD: f32 = 108.0;

fn arr<'a>(info: &'a IntegrationInfo, key: &str) -> Vec<&'a Value> {
    info.data.get(key).and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default()
}

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn t(g: &mut Gfx, text_: &str, x: f32, cy: f32, face: Face, size: f32, color: Color) -> f32 {
    text::draw(g, text_, x, cy, face, size, color, Align::Left);
    text::measure(text_, face, size)
}

/// True when this integration has data worth showing instead of the idle card.
fn has_data(id: &str, info: Option<&IntegrationInfo>) -> bool {
    let Some(info) = info else { return false };
    if info.error.is_some() {
        return false;
    }
    match id {
        "integration_vercel" => !arr(info, "deployments").is_empty(),
        "integration_resend" => !arr(info, "emails").is_empty(),
        "integration_github" => info.data.get("totalRepos").is_some(),
        "integration_stripe" => info.loaded,
        "integration_notion" => !arr(info, "pages").is_empty(),
        "integration_calcom" => info.loaded,
        _ => false,
    }
}

impl App {
    pub fn draw_integration_card(&mut self, g: &mut Gfx, area: Rect) {
        let Some(task) = self.st.focus_task() else { return };
        let id = task.id;
        let name = task.name.clone();
        let color = task.color;
        let state = task.state;
        let steps = task.steps.clone();
        let info = self.st.integration(id).cloned();
        let activity = !steps.is_empty() && (state == BotState::Finished || state == BotState::Error);

        let a = Rect::new(area.x + LEFT_PAD, area.y, area.w - LEFT_PAD - 12.0, area.h);

        if id == "integration_n8n" {
            if self.detail_open && activity {
                self.n8n_detail(g, a, state == BotState::Finished, &steps);
            } else if activity {
                self.n8n_card(g, a, state == BotState::Finished, &steps);
            } else {
                self.idle_card(g, a, id, &name, color, info.as_ref());
            }
            return;
        }
        if !has_data(id, info.as_ref()) {
            self.idle_card(g, a, id, &name, color, info.as_ref());
            return;
        }
        let info = info.unwrap();
        match id {
            "integration_vercel" => {
                if self.detail_open {
                    self.vercel_detail(g, a, &info);
                } else {
                    self.vercel_card(g, a, &info);
                }
            }
            "integration_resend" => self.resend_card(g, a, &info),
            "integration_github" => self.github_card(g, a, &info),
            "integration_stripe" => self.stripe_card(g, a, &info),
            "integration_notion" => self.notion_card(g, a, &info),
            "integration_calcom" => self.calcom_card(g, a, &info),
            _ => self.idle_card(g, a, id, &name, color, Some(&info)),
        }
    }

    fn header(&self, g: &mut Gfx, a: Rect, color: &str, name: &str, kind: &str) -> f32 {
        let cy = a.y + 19.0;
        ui::dot(g, a.x + 3.5, cy, 7.0, hex(color));
        let w = t(g, name, a.x + 13.0, cy, Face::Medium, 12.0, hex(pal::INK));
        t(g, kind, a.x + 13.0 + w + 6.0, cy, Face::Regular, 11.0, hex(pal::DIM2));
        cy
    }

    /// Highlighted first row + plain rows, the layout every list card shares.
    fn list_row(&self, g: &mut Gfx, a: Rect, y: f32, accent: &str, first: bool) {
        if first {
            g.fill_style(with_alpha(hex(accent), 0.08));
            g.fill_round_rect(a.x - 4.0, y, a.w + 4.0, 20.0, 8.0);
        }
        ui::dot(g, a.x + 3.0, y + 10.0, 5.0, hex(accent));
    }

    // ── Not configured / idle ─────────────────────────────────────────────────

    fn idle_card(&mut self, g: &mut Gfx, a: Rect, id: &'static str, name: &str, color: &'static str, info: Option<&IntegrationInfo>) {
        let configured = info.map(|i| i.configured).unwrap_or(false);
        let error = info.and_then(|i| i.error.clone());
        // The Claude Code pill is about hooks, not a key.
        let missing = if id == CLAUDE_ID { "Hooks not installed" } else { "Key not configured" };
        let label = error.clone().unwrap_or_else(|| if configured { "Connected · loading…".into() } else { missing.into() });
        let status = if error.is_some() || !configured { pal::RED } else { pal::GREEN };
        let title = if id == CLAUDE_ID { "Claude Code" } else { name };

        self.header(g, a, color, title, "Integration");
        let sy = a.y + 39.0;
        ui::dot(g, a.x + 3.0, sy, 5.0, hex(status));
        let label = text::ellipsize(&label, Face::Regular, 11.5, a.w - 14.0);
        t(g, &label, a.x + 13.0, sy, Face::Regular, 11.5, hex(pal::DIM2));

        let ay = a.y + 60.0;
        let mut x = a.x;
        let c = hex(color);
        if id == CLAUDE_ID {
            let (clicked, w) = self.ui.link(g, x, ay, "Open Visual Studio Code", with_alpha(c, 0.7));
            if clicked {
                let cwd = self.st.task(id).and_then(|t| t.session_cwd.clone());
                open_in_vscode(cwd.as_deref());
            }
            x += w + 12.0;
        } else if id == "integration_n8n" {
            let (clicked, w) = self.ui.link(g, x, ay, "Open n8n", with_alpha(c, 0.85));
            if clicked {
                if let Some(url) = crate::secrets::get("n8n-url") {
                    open_url(&url);
                }
            }
            x += w + 12.0;
        } else if let Some(url) = open_url_for(id) {
            let (clicked, w) = self.ui.link(g, x, ay, &format!("Open {name}"), with_alpha(c, 0.85));
            if clicked {
                open_url(url);
            }
            x += w + 12.0;
        }
        if configured {
            let (clicked, _) = self.ui.link(g, x, ay, "Refresh", with_alpha(c, 0.85));
            if clicked {
                let ctx = self.ctx.clone();
                crate::rt::spawn(async move { crate::integrations::poll_once(ctx, id).await });
            }
        } else {
            let (clicked, _) = self.ui.link(g, x, ay, "Settings…", hex(pal::DIM2));
            if clicked {
                crate::settings_win::show();
            }
        }
    }

    // ── Vercel ────────────────────────────────────────────────────────────────

    fn vercel_card(&mut self, g: &mut Gfx, a: Rect, info: &IntegrationInfo) {
        self.header(g, a, "#7C5CFF", "Vercel", "Deployments");
        for (i, d) in arr(info, "deployments").iter().take(3).enumerate() {
            let y = a.y + 32.0 + i as f32 * 22.0;
            let accent = if s(d, "state") == "READY" { pal::GREEN } else { pal::RED };
            self.list_row(g, a, y, accent, i == 0);
            let ago = time_ago(&d["createdAt"]);
            let aw = text::measure(&ago, Face::Regular, 10.0);
            let reserved = if i == 0 { 26.0 } else { 6.0 };
            let name = text::ellipsize(s(d, "projectName"), Face::Medium, 11.5, a.w - 16.0 - aw - reserved - 8.0);
            t(g, &name, a.x + 12.0, y + 10.0, Face::Medium, 11.5, hex("#E6E7EA"));
            text::draw(g, &ago, a.x + a.w - reserved - 2.0, y + 10.0, Face::Regular, 10.0, hex(pal::DIM3), Align::Right);
            if i == 0 {
                let (cx, cy) = (a.x + a.w - 10.0, y + 10.0);
                if self.ui.icon_button(g, "vercel-more", cx, cy, 16.0, Icon::Ellipsis, 9.0) {
                    self.detail_open = true;
                }
            }
        }
    }

    fn vercel_detail(&mut self, g: &mut Gfx, a: Rect, info: &IntegrationInfo) {
        let binding = arr(info, "deployments");
        let d = binding.first().copied().cloned().unwrap_or(Value::Null);
        let state = s(&d, "state");
        let success = state == "READY";
        let accent = if success { pal::GREEN } else { pal::RED };
        let status = if success { "Ready" } else if state == "CANCELED" { "Canceled" } else { "Error" };

        self.detail_head(g, a, accent, if s(&d, "projectName").is_empty() { "Deployment" } else { s(&d, "projectName") }, status);
        let mut y = a.y + 40.0;
        if !s(&d, "commitMessage").is_empty() {
            let m = text::ellipsize(s(&d, "commitMessage"), Face::Regular, 11.5, a.w);
            t(g, &m, a.x, y, Face::Regular, 11.5, hex("#C5C8CD"));
            y += 17.0;
        }
        let mut meta = String::new();
        if !s(&d, "branch").is_empty() {
            meta.push_str(s(&d, "branch"));
            meta.push_str("  ·  ");
        }
        meta.push_str(&format!("{} ago", time_ago(&d["createdAt"])));
        t(g, &meta, a.x, y, Face::Regular, 10.5, hex(pal::DIM3));
        y += 17.0;
        if !s(&d, "url").is_empty() {
            let url = s(&d, "url").to_string();
            let label = text::ellipsize(&url, Face::Regular, 10.5, a.w);
            let (clicked, _) = self.ui.link(g, a.x, y, &label, hex("#7C5CFF"));
            if clicked {
                open_url(&format!("https://{url}"));
            }
        }
    }

    fn detail_head(&mut self, g: &mut Gfx, a: Rect, accent: &str, title: &str, badge: &str) {
        let cy = a.y + 19.0;
        if self.ui.icon_button(g, "detail-back", a.x + 7.0, cy, 16.0, Icon::ChevronLeft, 8.0) {
            self.detail_open = false;
        }
        ui::dot(g, a.x + 24.0, cy, 6.0, hex(accent));
        let tt = text::ellipsize(title, Face::Medium, 12.0, a.w - 100.0);
        let w = t(g, &tt, a.x + 33.0, cy, Face::Medium, 12.0, hex(pal::INK));
        let bw = text::measure(badge, Face::Medium, 10.0) + 12.0;
        let bx = a.x + 33.0 + w + 8.0;
        g.fill_style(with_alpha(hex(accent), 0.14));
        g.fill_round_rect(bx, cy - 8.0, bw, 16.0, 8.0);
        text::draw(g, badge, bx + bw / 2.0, cy, Face::Medium, 10.0, hex(accent), Align::Center);
    }

    // ── Resend ────────────────────────────────────────────────────────────────

    fn resend_card(&mut self, g: &mut Gfx, a: Rect, info: &IntegrationInfo) {
        let cy = self.header(g, a, "#22C55E", "Resend", "Emails");
        if let Some(total) = info.data.get("total").filter(|v| !v.is_null()) {
            let label = total.to_string().trim_matches('"').to_string();
            let tw = text::measure(&label, Face::Medium, 11.0);
            let x = a.x + a.w - tw;
            let pulse = 0.5 + 0.5 * (crate::anim::now() * 3.0).sin();
            ui::dot(g, x - 10.0, cy, 5.0 + pulse, with_alpha(hex(pal::GREEN), 0.4 + 0.6 * pulse));
            text::draw(g, &label, a.x + a.w, cy, Face::Medium, 11.0, hex("#9AE6B4"), Align::Right);
        }
        for (i, e) in arr(info, "emails").iter().take(3).enumerate() {
            let y = a.y + 32.0 + i as f32 * 22.0;
            let delivered = s(e, "lastEvent") == "delivered";
            let accent = if delivered { pal::GREEN } else { pal::RED };
            self.list_row(g, a, y, accent, i == 0);
            let to = e.get("to").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_str).unwrap_or("?");
            let short = to.split('@').next().unwrap_or("?");
            let mut x = a.x + 12.0;
            x += t(g, &text::ellipsize(short, Face::Medium, 11.5, 70.0), x, y + 10.0, Face::Medium, 11.5, hex("#E6E7EA")) + 6.0;
            x += t(g, &time_ago(&e["createdAt"]), x, y + 10.0, Face::Regular, 10.0, hex(pal::DIM3)) + 6.0;
            if i == 0 && !s(e, "subject").is_empty() {
                let room = a.x + a.w - x;
                if room > 20.0 {
                    t(g, &text::ellipsize(s(e, "subject"), Face::Regular, 10.5, room), x, y + 10.0, Face::Regular, 10.5, hex(pal::DIM2));
                }
            }
        }
    }

    // ── GitHub ────────────────────────────────────────────────────────────────

    fn github_card(&mut self, g: &mut Gfx, a: Rect, info: &IntegrationInfo) {
        self.header(g, a, "#F4505E", "GitHub", "Overview");
        let stars = info.data.get("totalStars").and_then(Value::as_f64).unwrap_or(0.0);
        let repos = info.data.get("totalRepos").and_then(Value::as_f64).unwrap_or(0.0);
        let fmt = |n: f64| if n >= 1000.0 { format!("{:.1}k", n / 1000.0) } else { format!("{}", n as i64) };
        let rows = [
            (Icon::Star, "#F5A524", "Total stars", fmt(stars)),
            (Icon::Stack, "#6B7079", "Repositories", fmt(repos)),
        ];
        for (i, (icon, color, label, value)) in rows.iter().enumerate() {
            let y = a.y + 40.0 + i as f32 * 24.0;
            g.fill_style(rgba(255, 255, 255, 0.05));
            g.fill_round_rect(a.x - 4.0, y - 10.0, a.w + 4.0, 22.0, 8.0);
            icons::fill(g, *icon, a.x + 8.0, y, 11.0, hex(color));
            t(g, label, a.x + 22.0, y, Face::Regular, 11.5, hex(pal::DIM2));
            text::draw(g, value, a.x + a.w - 8.0, y, Face::Medium, 12.0, hex(pal::INK), Align::Right);
        }
    }

    // ── Stripe ────────────────────────────────────────────────────────────────

    fn stripe_card(&mut self, g: &mut Gfx, a: Rect, info: &IntegrationInfo) {
        self.header(g, a, "#0570DE", "Stripe", "Payments");
        let balance = info.data.get("balance").and_then(Value::as_f64).unwrap_or(0.0) / 100.0;
        let currency = info.data.get("currency").and_then(Value::as_str).unwrap_or("eur").to_uppercase();
        let bw = t(g, &format!("{balance:.2}"), a.x, a.y + 37.0, Face::Bold, 18.0, hex(pal::INK));
        t(g, &currency, a.x + bw + 6.0, a.y + 41.0, Face::Medium, 10.0, hex(pal::DIM3));
        for (i, p) in arr(info, "payments").iter().take(2).enumerate() {
            let y = a.y + 62.0 + i as f32 * 18.0;
            let ok = s(p, "status") == "succeeded";
            ui::dot(g, a.x + 3.0, y + 8.0, 5.0, hex(if ok { pal::GREEN } else { pal::RED }));
            let desc = if s(p, "description").is_empty() { "Payment" } else { s(p, "description") };
            let amount = format!("+{:.2}", p.get("amount").and_then(Value::as_f64).unwrap_or(0.0) / 100.0);
            let aw = text::measure(&amount, Face::Medium, 11.0);
            t(g, &text::ellipsize(desc, Face::Regular, 11.0, a.w - 20.0 - aw - 36.0), a.x + 12.0, y + 8.0, Face::Regular, 11.0, hex("#E6E7EA"));
            text::draw(g, &amount, a.x + a.w - 34.0, y + 8.0, Face::Medium, 11.0, hex(pal::GREEN), Align::Right);
            text::draw(g, &time_ago(&p["createdAt"]), a.x + a.w, y + 8.0, Face::Regular, 10.0, hex(pal::DIM3), Align::Right);
        }
    }

    // ── Notion ────────────────────────────────────────────────────────────────

    fn notion_card(&mut self, g: &mut Gfx, a: Rect, info: &IntegrationInfo) {
        self.header(g, a, "#E8E8E8", "Notion", "Recent");
        for (i, p) in arr(info, "pages").iter().take(3).enumerate() {
            let y = a.y + 32.0 + i as f32 * 22.0;
            let r = Rect::new(a.x - 4.0, y, a.w + 4.0, 20.0);
            let (clicked, hover, _) = self.ui.click_region(id_of(s(p, "title"), 53 + i as u64), r);
            if hover {
                g.fill_style(rgba(255, 255, 255, 0.07));
                g.fill_round_rect(r.x, r.y, r.w, r.h, 8.0);
            }
            let emoji = s(p, "emoji");
            if emoji.is_empty() {
                icons::fill(g, Icon::Doc, a.x + 7.0, y + 10.0, 10.0, hex(pal::DIM3));
            } else {
                t(g, emoji, a.x + 1.0, y + 10.0, Face::Regular, 11.0, hex(pal::INK));
            }
            let ago = time_ago(&p["lastEditedAt"]);
            let aw = text::measure(&ago, Face::Regular, 10.0);
            let title = if s(p, "title").is_empty() { "Untitled" } else { s(p, "title") };
            t(g, &text::ellipsize(title, Face::Medium, 11.5, a.w - 24.0 - aw - 8.0), a.x + 18.0, y + 10.0, Face::Medium, 11.5, hex("#E6E7EA"));
            text::draw(g, &ago, a.x + a.w - 4.0, y + 10.0, Face::Regular, 10.0, hex(pal::DIM3), Align::Right);
            if clicked {
                if let Some(u) = p.get("url").and_then(Value::as_str) {
                    open_url(u);
                }
            }
        }
    }

    // ── Cal.com ───────────────────────────────────────────────────────────────

    fn calcom_card(&mut self, g: &mut Gfx, a: Rect, info: &IntegrationInfo) {
        self.header(g, a, "#C9956A", "Cal.com", "Schedule");
        let mut bookings: Vec<(f64, &Value)> = arr(info, "bookings")
            .into_iter()
            .map(|b| (as_epoch(&b["start"]).unwrap_or(0.0), b))
            .collect();
        bookings.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
        if bookings.is_empty() {
            t(g, "No calls scheduled", a.x, a.y + 46.0, Face::Regular, 11.5, hex(pal::DIM3));
            return;
        }
        for (i, (when, b)) in bookings.iter().take(3).enumerate() {
            let y = a.y + 32.0 + i as f32 * 20.0;
            ui::dot(g, a.x + 2.0, y + 10.0, 4.0, hex("#C9956A"));
            let stamp = local_day_time(*when);
            let w = t(g, &stamp, a.x + 10.0, y + 10.0, Face::Medium, 10.5, hex("#C9956A"));
            let title = if s(b, "title").is_empty() { "Meeting" } else { s(b, "title") };
            t(g, &text::ellipsize(title, Face::Regular, 11.0, a.w - 18.0 - w), a.x + 18.0 + w, y + 10.0, Face::Regular, 11.0, hex("#E6E7EA"));
        }
    }

    // ── n8n ───────────────────────────────────────────────────────────────────

    fn n8n_card(&mut self, g: &mut Gfx, a: Rect, success: bool, steps: &[String]) {
        self.header(g, a, "#F29B38", "n8n", "Workflow");
        let accent = if success { pal::GREEN } else { pal::RED };
        let label = text::ellipsize(steps.first().map(String::as_str).unwrap_or("Workflow"), Face::Medium, 11.5, a.w - 50.0);
        let w = text::measure(&label, Face::Medium, 11.5) + 12.0 + 5.0 + 6.0 + 14.0 + 8.0;
        let r = Rect::new(a.x, a.y + 38.0, w.min(a.w), 24.0);
        let (clicked, hover, _) = self.ui.click_region(id_of("n8n-pill", 59), r);
        g.fill_style(with_alpha(hex(accent), if hover { 0.22 } else { 0.1 }));
        g.fill_round_rect(r.x, r.y, r.w, r.h, 12.0);
        g.stroke_style(with_alpha(hex(accent), 0.22));
        g.line_width(1.0);
        g.round_rect(r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0, 11.5);
        g.stroke();
        ui::dot(g, r.x + 12.0, r.cy(), 5.0, hex(accent));
        t(g, &label, r.x + 21.0, r.cy(), Face::Medium, 11.5, hex("#E6E7EA"));
        icons::fill(g, Icon::Ellipsis, r.x + r.w - 12.0, r.cy(), 9.0, hex(pal::DIM2));
        if clicked {
            self.detail_open = true;
        }
    }

    fn n8n_detail(&mut self, g: &mut Gfx, a: Rect, success: bool, steps: &[String]) {
        let accent = if success { pal::GREEN } else { pal::RED };
        let title = steps.first().map(String::as_str).unwrap_or("Workflow");
        self.detail_head(g, a, accent, title, if success { "Success" } else { "Failed" });
        match steps.get(1) {
            Some(detail) => {
                let lines = text::wrap(detail, Face::Mono, 10.5, a.w);
                for (i, l) in lines.iter().take(4).enumerate() {
                    t(g, l, a.x, a.y + 42.0 + i as f32 * 14.0, Face::Mono, 10.5, hex("#C5C8CD"));
                }
            }
            None => {
                let msg = if success { "Completed successfully." } else { "No error details available." };
                t(g, msg, a.x, a.y + 46.0, Face::Regular, 11.5, hex(pal::DIM2));
            }
        }
    }
}
