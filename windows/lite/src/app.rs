// The island — port of island/island.ts: modes, geometry animation, Mochi
// placement, mouse handling, the frame loop. Rendering of the views lives in
// views.rs; the Claude Code / integration event handlers in handlers.rs.

use std::cell::RefCell;

use crate::anim::{clamp, ease, lerp, now, Spring, Tracked};
use crate::ctx::{AppHandle, Event};
use crate::files;
use crate::fsm::{Fsm, Machine};
use crate::gfx::{hex, rgba, Gfx};
use crate::layout::{
    self, bot_glow_color, bot_glow_opacity, bot_position, island_size, Mode, View,
    EXPANDED_CORNER, EXPANDED_W, HIT_MARGIN, NOTCH_W, PANEL_H, PANEL_W, ROUNDED_CORNER,
};
use crate::mochi::engine::{BotEngine, BotState, Emote};
use crate::mochi::greeting::Greeting;
use crate::mochi::upload::{UploadSeq, USC};
use crate::platform::{self, Platform, TIMER_FRAME, TIMER_POLL};
use crate::settings;
use crate::sound;
use crate::state::{DroppedInfo, State, CLAUDE_ID};
use crate::tray::Tray;
use crate::ui::Ui;
use crate::views::{ChatUi, Ticker};

pub const BOT_OVERHANG: f32 = 80.0;

/// Views the drop sequence owns; leaving them stops the sequence.
pub fn is_upload_view(v: View) -> bool {
    matches!(v, View::Upload | View::Uploading | View::Choose)
}

/// Seconds between the drop and the moment the progress bar starts filling.
fn pre_progress() -> f32 {
    USC.t_prog_start - USC.t_drop
}

