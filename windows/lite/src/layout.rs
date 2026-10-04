// Island geometry — port of core/layout.ts (itself IslandTypes.swift +
// IslandWindowController.islandSize + IslandRootView.botPosition). Logical px.

use crate::mochi::engine::BotState;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Hidden,
    Compact,
    Expanded,
}

impl Mode {
    pub fn order(self) -> i32 {
        match self {
            Mode::Hidden => 0,
            Mode::Compact => 1,
            Mode::Expanded => 2,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum View {
    Overview,
    Empty,
    Approval,
    Question,
    Error,
    Finished,
    Confused,
    Upload,
    Uploading,
    Choose,
    Prompt,
    Note,
    Settings,
    Greeting,
    Dashboard,
    Stocks,
    Weather,
    Teleprompter,
}

/// The window is a fixed 720×320 (largest view); the island is drawn inside it,
/// glued to the top edge and horizontally centred.
pub const PANEL_W: f32 = 720.0;
pub const PANEL_H: f32 = 320.0;

pub const NOTCH_W: f32 = 184.0;
pub const NOTCH_H: f32 = 32.0;
/// The idle bar keeps the notch height; the mascot is sized to fit inside it.
pub const COMPACT_H: f32 = 32.0;
pub const COMPACT_W: f32 = 288.0;
pub const EXPANDED_W: f32 = 640.0;
/// The dashboard pages (home, stocks, weather, teleprompter) are slimmer than the other views.
pub const DASH_W: f32 = 548.0;

pub fn view_width(v: View) -> f32 {
    match v {
        View::Dashboard | View::Stocks | View::Weather | View::Teleprompter => DASH_W,
        _ => EXPANDED_W,
    }
}

pub const ROUNDED_CORNER: f32 = 14.0;
pub const EXPANDED_CORNER: f32 = 22.0;

/// Invisible hover strip that wakes the island when it is hidden.
pub const WAKE_STRIP_W: f32 = 240.0;
pub const WAKE_STRIP_H: f32 = 6.0;

/// Where the idle mascot sits (logical px from the island's top-left), fully
/// inside the bar: crown at the top edge, chin near the bottom.
pub const COMPACT_BOT: (f32, f32, f32) = (26.0, 17.5, 24.0); // cx, cy, diameter

/// Margin around the island that still counts as "on the island".
pub const HIT_MARGIN: f32 = 14.0;

pub struct ViewLayout {
    pub height: f32,
    pub bot_x: f32,
    pub bot_y: Option<f32>,
    pub bot_d: f32,
}

pub fn layout(v: View) -> ViewLayout {
    let l = |height, bot_x, bot_y, bot_d| ViewLayout { height, bot_x, bot_y, bot_d };
    match v {
        View::Overview => l(160.0, 68.0, None, 58.0),
        View::Empty => l(160.0, 70.0, None, 62.0),
        View::Approval => l(160.0, 62.0, None, 56.0),
        View::Question => l(160.0, 62.0, None, 56.0),
        View::Error => l(160.0, 62.0, None, 58.0),
        View::Finished => l(160.0, 62.0, None, 58.0),
        View::Confused => l(160.0, 76.0, None, 66.0),
        View::Upload => l(176.0, 140.0, Some(104.0), 62.0),
        // botY 103 = bar top (42 + 58) + 3, so the dot really rides the bar.
        View::Uploading => l(176.0, 46.0, Some(103.0), 20.0),
        View::Choose => l(176.0, 60.0, Some(101.0), 52.0),
        View::Prompt => l(160.0, 52.0, None, 44.0),
        View::Note => l(160.0, 60.0, None, 50.0),
        View::Settings => l(160.0, 54.0, None, 46.0),
        View::Greeting => l(150.0, 320.0, Some(90.0), 0.0),
        // Home is short; Mochi sits in the header row beside the greeting.
        View::Dashboard => l(196.0, 286.0, Some(21.0), 26.0),
        View::Stocks | View::Weather | View::Teleprompter => l(220.0, 286.0, Some(21.0), 26.0),
    }
}

pub fn chat_prompt_height(count: usize) -> f32 {
    (240.0 + count as f32 * 40.0).min(300.0)
}

pub fn island_size(mode: Mode, view: View, chat_count: usize) -> (f32, f32) {
    match mode {
        // No notch on a PC: the island retracts to zero height and slides into
        // the top edge of the screen.
        Mode::Hidden => (NOTCH_W, 0.0),
        Mode::Compact => (COMPACT_W, COMPACT_H),
        Mode::Expanded => {
            let h = if view == View::Prompt { chat_prompt_height(chat_count) } else { layout(view).height };
            (view_width(view), h)
        }
    }
}

pub struct BotPlacement {
    pub cx: f32,
    pub cy: f32,
    pub d: f32,
    pub opacity: f32,
}

/// IslandRootView.botPosition — cy is measured from the island's top edge.
pub fn bot_position(mode: Mode, view: View, island_h: f32, upload_progress: f32) -> BotPlacement {
    match mode {
        Mode::Hidden => BotPlacement { cx: 46.0, cy: 16.0, d: 6.0, opacity: 0.0 },
        Mode::Compact => BotPlacement { cx: COMPACT_BOT.0, cy: COMPACT_BOT.1, d: COMPACT_BOT.2, opacity: 1.0 },
        Mode::Expanded => {
            let l = layout(view);
            if view == View::Uploading {
                return BotPlacement {
                    cx: 36.0 + upload_progress * 526.0,
                    cy: l.bot_y.unwrap_or(103.0),
                    d: l.bot_d,
                    opacity: 1.0,
                };
            }
            if let Some(y) = l.bot_y {
                return BotPlacement { cx: l.bot_x, cy: y, d: l.bot_d, opacity: 1.0 };
            }
            // Centre of the fixed 84 pt card (8 pt top inset + 34 pt header → content at y = 42)
            let header_bottom = 42.0;
            let card_h = 84.0;
            let cy = header_bottom + (island_h - header_bottom - card_h) / 2.0 + card_h / 2.0;
            BotPlacement { cx: l.bot_x, cy, d: l.bot_d, opacity: 1.0 }
        }
    }
}

pub fn bot_glow_color(s: BotState) -> &'static str {
    match s {
        BotState::Working => "#3B9EFF",
        BotState::Thinking => "#A78BFA",
        BotState::Searching => "#6366F1",
        BotState::Approval => "#F5A524",
        BotState::Error => "#F4505E",
        BotState::Finished => "#34D399",
        BotState::RateLimit => "#F59E0B",
        _ => "#FFFFFF",
    }
}

pub fn bot_glow_opacity(s: BotState) -> f32 {
    match s {
        BotState::Idle | BotState::Sleeping => 0.15,
        BotState::Dizzy => 0.0,
        _ => 0.65,
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Wash {
    None, Red, Green, Pink, Amber, Cyan, Indigo,
}

/// Card wash colours (CardBackground.washColor) as r,g,b,a.
pub fn wash_rgba(w: Wash) -> (u8, u8, u8, f32) {
    match w {
        Wash::Red => (244, 80, 94, 0.55),
        Wash::Green => (52, 211, 153, 0.5),
        Wash::Pink => (244, 114, 182, 0.55),
        Wash::Amber => (245, 165, 36, 0.42),
        Wash::Cyan => (34, 211, 238, 0.38),
        Wash::Indigo => (99, 102, 241, 0.5),
        Wash::None => (0, 0, 0, 0.0),
    }
}
