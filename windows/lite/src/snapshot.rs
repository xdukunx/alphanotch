// `coucou-lite --snapshot <dir>` renders views to PNG without opening a window.
// Time is driven by a manual clock, so every image is deterministic.

use serde_json::json;

use crate::anim::set_manual_clock;
use crate::app::App;
use crate::ctx::AppHandle;
use crate::gfx::{rgb, Gfx};
use crate::layout::{Mode, View};
use crate::mochi::engine::{BotEngine, BotState, Emote};
use crate::settings::Settings;
use crate::state::{ChatMessage, DroppedInfo, Role};

/// `--bench`: average frame time of the busiest views at 1x and 2x.
fn bench() {
    let mut clock = 10.0f64;
    {
        // The approval card at 125 % — the heaviest steady state.
        set_manual_clock(Some(clock));
        let mut app = App::headless(AppHandle::new(Settings::default()), 1.25);
        app.debug_show(Mode::Expanded, View::Approval);
        app.handle_hook(json!({"hook_event_name":"PermissionRequest","request_id":"b-1","cwd":"C:/p/coucou","tool_name":"Bash","tool_input":{"command":"curl x"}}));
        app.debug_show(Mode::Expanded, View::Approval);
        advance(&mut app, &mut clock, 1.0);
        let t0 = std::time::Instant::now();
        let n = 120;
        for _ in 0..n {
            clock += 1.0 / 60.0;
            set_manual_clock(Some(clock));
            app.frame();
        }
        println!("approval @1.25x: {:.2} ms/frame", t0.elapsed().as_secs_f64() * 1000.0 / n as f64);
    }
    for scale in [1.0f32, 2.0] {
        for (name, mode, view) in [("compact", Mode::Compact, View::Overview), ("overview", Mode::Expanded, View::Overview)] {
            set_manual_clock(Some(clock));
            let ctx = AppHandle::new(Settings::default());
            let mut app = App::headless(ctx, scale);
            app.debug_show(mode, view);
            advance(&mut app, &mut clock, 0.5);
            let t0 = std::time::Instant::now();
            let n = 120;
            for _ in 0..n {
                clock += 1.0 / 60.0;
                set_manual_clock(Some(clock));
                app.frame();
            }
            println!("{name} @{scale}x: {:.2} ms/frame", t0.elapsed().as_secs_f64() * 1000.0 / n as f64);
        }
    }
}

/// `--leak`: the live approval flow (no debug jump), watching how long frames take.
fn leak() {
    let mut clock = 10.0f64;
    let mut app = new_app(&mut clock);
    app.debug_show(Mode::Expanded, View::Overview);
    advance(&mut app, &mut clock, 0.5);
    hook(&mut app, json!({"hook_event_name":"PermissionRequest","request_id":"t-1","cwd":"C:/p/coucou","tool_name":"Bash","tool_input":{"command":"curl x"}}));
    for i in 0..90 {
        clock += 1.0 / 60.0;
        set_manual_clock(Some(clock));
        let t0 = std::time::Instant::now();
        app.frame();
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if ms > 30.0 || i % 15 == 0 {
            println!("frame {i}: {ms:.1} ms");
        }
    }
}

pub fn run(dir: &str) {
    if std::env::args().any(|a| a == "--leak") {
        leak();
        return;
    }
    if std::env::args().any(|a| a == "--bench") {
        bench();
        return;
    }
    std::fs::create_dir_all(dir).ok();
    let only = std::env::args().skip_while(|a| a != "--only").nth(1);
    let want = |n: &str| only.as_deref().map(|o| n.contains(o)).unwrap_or(true);
    if want("characters") {
        characters(dir);
    }
    views(dir, &want);
}

fn save(g: &Gfx, path: &str) {
    g.pm.save_png(path).expect("png");
    println!("wrote {path}");
}