#[derive(Clone, Copy, Debug)]
pub enum Timer {
    ConfusedRecovery,
    ApprovalTimeout,
    TaskIdle(&'static str),
    RemoveTask(&'static str),
    IntegrationClear(&'static str),
    NoteBack,
    CollapseToStrip,
    FocusChat,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

pub fn install(app: App) {
    APP.with(|a| *a.borrow_mut() = Some(app));
}

pub fn uninstall() {
    APP.with(|a| *a.borrow_mut() = None);
}

/// Runs `f` on the app if it is not already borrowed (window messages can nest).
pub fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| {
        let mut b = a.try_borrow_mut().ok()?;
        let app = b.as_mut()?;
        let r = f(app);
        app.after_event();
        Some(r)
    })
}

/// Text that fades out at `left` / `right` instead of being clipped: a clip would allocate a
/// window-sized mask on every frame, and the compact bar redraws at 30 Hz while an activity shows.
pub fn fading_text(g: &mut Gfx, s: &str, x: f32, cy: f32, size: f32, color: tiny_skia::Color, left: f32, right: f32) {
    use crate::text::{self, Align, Face};
    let width = text::measure(s, Face::Medium, size);
    text::draw_with(g, s, x, cy, Face::Medium, size, Align::Left, |f| {
        let gx = x + f * width;
        let a = ((gx - left + 6.0) / 6.0).clamp(0.0, 1.0) * ((right - gx) / 10.0).clamp(0.0, 1.0);
        crate::gfx::with_alpha(color, color.alpha() * a)
    });
}

/// Compact bar: where the live-activity slot starts (right of Mochi) and how much room
/// the integration pills keep on the right.
const ACT_X: f32 = 56.0;
const ACT_RIGHT: f32 = 62.0;

pub struct App {
    pub platform: Platform,
    pub ctx: AppHandle,
    pub st: State,
    pub fsm: Machine,
    pub ui: Ui,
    pub gfx: Gfx,
    pub tray: Option<Tray>,

    pub engine: BotEngine,
    pub greeting: Greeting,
    pub upload: UploadSeq,
    pub ticker: Ticker,
    pub chat: ChatUi,

    pub width: Tracked,
    pub height: Tracked,
    pub radius: Tracked,
    pub bot_cx: Spring,
    pub bot_cy: Spring,
    pub bot_size: Spring,

    running: bool,
    last_frame: f32,
    timers: Vec<(f32, Timer)>,
    /// Window collapsed to the wake strip?
    collapsed: bool,

    pub home_collapse_at: Option<f32>,
    was_in_island: bool,
    bot_hovering: bool,
    bot_hover_at: Option<f32>,
    last_love: f32,
    bot_hover_start: (f32, f32),
    confused_prev_view: View,
    last_synced_view: Option<View>,
    pub view_t0: f32,

    upload_tens: i32,
    upload_done: bool,

    /// The pill detail (Vercel / n8n "more") is open.
    pub detail_open: bool,
    pub drag_catch: bool,
    pub slider_drag: bool,
    had_activity: bool,
    dash_t: f32,
    pub todos: Vec<crate::todos::Todo>,
    pub done_gids: Vec<String>,
    pub todo_input: crate::textfield::TextField,
    quit: bool,
}

impl App {
    pub fn new(ctx: AppHandle) -> App {
        let platform = Platform::new(&ctx.settings().screen);
        Self::build(ctx, platform, true)
    }

    /// No window, no tray: renders into memory for the snapshot tool.
    #[cfg_attr(not(feature = "snapshot"), allow(dead_code))]
    pub fn headless(ctx: AppHandle, scale: f32) -> App {
        Self::build(ctx, Platform::headless(scale), false)
    }

    fn build(ctx: AppHandle, platform: Platform, with_tray: bool) -> App {
        let st = State::new(ctx.settings());
        let (pw, ph) = platform.phys_size();
        let mut fsm = Machine::new();
        fsm.home_to_petit = st.settings.auto_close_interval as f32;
        fsm.stay_visible = st.settings.stay_visible;
        sound::set_enabled(st.settings.sound_enabled);
        sound::set_volume(st.settings.sound_volume as f32);
        let tray = if with_tray { Some(Tray::new(platform.hwnd)) } else { None };
        let gfx = Gfx::new(pw as u32, ph as u32, platform.scale);
        let mut app = App {
            platform,
            ctx,
            st,
            fsm,
            ui: Ui::new(),
            gfx,
            tray,
            engine: BotEngine::new(),
            greeting: Greeting::new(),
            upload: UploadSeq::new(),
            ticker: Ticker::new(),
            chat: ChatUi::new(),
            width: Tracked::new(NOTCH_W),
            height: Tracked::new(0.0),
            radius: Tracked::new(ROUNDED_CORNER),
            bot_cx: Spring::new(46.0),
            bot_cy: Spring::new(16.0),
            bot_size: Spring::new(10.0),
            running: false,
            last_frame: now(),
            timers: Vec::new(),
            collapsed: false,
            home_collapse_at: None,
            was_in_island: false,
            bot_hovering: false,
            bot_hover_at: None,
            last_love: -100.0,
            bot_hover_start: (0.0, 0.0),
            confused_prev_view: View::Overview,
            last_synced_view: None,
            view_t0: 0.0,
            upload_tens: 0,
            upload_done: false,
            detail_open: false,
            drag_catch: false,
            slider_drag: false,
            had_activity: false,
            dash_t: 0.0,
            todos: crate::todos::load(),
            done_gids: Vec::new(),
            todo_input: crate::textfield::TextField::new("Tambah tugas…", false),
            quit: false,
        };
        app.refresh_flags();
        app
    }

    /// Hooks / key presence, as the idle cards and the in-island settings show them.
    pub fn refresh_flags(&mut self) {
        let status = crate::hooks::status();
        self.st.hooks_installed = status.installed;
        self.st.has_api_key = crate::secrets::present("anthropic-api-key");
        let keys = [
            ("integration_stripe", "stripe-api-key"),
            ("integration_github", "github-token"),
            ("integration_vercel", "vercel-token"),
            ("integration_n8n", "n8n-api-key"),
            ("integration_resend", "resend-api-key"),
            ("integration_notion", "notion-api-key"),
            ("integration_calcom", "calcom-api-key"),
        ];
        for (id, key) in keys {
            let present = crate::secrets::present(key);
            self.st.integrations.entry(id).or_default().configured = present;
        }
        let hooks = self.st.hooks_installed;
        self.st.integrations.entry(CLAUDE_ID).or_default().configured = hooks;
    }

    /// Snapshot helper: jump straight to a mode/view with no animation.
    #[cfg_attr(not(feature = "snapshot"), allow(dead_code))]
    pub fn debug_show(&mut self, mode: Mode, view: View) {
        self.st.mode = mode;
        self.st.view = view;
        self.collapsed = false;
        let (w, h, r) = self.target_size();
        self.width.jump(w);
        self.height.jump(h);
        self.radius.jump(r);
        let p = bot_position(mode, view, h, 0.0);
        self.bot_cx.set(p.cx);
        self.bot_cy.set(p.cy);
        self.bot_size.set(p.d / 0.6);
        self.view_t0 = now() - 1.0;
        self.last_synced_view = Some(view);
    }

    pub fn launch(&mut self) {
        self.platform.show();
        self.fsm.launch();
        self.process_transitions();
        self.ensure_running();
    }

    pub fn quit(&mut self) {
        self.quit = true;
        unsafe { windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0) };
    }

    // ── Timers (setTimeout) ───────────────────────────────────────────────────

    pub fn later(&mut self, secs: f32, t: Timer) {
        self.timers.push((now() + secs, t));
        self.ensure_poll();
    }

    pub fn cancel_timer(&mut self, pred: impl Fn(&Timer) -> bool) {
        self.timers.retain(|(_, t)| !pred(t));
    }

    fn run_timers(&mut self) {
        let n = now();
        let due: Vec<Timer> = {
            let mut due = Vec::new();
            self.timers.retain(|(at, t)| {
                if *at <= n {
                    due.push(*t);
                    false
                } else {
                    true
                }
            });
            due
        };
        for t in due {
            self.fire(t);
        }
    }

    fn fire(&mut self, t: Timer) {
        match t {
            Timer::ConfusedRecovery => {
                self.st.state_override = None;
                self.engine.set_state(self.st.effective_state(), false);
                if self.st.view == View::Confused {
                    let fallback = self.st.default_view();
                    let prev = if self.confused_prev_view == View::Confused { fallback } else { self.confused_prev_view };
                    self.set_view(prev);
                }
                self.engine.trigger_emote(Emote::Happy, 1.8);
            }
            Timer::ApprovalTimeout => self.approval_timeout(),
            Timer::TaskIdle(id) => {
                self.st.update_task(id, BotState::Idle);
                self.st.set_pill_badge(id, None);
            }
            Timer::RemoveTask(id) => self.st.remove_task(id),
            Timer::IntegrationClear(id) => {
                if let Some(t) = self.st.task_mut(id) {
                    if t.state == BotState::Finished || t.state == BotState::Error {
                        t.state = BotState::Idle;
                        t.mini.set_state(BotState::Idle, false);
                        t.steps.clear();
                        t.step_index = 0;
                        t.pill_badge = None;
                    }
                }
            }
            Timer::NoteBack => {
                let v = self.st.default_view();
                self.set_view(v);
            }
            Timer::CollapseToStrip => {
                if self.st.mode == Mode::Hidden && !self.collapsed {
                    self.collapsed = true;
                    self.platform.apply_geometry(&self.st.settings.screen, true);
                    self.platform.set_poll_timer(false);
                    self.platform.set_frame_timer(false);
                    self.running = false;
                }
            }
            Timer::FocusChat => {
                if self.st.view == View::Prompt {
                    self.chat.input.focused = true;
                    self.chat.input.select_all();
                }
            }
        }
    }

    // ── Frame loop ────────────────────────────────────────────────────────────

    pub fn ensure_running(&mut self) {
        if self.running {
            return;
        }
        self.running = true;
        self.last_frame = now();
        self.platform.set_frame_timer(true);
        self.ensure_poll();
    }

    fn ensure_poll(&mut self) {
        if self.st.mode != Mode::Hidden || !self.timers.is_empty() || self.fsm.has_deadline() {
            self.platform.set_poll_timer(true);
        }
    }

    pub fn on_timer(&mut self, id: usize) {
        match id {
            TIMER_FRAME => self.frame(),
            TIMER_POLL => self.poll(),
            _ => {}
        }
    }

    /// ~30 Hz while the island is visible: cursor, FSM deadlines, timers.
    fn poll(&mut self) {
        let n = now();
        self.run_timers();
        self.fsm.tick(n);
        self.process_transitions();

        if self.st.mode != Mode::Hidden {
            let (cx, cy) = self.platform.cursor_logical();
            self.on_cursor(cx, cy);

            // A file being dragged has to be able to find us: while a button is
            // held anywhere over the panel, the whole panel takes the mouse.
            let (lw, lh) = self.platform.logical_size();
            let down = platform::left_button_down();
            let over = cx >= 0.0 && cx <= lw && cy >= 0.0 && cy <= lh;
            let catch = down && over;
            if catch != self.drag_catch {
                self.drag_catch = catch;
                self.render_now();
            }

            // Blinks in the idle island: wake the frame loop only for the blink itself.
            if !self.running && self.engine.blink_due() {
                self.ensure_running();
            }

            // Live activities: keep frames coming while one is shown, and once more
            // when it ends so the slot clears.
            if crate::activity::take_finished_timer() {
                sound::play("finish");
                self.ensure_running();
            }
            if let Some(remote) = crate::gtasks::take_remote() {
                self.merge_remote_todos(remote);
                if self.st.view == View::Dashboard && self.st.mode == Mode::Expanded {
                    self.render_now();
                }
            }
            // The dashboard shows a live clock and track position: redraw twice a second.
            if self.st.mode == Mode::Expanded && self.st.view == View::Dashboard && n - self.dash_t > 0.5 {
                self.dash_t = n;
                self.render_now();
            }
            if self.st.mode == Mode::Compact {
                let on = crate::activity::current().is_some();
                if !self.running && (on || self.had_activity) {
                    self.ensure_running();
                }
            }
        }

        // Nothing left to watch: stop polling so a hidden island costs nothing.
        if self.st.mode == Mode::Hidden && self.timers.is_empty() && !self.fsm.has_deadline() {
            self.platform.set_poll_timer(false);
        }
    }

    pub fn frame(&mut self) {
        let n = now();
        let dt = (n - self.last_frame).clamp(0.0, 0.05);
        self.last_frame = n;

        self.run_timers();
        self.fsm.tick(n);
        self.process_transitions();

        self.width.step(dt);
        self.height.step(dt);
        self.radius.step(dt);

        self.update_bot_targets();
        self.bot_cx.step(dt);
        self.bot_cy.step(dt);
        self.bot_size.step(dt);

        self.engine.set_state(self.st.effective_state(), false);
        let greeting_active = self.st.mode == Mode::Expanded && self.st.view == View::Greeting;
        if greeting_active {
            self.greeting.update(n);
            if self.greeting.take_complete() {
                self.fsm.greet_complete(n);
                self.process_transitions();
            }
        } else {
            self.update_engine(dt);
        }
        for t in &mut self.st.tasks {
            t.mini.update(dt);
        }
        self.ticker.tick(n, self.st.focus_task());
        if self.upload.is_active() {
            self.step_sequence();
        }
        self.drain_sounds();

        if self.engine.dizzy_triggered {
            self.engine.dizzy_triggered = false;
            self.handle_dizzy();
        }

        self.render_now();

        let settling = self.width.animating() || self.height.animating() || self.radius.animating();
        let activity_on = self.st.mode == Mode::Compact && crate::activity::current().is_some();
        self.had_activity = activity_on;
        let busy = if self.st.mode == Mode::Hidden {
            settling
        } else {
            settling
                || !self.bot_cx.settled()
                || !self.bot_cy.settled()
                || !self.bot_size.settled()
                || greeting_active
                || self.engine.busy()
                || self.upload.is_active()
                || self.ticker.animating()
                || self.chat.sending
                || activity_on
        };
        if !busy {
            self.running = false;
            self.platform.set_frame_timer(false);
        } else {
            // Motion that is only ambient (bouncing, beating, blinking) does not need 60 fps.
            let hard = settling
                || !self.bot_cx.settled()
                || !self.bot_cy.settled()
                || !self.bot_size.settled()
                || greeting_active
                || self.upload.is_active()
                || self.ticker.animating()
                || self.engine.in_transition();
            self.platform.set_frame_interval(if hard { 16 } else { 33 });
        }
    }

    fn update_engine(&mut self, dt: f32) {
        let focus = self.st.focus_task();
        self.engine.body_color = match focus {
            Some(t) if t.id != CLAUDE_ID => Some(crate::mochi::engine::hex_to_rgb(t.color)),
            _ => None,
        };
        // Claude Code's own colour is white-ish: keep Mochi plum for it.
        self.engine.particle_overhang = BOT_OVERHANG;
        let (lx, ly) = self.look();
        self.engine.look_x = lx;
        self.engine.look_y = ly;
        if self.engine.morph > 0.3 {
            self.engine.slot_h_target = if self.st.file_drag_over { 0.2 } else { 0.0 };
        } else {
            self.engine.slot_h_target = 0.0;
            if self.engine.morph < 0.05 {
                self.engine.slot_h = 0.0;
            }
        }
        self.engine.update(dt);
    }

    /// BotCanvasView.lookX / lookY — tanh of the distance to the bot.
    fn look(&self) -> (f32, f32) {
        let iw = self.width.value();
        let ix = (PANEL_W - iw) / 2.0;
        let bot_x = ix + self.bot_cx.value;
        (
            ((self.st.mouse.0 - bot_x) / 260.0).tanh(),
            -((self.st.mouse.1 - self.bot_cy.value) / 200.0).tanh(),
        )
    }

    fn drain_sounds(&mut self) {
        for s in self.engine.take_sounds() {
            sound::play(s);
        }
        for t in &mut self.st.tasks {
            t.mini.take_sounds();
        }
    }

    // ── After every external event ────────────────────────────────────────────

    /// Called after each message handler: render once and keep the loop alive if needed.
    pub fn after_event(&mut self) {
        self.process_transitions();
        self.ui.end_frame();
        if self.running {
            return;
        }
        // Input events render a still frame; animations keep the timer.
        if self.engine.busy() || self.width.animating() || self.height.animating() {
            self.ensure_running();
        }
    }

    pub fn render_now(&mut self) {
        let (pw, ph) = self.platform.phys_size();
        if self.gfx.width() as i32 != pw || self.gfx.height() as i32 != ph || (self.gfx.scale() - self.platform.scale).abs() > 1e-4 {
            self.gfx = Gfx::new(pw as u32, ph as u32, self.platform.scale);
        }
        self.render();
        // Nothing is ever drawn below the island plus a little hang-room.
        let used = ((self.height.value() + 70.0) * self.platform.scale).ceil() as usize;
        let used = if self.collapsed { usize::MAX } else { used };
        let pm = &self.gfx.pm;
        // Borrow dance: present needs &mut platform and the pixmap.
        let pm_ptr: *const tiny_skia::Pixmap = pm;
        // SAFETY: `gfx` and `platform` are disjoint fields; the pixmap is only read.
        self.platform.present(unsafe { &*pm_ptr }, used);
        self.ui.end_frame();
    }

    // ── FSM wiring ────────────────────────────────────────────────────────────

    fn process_transitions(&mut self) {
        for (from, to) in self.fsm.take_transitions() {
            match to {
                Fsm::Hidden => self.set_mode(Mode::Hidden),
                Fsm::Petit => {
                    if from == Fsm::Coucou {
                        self.greeting.interrupt();
                    } else if from == Fsm::Hidden {
                        sound::play("peek");
                    }
                    self.set_mode(Mode::Compact);
                    if from == Fsm::Coucou {
                        self.st.view = self.st.default_view();
                    }
                    if !self.was_in_island {
                        self.fsm.mouse_left(now());
                    }
                }
                Fsm::Home => {
                    let v = self.st.default_view();
                    self.expand(v);
                    if !self.was_in_island {
                        self.fsm.mouse_left(now());
                    }
                }
                Fsm::Coucou => {
                    self.expand(View::Greeting);
                    self.greeting.start();
                }
            }
        }
    }

    // ── Mode / view ───────────────────────────────────────────────────────────

    fn set_mode(&mut self, mode: Mode) {
        let prev = self.st.mode;
        if mode == prev {
            return;
        }
        self.st.mode = mode;
        if mode == Mode::Expanded {
            sound::play("open");
        }
        if prev == Mode::Expanded {
            sound::play("close");
            self.st.is_pinned = false;
            self.platform.set_activating(false);
            self.chat.input.focused = false;
            self.todo_input.focused = false;
        }
        if mode != Mode::Expanded {
            self.engine.reset_morph();
            self.upload.deactivate();
        }
        self.update_window_collapsed();
        self.animate_geometry(mode.order() < prev.order());
        self.ensure_running();
        self.ensure_poll();
    }

    pub fn upload_active(&self) -> bool {
        self.st.mode == Mode::Expanded && self.upload.is_active() && is_upload_view(self.st.view)
    }

    fn stop_sequence_if_leaving(&mut self, view: View) {
        if self.upload.is_active() && !is_upload_view(view) {
            self.upload.deactivate();
        }
    }

    pub fn expand(&mut self, view: View) {
        self.stop_sequence_if_leaving(view);
        self.change_view(view);
        if self.st.mode != Mode::Expanded {
            self.set_mode(Mode::Expanded);
        } else {
            self.animate_geometry(false);
        }
        self.home_collapse_at = None;
        self.ensure_running();
    }

    fn change_view(&mut self, view: View) {
        if self.st.view != view {
            self.view_t0 = now();
            self.detail_open = false;
        }
        self.st.view = view;
        self.sync_view_focus();
    }

    pub fn set_view(&mut self, view: View) {
        self.stop_sequence_if_leaving(view);
        if self.st.mode != Mode::Expanded {
            self.fsm.force_home();
            self.change_view(view);
            self.animate_geometry(false);
            self.process_transitions();
            self.ensure_running();
            return;
        }
        let grew = layout::layout(view).height >= layout::layout(self.st.view).height;
        self.change_view(view);
        self.animate_geometry(!grew);
        self.ensure_running();
    }

    /// The chat is the only view with a text field, so it is the only time the
    /// island may take keyboard focus.
    fn sync_view_focus(&mut self) {
        let v = self.st.view;
        if self.last_synced_view == Some(v) {
            return;
        }
        let was_chat = self.last_synced_view == Some(View::Prompt);
        if self.last_synced_view == Some(View::Dashboard) && v != View::Dashboard && self.todo_input.focused {
            self.todo_input.focused = false;
            self.platform.set_activating(false);
        }
        self.last_synced_view = Some(v);
        if v == View::Prompt {
            self.platform.set_activating(true);
            self.later(0.12, Timer::FocusChat);
        } else if was_chat {
            self.platform.set_activating(false);
            self.chat.input.focused = false;
        }
    }

    pub fn collapse(&mut self) {
        self.st.is_pinned = false;
        self.fsm.pinned = false;
        self.fsm.force_petit();
        self.process_transitions();
    }

    /// Alert from the hook server: open on this view. Pinned alerts never auto-close.
    pub fn alert(&mut self, view: View) {
        self.fsm.pinned = self.st.is_pinned;
        self.fsm.force_home();
        self.process_transitions();
        self.expand(view);
    }

    pub fn reveal(&mut self) {
        self.fsm.reveal(now());
        self.process_transitions();
    }

    pub fn drop_pin(&mut self) {
        self.fsm.pinned = false;
    }

    // ── Geometry ──────────────────────────────────────────────────────────────

    fn target_size(&self) -> (f32, f32, f32) {
        let (w, h) = island_size(self.st.mode, self.st.view, self.st.chat_history.len());
        let r = if self.st.mode == Mode::Expanded { EXPANDED_CORNER } else { ROUNDED_CORNER };
        (w, h, r)
    }

    pub fn animate_geometry(&mut self, shrinking: bool) {
        let (w, h, r) = self.target_size();
        if shrinking {
            self.width.curve_towards(w);
            self.height.curve_towards(h);
            self.radius.curve_towards(r);
        } else {
            self.width.spring_to(w);
            self.height.spring_to(h);
            self.radius.spring_to(r);
        }
        self.ensure_running();
    }

    fn update_window_collapsed(&mut self) {
        self.cancel_timer(|t| matches!(t, Timer::CollapseToStrip));
        if self.st.mode == Mode::Hidden {
            // Let the island finish retracting, then drop the window to the wake
            // strip: from there the OS delivers no cursor events, so nothing polls.
            self.later(0.42, Timer::CollapseToStrip);
        } else if self.collapsed {
            // Grow the window back before the island animates open.
            self.collapsed = false;
            self.platform.apply_geometry(&self.st.settings.screen, false);
        }
    }

    pub fn island_rect(&self) -> (f32, f32, f32, f32) {
        let w = self.width.value();
        ((PANEL_W - w) / 2.0, 0.0, w, self.height.value())
    }

    fn update_bot_targets(&mut self) {
        let p = bot_position(self.st.mode, self.st.view, self.height.value(), self.st.upload_progress);
        self.bot_cx.target = p.cx;
        self.bot_cy.target = p.cy;
        self.bot_size.target = p.d / 0.6;
    }

    // ── Input ─────────────────────────────────────────────────────────────────

    pub fn on_mouse_move(&mut self, x: f32, y: f32) {
        if self.collapsed {
            // The wake strip is the only thing the OS can hit while hidden.
            if self.st.mode == Mode::Hidden {
                self.fsm.mouse_entered(now());
                self.process_transitions();
            }
            return;
        }
        self.on_cursor(x, y);
        if self.st.mode != Mode::Hidden {
            self.render_now();
        }
    }

    pub fn on_mouse_down(&mut self, x: f32, y: f32) {
        self.on_cursor(x, y);
        self.platform.capture(true);
        let (ix, _, _, _) = self.island_rect();
        self.ui.input.mouse = (x - ix, y);
        self.ui.input.inside = true;
        self.ui.input.down = true;
        self.ui.input.pressed = true;

        if self.st.mode == Mode::Compact {
            let (lx, _, iw, _) = self.island_rect();
            let local = x - lx;
            if local >= ACT_X && local <= iw - ACT_RIGHT {
                match crate::activity::current() {
                    Some(crate::activity::Activity::Media(_)) => {
                        crate::activity::toggle_media();
                        self.ui.input.pressed = false;
                        return;
                    }
                    Some(crate::activity::Activity::Timer { .. }) => {
                        crate::activity::cancel_timer();
                        self.ui.input.pressed = false;
                        self.ensure_running();
                        return;
                    }
                    _ => {}
                }
            }
        }
        if self.st.mode != Mode::Expanded {
            self.fsm.click();
            self.process_transitions();
            self.ui.input.pressed = false;
            return;
        }
        if self.is_bot_hit(x, y) && !self.upload_active() {
            self.cancel_bot_hover();
            self.engine.slap();
            self.drain_sounds();
        }
        // Text fields claim the click before the frame draws.
        self.chat.on_mouse_down(self.ui.input.mouse.0);
        self.render_now_keep_edges();
    }

    pub fn on_mouse_up(&mut self, x: f32, y: f32) {
        let (ix, _, _, _) = self.island_rect();
        self.ui.input.mouse = (x - ix, y);
        self.ui.input.down = false;
        self.ui.input.released = true;
        self.platform.capture(false);
        self.slider_drag = false;
        if self.st.mode == Mode::Expanded {
            self.render_now_keep_edges();
        }
    }

    pub fn on_wheel(&mut self, delta: f32) {
        self.ui.input.wheel += delta;
        if self.st.mode == Mode::Expanded {
            self.render_now_keep_edges();
        }
    }

    /// Renders a frame in response to an input edge; `render_now` clears the edges after.
    fn render_now_keep_edges(&mut self) {
        self.render_now();
        self.ensure_running();
    }

    pub fn on_char(&mut self, c: char) {
        if self.st.view == View::Prompt && self.chat.input.focused {
            self.chat.input.on_char(c);
            self.render_now();
        } else if self.st.view == View::Dashboard && self.todo_input.focused {
            self.todo_input.on_char(c);
            self.render_now();
        }
    }

    pub fn on_key(&mut self, vk: u32) -> bool {
        use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
        if vk == VK_ESCAPE.0 as u32 && self.st.mode == Mode::Expanded && !self.st.is_pinned {
            self.collapse();
            return true;
        }
        if self.st.view == View::Prompt && self.chat.input.focused {
            let handled = self.chat.input.on_key(vk);
            if self.chat.input.take_submit() {
                self.submit_chat();
            }
            self.render_now();
            return handled;
        }
        if self.st.view == View::Dashboard && self.todo_input.focused {
            let handled = self.todo_input.on_key(vk);
            if self.todo_input.take_submit() {
                self.add_todo();
            }
            self.render_now();
            return handled;
        }
        false
    }

    pub fn on_focus_lost(&mut self) {
        self.chat.input.focused = false;
        self.todo_input.focused = false;
    }

    /// Cursor in window-logical coordinates.
    pub fn on_cursor(&mut self, x: f32, y: f32) {
        self.st.mouse = (x, y);
        let (ix, iy, iw, ih) = self.island_rect();
        self.st.mouse_in_island = (x - ix, y - iy);
        self.ui.input.mouse = self.st.mouse_in_island;

        // Windows sends no cursor position with an OLE drag, so the drop sequence
        // is fed from the cursor poll instead — it runs throughout the drag.
        if self.upload.is_active() && !self.upload.dropped() {
            self.upload.update_cursor(self.st.mouse_in_island.0, self.st.mouse_in_island.1);
        }

        let in_island = x >= ix - HIT_MARGIN && x <= ix + iw + HIT_MARGIN && y >= iy - HIT_MARGIN && y <= iy + ih + HIT_MARGIN;
        self.ui.input.inside = in_island && x >= ix && x <= ix + iw && y <= ih + 40.0;

        let n = now();
        if in_island && !self.was_in_island {
            if self.fsm.state == Fsm::Coucou {
                self.greeting.hover();
            }
            self.fsm.mouse_entered(n);
            self.home_collapse_at = None;
        }
        if !in_island && self.was_in_island {
            self.fsm.mouse_left(n);
            if self.fsm.state == Fsm::Home && !self.st.is_pinned {
                self.home_collapse_at = Some(n + self.st.settings.auto_close_interval as f32);
            }
        }
        self.was_in_island = in_island;
        self.process_transitions();

        // Bot hover → love
        let over_bot = self.st.mode == Mode::Expanded && self.st.state_override.is_none() && self.is_bot_hit(x, y);
        if over_bot && !self.bot_hovering {
            self.bot_hover_in(x, y);
        }
        if !over_bot && self.bot_hovering {
            self.cancel_bot_hover();
        }
        self.bot_hovering = over_bot;
        if self.bot_hovering {
            let d = ((x - self.bot_hover_start.0).powi(2) + (y - self.bot_hover_start.1).powi(2)).sqrt();
            if d > 40.0 {
                self.bot_hover_start = (x, y);
                self.schedule_love();
            }
        }
        if let Some(at) = self.bot_hover_at {
            if n >= at {
                self.bot_hover_at = None;
                if self.bot_hovering && self.st.state_override.is_none() && n - self.last_love >= 6.0 {
                    self.last_love = n;
                    self.engine.trigger_emote(Emote::Love, 1.8);
                    sound::play("love");
                    self.ensure_running();
                }
            }
        }
        if self.st.mode != Mode::Hidden {
            self.ensure_running_if_look();
        }
    }

    /// The eyes follow the cursor, so a moving cursor needs frames.
    fn ensure_running_if_look(&mut self) {
        if self.st.mode == Mode::Expanded || self.st.mode == Mode::Compact {
            let (lx, ly) = self.look();
            if (lx - self.engine.look_x).abs() > 0.01 || (ly - self.engine.look_y).abs() > 0.01 {
                self.ensure_running();
            }
        }
    }

    fn is_bot_hit(&self, x: f32, y: f32) -> bool {
        let (ix, iy, _, _) = self.island_rect();
        let cx = ix + self.bot_cx.value;
        let cy = iy + self.bot_cy.value;
        let r = self.bot_size.value * 0.6 / 2.0;
        (x - cx).powi(2) + (y - cy).powi(2) <= r * r
    }

    fn bot_hover_in(&mut self, x: f32, y: f32) {
        if now() - self.last_love < 6.0 {
            return;
        }
        self.bot_hover_start = (x, y);
        self.engine.blink();
        self.engine.tg_es = 1.08;
        sound::play("hover");
        self.schedule_love();
        self.ensure_running();
    }

    fn schedule_love(&mut self) {
        self.bot_hover_at = Some(now() + 1.9);
    }

    fn cancel_bot_hover(&mut self) {
        self.bot_hover_at = None;
        self.engine.tg_es = 1.0;
    }

    /// Three slaps → dizzy + confused view for 3.3 s, then back.
    fn handle_dizzy(&mut self) {
        self.confused_prev_view = self.st.view;
        self.st.state_override = Some(BotState::Dizzy);
        self.engine.set_state(BotState::Dizzy, false);
        sound::play("dizzy");
        self.alert(View::Confused);
        self.cancel_timer(|t| matches!(t, Timer::ConfusedRecovery));
        self.later(3.3, Timer::ConfusedRecovery);
    }

    /// Screen position of an in-flight file drag (OLE hands it to us in device px).
    pub fn on_drag_pos(&mut self, sx: i32, sy: i32) {
        let (ox, oy) = self.platform.origin;
        let k = self.platform.scale;
        let (x, y) = ((sx - ox) as f32 / k, (sy - oy) as f32 / k);
        self.st.mouse = (x, y);
        let (ix, iy, _, _) = self.island_rect();
        self.st.mouse_in_island = (x - ix, y - iy);
        if self.upload.is_active() && !self.upload.dropped() {
            self.upload.update_cursor(self.st.mouse_in_island.0, self.st.mouse_in_island.1);
        }
    }

    pub fn on_display_change(&mut self) {
        let collapsed = self.collapsed;
        self.platform.apply_geometry(&self.st.settings.screen, collapsed);
        self.render_now();
    }

    pub fn on_open_request(&mut self) {
        self.st.paused = false;
        crate::integrations::set_paused(false);
        let v = self.st.default_view();
        self.alert(v);
    }

    // ── Tray ──────────────────────────────────────────────────────────────────

    pub fn on_tray_message(&mut self, lparam: u32) {
        use crate::tray::TrayCmd;
        let paused = self.st.paused;
        if let Some(cmd) = self.tray.as_mut().and_then(|t| t.on_message(lparam, paused)) {
            match cmd {
                TrayCmd::Open => self.on_open_request(),
                TrayCmd::Settings => {
                    self.st.paused = false;
                    crate::integrations::set_paused(false);
                    crate::settings_win::show();
                }
                TrayCmd::Pause => {
                    let on = !self.st.paused;
                    self.st.paused = on;
                    crate::integrations::set_paused(on);
                    crate::activity::set_paused(on);
                    if on {
                        self.fsm.force_hidden();
                        self.process_transitions();
                    } else {
                        self.reveal();
                    }
                }
                TrayCmd::Quit => self.quit(),
            }
            self.ensure_running();
        }
    }

    // ── Events from the backend threads ───────────────────────────────────────

    pub fn on_events(&mut self) {
        for e in self.ctx.drain() {
            match e {
                Event::Hook(p) => self.handle_hook(p),
                Event::Integration(u) => self.handle_integration(u),
                Event::ChatReply(r) => self.on_chat_reply(r),
                Event::Ingested(r) => self.on_ingested(r),
            }
        }
        self.ensure_running();
        self.render_now();
    }

    /// Settings changed in the settings window (or by a click in the island).
    pub fn apply_settings(&mut self, s: settings::Settings) {
        self.st.settings = s;
        sound::set_enabled(self.st.settings.sound_enabled);
        sound::set_volume(self.st.settings.sound_volume as f32);
        self.fsm.home_to_petit = self.st.settings.auto_close_interval as f32;
        self.fsm.set_stay_visible(self.st.settings.stay_visible);
        self.process_transitions();
        self.st.load_integration_tasks();
        self.refresh_flags();
        self.on_display_change();
    }

    pub fn save_settings(&mut self) {
        let s = self.st.settings.clone();
        let screen_changed = self.ctx.settings().screen != s.screen;
        let s2 = s.clone();
        self.ctx.update_settings(|cur| *cur = s2);
        if let Err(e) = settings::save(&s) {
            crate::log::line(format!("could not save settings: {e}"));
        }
        if screen_changed {
            self.on_display_change();
        }
        crate::settings_win::refresh();
    }

    // ── File drop ─────────────────────────────────────────────────────────────

    pub fn on_drag(&mut self, kind: DragKind) {
        if self.st.paused {
            return;
        }
        match kind {
            DragKind::Over => {
                if self.st.file_drag_over {
                    return;
                }
                self.st.file_drag_over = true;
                self.engine.animate_morph(1.0);
                // enter_zone must run before the island expands, so the sequence
                // is already active by the time the view becomes `upload`.
                self.upload.enter_zone(self.st.mouse_in_island.0, self.st.mouse_in_island.1);
                self.alert(View::Upload);
            }
            DragKind::Leave => {
                if !self.st.file_drag_over {
                    return;
                }
                self.st.file_drag_over = false;
                self.engine.animate_morph(0.0);
                // The island deliberately stays open: the drag session is still alive.
                self.upload.exit_zone();
                self.ensure_running();
            }
            DragKind::Drop(path) => {
                self.st.file_drag_over = false;
                match path {
                    None => {
                        self.engine.animate_morph(0.0);
                        let v = self.st.default_view();
                        self.set_view(v);
                    }
                    Some(p) => self.swallow(p),
                }
            }
        }
    }

    /// Mochi eats the file. Nothing here waits on the file system: the copy into
    /// the inbox runs in the background and swaps the path in when it lands.
    fn swallow(&mut self, path: String) {
        let name = path.rsplit(['\\', '/']).next().unwrap_or("file").to_string();
        self.st.dropped_file = Some(DroppedInfo { name, path: path.clone() });
        self.st.chat_history.clear();
        self.ctx.chat().reset();

        self.upload.perform_drop(self.st.upload_duration);
        self.upload_tens = 0;
        self.upload_done = false;

        self.engine.gulp();
        sound::play("approve");
        self.engine.trigger_emote(Emote::Happy, 1.8);
        self.engine.animate_morph(0.0);

        self.st.upload_progress = 0.0;
        self.set_view(View::Uploading);
        self.ensure_running();

        let ctx = self.ctx.clone();
        crate::rt::spawn(async move {
            let r = files::ingest(&path);
            ctx.push(Event::Ingested(r));
        });
    }

    fn on_ingested(&mut self, r: Result<files::DroppedFile, String>) {
        match r {
            Ok(f) => {
                self.st.dropped_file = Some(DroppedInfo { name: f.name, path: f.path });
            }
            Err(err) => {
                self.upload.deactivate();
                self.st.note_message = Some(err);
                self.engine.animate_morph(0.0);
                self.set_view(View::Note);
                sound::play("error");
                self.later(2.4, Timer::NoteBack);
            }
        }
    }

    /// Sounds and view changes hung off the canvas timeline: a `tick` every 10 %,
    /// the ✓ chime when the bar completes, then `choose` once Mochi has grown back.
    fn step_sequence(&mut self) {
        let Some(since) = self.upload.since_drop() else { return };
        let dur = self.st.upload_duration;
        let pre = pre_progress();
        let p = clamp((since - pre) / dur, 0.0, 1.0);
        self.st.upload_progress = p;

        let tens = (p * 10.0).floor() as i32;
        if tens > self.upload_tens && tens < 10 {
            self.upload_tens = tens;
            sound::play("tick");
        }
        if !self.upload_done && since >= pre + dur {
            self.upload_done = true;
            sound::play("approve");
            self.engine.trigger_emote(Emote::Happy, 1.8);
        }
        // The extra second is the grow-back, after which the choose card is up.
        if since >= pre + dur + 1.0 && self.st.view == View::Uploading {
            self.set_view(View::Choose);
        }
    }

    // ── Chat ──────────────────────────────────────────────────────────────────

    pub fn submit_chat(&mut self) {
        let query = self.chat.input.text().trim().to_string();
        if query.is_empty() || self.chat.sending {
            return;
        }
        self.chat.input.clear();
        self.chat.sending = true;
        sound::play("send");
        self.st.chat_history.push(crate::state::ChatMessage { role: crate::state::Role::User, content: query.clone() });
        self.st.state_override = Some(BotState::Thinking);
        self.animate_geometry(false);
        self.chat.scroll_to_end = true;

        let context = match (&self.st.dropped_file, self.st.chat_history.len()) {
            (Some(f), 1) => Some(crate::claude::ChatContext::File { name: f.name.clone(), path: f.path.clone() }),
            _ => None,
        };
        let model = self.ctx.settings().model;
        let ctx = self.ctx.clone();
        crate::rt::spawn(async move {
            let r = crate::claude::send(ctx.chat(), &model, query, context).await.map(|r| r.text);
            ctx.push(Event::ChatReply(r));
        });
        self.ensure_running();
    }

    fn on_chat_reply(&mut self, r: Result<String, String>) {
        self.chat.sending = false;
        self.st.state_override = None;
        match r {
            Ok(text) => {
                self.st.chat_history.push(crate::state::ChatMessage { role: crate::state::Role::Assistant, content: text });
                sound::play("finish");
                self.chat.scroll_to_end = true;
                self.animate_geometry(false);
                if self.st.view == View::Prompt {
                    self.chat.input.focused = true;
                }
            }
            Err(err) => {
                self.st.note_message = Some(err.trim_start_matches("Error:").trim().to_string());
                self.set_view(View::Note);
                sound::play("error");
                self.later(2.4, Timer::NoteBack);
            }
        }
    }

    // ── Rendering ─────────────────────────────────────────────────────────────

    fn render(&mut self) {
        let mut g = std::mem::replace(&mut self.gfx, Gfx::new(1, 1, 1.0));
        self.render_into(&mut g);
        self.gfx = g;
    }

    pub fn render_into(&mut self, g: &mut Gfx) {
        g.clear();

        // The wake strip: invisible, but hit-testable.
        if self.collapsed {
            g.fill_style(rgba(0, 0, 0, 1.0 / 255.0));
            g.fill_rect(0.0, 0.0, 10_000.0, 10_000.0);
            return;
        }
        if self.drag_catch {
            g.fill_style(rgba(0, 0, 0, 1.0 / 255.0));
            g.fill_rect(0.0, 0.0, PANEL_W, PANEL_H);
        }

        let n = now();
        let (ix, _, iw, ih) = self.island_rect();
        let r = self.radius.value();
        if ih < 0.6 && self.st.mode == Mode::Hidden {
            return;
        }

        g.save();
        g.translate(ix, 0.0);

        // Island body.
        g.begin_path();
        g.round_rect4(0.0, 0.0, iw, ih, [0.0, 0.0, r, r]);
        let body = g.take_path().unwrap();
        g.fill_style(hex("#000000"));
        g.fill_path(&body);

        let expanded = self.st.mode == Mode::Expanded;
        let greeting_active = expanded && self.st.view == View::Greeting;

        // Content. Only the greeting and the drop sequence paint to the island's
        // edge and need the rounded clip; the views are inset and cost nothing extra.
        if greeting_active {
            g.save();
            g.clip_path(&body);
            g.translate((iw - EXPANDED_W) / 2.0, 0.0);
            self.greeting.draw(g);
            g.restore();
        } else if expanded {
            if self.upload_active() {
                g.save();
                g.clip_path(&body);
                g.translate((iw - EXPANDED_W) / 2.0, 0.0);
                self.draw_upload_canvas(g, n);
                g.restore();
            }
            self.draw_content(g, iw, ih, n);
        }

        // Bot, glow, minis and the countdown bar sit above the clip.
        if !greeting_active {
            self.draw_bot(g, iw, ih, &body);
        }
        if self.st.mode == Mode::Compact {
            self.draw_activity(g, iw, ih);
            self.draw_mini_grid(g, iw, ih);
        }
        self.draw_countdown(g, iw, ih, n);

        g.restore();
    }

    fn draw_bot(&mut self, g: &mut Gfx, _iw: f32, ih: f32, body: &tiny_skia::Path) {
        let p = bot_position(self.st.mode, self.st.view, ih, self.st.upload_progress);
        let visible = p.opacity > 0.0 && !self.upload_active();
        if !visible {
            return;
        }
        let size = self.bot_size.value;
        if !(size > 1.0) {
            return;
        }
        let cx = self.bot_cx.value;
        let cy = self.bot_cy.value;

        if self.st.mode == Mode::Expanded && self.st.view != View::Uploading {
            let d = p.d;
            let state = self.st.effective_state();
            let color = hex(bot_glow_color(state));
            let op = bot_glow_opacity(state);
            if op > 0.0 {
                // The glow is confined to the island's own shape.
                g.save();
                g.clip_path(body);
                let rad = d * 1.1;
                let key = crate::gfx::layer_key(&[rad, color.red(), color.green(), color.blue(), op, 2.0]);
                g.cached_layer(key, rad * 2.0, rad * 2.0, cx - rad, cy - rad, |l| {
                    let grad = l.radial(rad, rad, 0.0, rad, &[
                        (0.0, crate::gfx::with_alpha(color, op * 0.8)),
                        (0.62, crate::gfx::with_alpha(color, 0.0)),
                        (1.0, crate::gfx::with_alpha(color, 0.0)),
                    ]);
                    l.fill_style(grad);
                    l.circle(rad, rad, rad);
                    l.fill();
                });
                g.restore();
            }
        }

        g.save();
        g.translate(cx - size / 2.0, cy - size / 2.0 - BOT_OVERHANG);
        let h = size + BOT_OVERHANG;
        self.engine.draw(g, size, h);
        g.restore();
    }

    /// The live activity in the compact bar, between Mochi and the integration pills.
    fn draw_activity(&mut self, g: &mut Gfx, iw: f32, ih: f32) {
        use crate::activity::{format_bytes, format_clock, Activity};
        use crate::text::{self, Align, Face};
        use crate::ui::pal;
        let Some(act) = crate::activity::current() else { return };
        let n = now();
        let (x0, x1) = (ACT_X, iw - ACT_RIGHT);
        let cy = ih / 2.0;
        match act {
            Activity::Media(m) => {
                // Three bars that bounce while it plays.
                for i in 0..3 {
                    let ph = n * (5.0 + i as f32 * 1.7) + i as f32 * 1.3;
                    let h = 4.0 + (ph.sin() * 0.5 + 0.5) * 8.0;
                    g.fill_style(hex("#B38AFF"));
                    g.fill_round_rect(x0 + i as f32 * 4.5, cy - h / 2.0, 3.0, h, 1.5);
                }
                let tx = x0 + 18.0;
                let w = x1 - tx;
                let label = if m.artist.is_empty() { m.title.clone() } else { format!("{} · {}", m.title, m.artist) };
                let tw = text::measure(&label, Face::Medium, 11.5);
                let mut px = tx;
                if tw > w {
                    // Slow marquee with a pause at both ends.
                    let travel = tw - w + 12.0;
                    let t = (n * 0.18).fract();
                    let k = ((t * 2.0).min(1.0) - ((t - 0.5) * 2.0).max(0.0)).clamp(0.0, 1.0);
                    px = tx - travel * (k * k * (3.0 - 2.0 * k));
                }
                fading_text(g, &label, px, cy, 11.5, hex(pal::INK), tx, tx + w);
            }
            Activity::Timer { remaining, total } => {
                let r = 7.0;
                let (cx, ccy) = (x0 + r, cy);
                g.line_width(2.0);
                g.line_cap_round();
                g.stroke_style(rgba(255, 255, 255, 0.18));
                g.begin_path();
                g.circle(cx, ccy, r);
                g.stroke();
                let frac = clamp(remaining / total.max(1.0), 0.0, 1.0);
                g.stroke_style(hex(pal::AMBER));
                g.begin_path();
                let a0 = -std::f32::consts::FRAC_PI_2;
                g.arc(cx, ccy, r, a0, a0 + frac * std::f32::consts::TAU, false);
                g.stroke();
                text::draw(g, &format_clock(remaining), x0 + 2.0 * r + 8.0, cy, Face::Medium, 12.5, hex(pal::INK), Align::Left);
            }
            Activity::Download(d) => {
                // A small arrow that slides down, then the file and what has arrived so far.
                let bob = (n * 3.0).sin() * 1.5;
                g.line_width(2.0);
                g.line_cap_round();
                g.line_join_round();
                g.stroke_style(hex(pal::GREEN2));
                g.begin_path();
                g.move_to(x0 + 5.0, cy - 5.0 + bob);
                g.line_to(x0 + 5.0, cy + 3.0 + bob);
                g.move_to(x0 + 1.5, cy - 0.5 + bob);
                g.line_to(x0 + 5.0, cy + 3.0 + bob);
                g.line_to(x0 + 8.5, cy - 0.5 + bob);
                g.stroke();
                let tx = x0 + 18.0;
                let size = format_bytes(d.bytes);
                let sw = text::measure(&size, Face::Regular, 11.0);
                fading_text(g, &d.name, tx, cy, 11.5, hex(pal::INK), tx, x1 - sw - 6.0);
                text::draw(g, &size, x1, cy, Face::Regular, 11.0, hex(pal::DIM), Align::Right);
            }
        }
    }

    fn draw_mini_grid(&mut self, g: &mut Gfx, iw: f32, ih: f32) {
        let left = iw - 40.0 - 14.5;
        let top = ih / 2.0 - 14.5;
        let others: Vec<&'static str> = self.st.other_tasks().iter().take(4).map(|t| t.id).collect();
        for (i, id) in others.iter().enumerate() {
            let (col, row) = ((i % 2) as f32, (i / 2) as f32);
            let cx = left + col * 16.0 + 6.5;
            let cy = top + row * 16.0 + 6.5;
            let eng_size = 13.0 / 0.6;
            if let Some(t) = self.st.task_mut(id) {
                g.save();
                g.translate(cx - eng_size / 2.0, cy - eng_size / 2.0);
                t.mini.draw(g, eng_size, eng_size);
                g.restore();
            }
        }
    }

    fn draw_countdown(&mut self, g: &mut Gfx, iw: f32, ih: f32, n: f32) {
        if self.st.mode != Mode::Expanded || self.st.is_pinned {
            return;
        }
        let Some(at) = self.home_collapse_at else { return };
        let auto = self.st.settings.auto_close_interval as f32;
        let window = (auto * 0.6).min(10.0);
        let remaining = at - n;
        if remaining < window {
            let w = clamp(remaining / window, 0.0, 1.0) * 160.0;
            g.fill_style(rgba(255, 255, 255, 0.35));
            g.fill_rect((iw - w) / 2.0, ih - 2.0, w, 2.0);
        }
    }

    pub fn view_alpha_scale(&self) -> (f32, f32) {
        let t = clamp((now() - self.view_t0) / 0.3, 0.0, 1.0);
        (ease::out(t), lerp(0.97, 1.0, ease::back(t)))
    }

}

pub enum DragKind {
    Over,
    Leave,
    Drop(Option<String>),
}