fn characters(dir: &str) {
    let (cw, ch) = (190.0f32, 230.0f32);
    let states = [
        BotState::Idle, BotState::Working, BotState::Thinking, BotState::Searching,
        BotState::Approval, BotState::Question, BotState::Error, BotState::Finished,
        BotState::RateLimit, BotState::Sleeping, BotState::Dizzy,
    ];
    let cols = 6usize;
    let cells = states.len() + 3;
    let rows = cells.div_ceil(cols);
    let mut g = Gfx::new((cw * cols as f32) as u32, (ch * rows as f32) as u32, 1.0);
    g.pm.fill(rgb(22, 22, 26));

    let mut clock = 0.0f64;
    for i in 0..cells {
        set_manual_clock(Some(clock));
        let mut e = BotEngine::new();
        e.particle_overhang = 80.0;
        e.look_x = 0.2;
        if i < states.len() {
            e.set_state(states[i], true);
        } else if i == states.len() {
            e.trigger_emote(Emote::Love, 4.0);
        } else if i == states.len() + 1 {
            e.animate_morph(1.0);
        } else {
            e.greet();
        }
        let steps = if i == states.len() + 2 { 55 } else { 90 };
        for _ in 0..steps {
            clock += 1.0 / 60.0;
            set_manual_clock(Some(clock));
            e.update(1.0 / 60.0);
        }
        g.save();
        g.translate((i % cols) as f32 * cw, (i / cols) as f32 * ch);
        e.draw(&mut g, cw, ch);
        g.restore();
    }
    save(&g, &format!("{dir}/characters.png"));
}

/// Advances the app by `secs` of manual time at 60 fps.
fn advance(app: &mut App, clock: &mut f64, secs: f32) {
    let steps = (secs * 60.0).round() as i32;
    for _ in 0..steps {
        *clock += 1.0 / 60.0;
        set_manual_clock(Some(*clock));
        app.frame();
    }
}

fn new_app(clock: &mut f64) -> App {
    set_manual_clock(Some(*clock));
    let ctx = AppHandle::new(Settings::default());
    let mut app = App::headless(ctx, 2.0);
    // A stable demo configuration, whatever the machine has stored.
    app.st.hooks_installed = true;
    app.st.has_api_key = true;
    app
}

fn shot(app: &mut App, dir: &str, name: &str) {
    app.render_now();
    // Opaque backdrop so the transparent window reads as it would on a desktop.
    let mut out = Gfx::new(app.gfx.width(), app.gfx.height(), app.gfx.scale());
    out.pm.fill(rgb(54, 58, 66));
    out.pm.draw_pixmap(0, 0, app.gfx.pm.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
    save(&out, &format!("{dir}/{name}.png"));
}

fn hook(app: &mut App, v: serde_json::Value) {
    app.handle_hook(v);
}

fn views(dir: &str, want: &dyn Fn(&str) -> bool) {
    let mut clock = 10.0f64;

    macro_rules! scene {
        ($name:expr, $mode:expr, $view:expr, |$app:ident| $setup:block, $secs:expr) => {
            if want($name) {
                let mut $app = new_app(&mut clock);
                $app.debug_show($mode, $view);
                $setup
                advance(&mut $app, &mut clock, $secs);
                shot(&mut $app, dir, $name);
            }
        };
    }

    scene!("compact_idle", Mode::Compact, View::Overview, |app| {}, 1.5);
    scene!("overview_idle", Mode::Expanded, View::Overview, |app| {}, 1.5);
    scene!("overview_session", Mode::Expanded, View::Overview, |app| {
        hook(&mut app, json!({"hook_event_name":"SessionStart","cwd":"C:/Users/me/Project/coucou"}));
        hook(&mut app, json!({"hook_event_name":"UserPromptSubmit","cwd":"C:/Users/me/Project/coucou","prompt":"Port the island to native Rust"}));
        hook(&mut app, json!({"hook_event_name":"PreToolUse","cwd":"C:/Users/me/Project/coucou","tool_name":"Read","tool_input":{"file_path":"src/app.rs"}}));
        hook(&mut app, json!({"hook_event_name":"PreToolUse","cwd":"C:/Users/me/Project/coucou","tool_name":"Bash","tool_input":{"command":"cargo build --release"}}));
        app.debug_show(Mode::Expanded, View::Overview);
    }, 2.0);
    scene!("agent_pill", Mode::Expanded, View::Overview, |app| {
        hook(&mut app, json!({"hook_event_name":"UserPromptSubmit","coucou_agent":"gemini-cli","cwd":"C:/p/x","prompt":"Summarise the repo"}));
        hook(&mut app, json!({"hook_event_name":"PreToolUse","coucou_agent":"gemini-cli","cwd":"C:/p/x","tool_name":"Read","tool_input":{"file_path":"README.md"}}));
        app.st.set_focus("agent_gemini-cli");
    }, 2.0);
    scene!("approval", Mode::Expanded, View::Approval, |app| {
        hook(&mut app, json!({"hook_event_name":"PermissionRequest","request_id":"t-1","cwd":"C:/Users/me/Project/coucou","tool_name":"Bash","tool_input":{"command":"curl evil.example.com"}}));
        app.debug_show(Mode::Expanded, View::Approval);
    }, 1.5);
    scene!("question", Mode::Expanded, View::Question, |app| {
        hook(&mut app, json!({"hook_event_name":"Notification","cwd":"C:/p/coucou","message":"Should I keep the Tauri build too?"}));
    }, 1.5);
    scene!("error", Mode::Expanded, View::Error, |app| {
        hook(&mut app, json!({"hook_event_name":"PreToolUse","cwd":"C:/p/coucou","tool_name":"Bash","tool_input":{"command":"cargo test"}}));
        hook(&mut app, json!({"hook_event_name":"StopFailure","cwd":"C:/p/coucou"}));
        app.debug_show(Mode::Expanded, View::Error);
    }, 1.5);
    scene!("finished", Mode::Expanded, View::Finished, |app| {
        hook(&mut app, json!({"hook_event_name":"PreToolUse","cwd":"C:/p/coucou","tool_name":"Edit","tool_input":{"file_path":"src/app.rs"}}));
        hook(&mut app, json!({"hook_event_name":"Stop","cwd":"C:/p/coucou","message":"Native build is done"}));
        app.debug_show(Mode::Expanded, View::Finished);
    }, 1.5);
    scene!("confused", Mode::Expanded, View::Confused, |app| {
        app.st.state_override = Some(BotState::Dizzy);
    }, 1.5);
    scene!("empty", Mode::Expanded, View::Empty, |app| {}, 1.5);
    scene!("settings", Mode::Expanded, View::Settings, |app| {}, 1.5);
    scene!("note", Mode::Expanded, View::Note, |app| {
        app.st.note_message = Some("No API key yet — add one in Settings.".into());
    }, 1.5);
    scene!("prompt", Mode::Expanded, View::Prompt, |app| {
        app.st.chat_history = vec![
            ChatMessage { role: Role::User, content: "bonjour Mochi".into() },
            ChatMessage { role: Role::Assistant, content: "Bonjour ! Comment puis-je vous aider aujourd'hui ?".into() },
        ];
        app.debug_show(Mode::Expanded, View::Prompt);
    }, 1.5);
    scene!("drop_zone", Mode::Expanded, View::Upload, |app| {}, 1.5);

    // The drop sequence at a few moments.
    for (name, after_drop) in [("seq_0_hover", None), ("seq_1_suck", Some(0.35f32)), ("seq_2_chew", Some(0.85)), ("seq_3_bar", Some(2.4)), ("seq_4_done", Some(6.5))] {
        if !want(name) {
            continue;
        }
        let mut app = new_app(&mut clock);
        app.debug_show(Mode::Expanded, View::Upload);
        app.st.dropped_file = Some(DroppedInfo { name: "rapport-q3.pdf".into(), path: "C:/tmp/rapport-q3.pdf".into() });
        app.st.file_drag_over = true;
        app.upload.enter_zone(300.0, 100.0);
        advance(&mut app, &mut clock, 1.2);
        if let Some(t) = after_drop {
            app.st.file_drag_over = false;
            app.upload.perform_drop(2.4);
            app.set_view(View::Uploading);
            advance(&mut app, &mut clock, t);
        }
        shot(&mut app, dir, name);
    }

    // The launch greeting.
    for (name, t) in [("greet_0", 0.5f32), ("greet_1", 1.7), ("greet_2", 2.7), ("greet_3", 4.2)] {
        if !want(name) {
            continue;
        }
        let mut app = new_app(&mut clock);
        app.debug_show(Mode::Expanded, View::Greeting);
        app.greeting.start();
        advance(&mut app, &mut clock, t);
        shot(&mut app, dir, name);
    }
}
