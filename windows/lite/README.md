> In this repository (**AlphaNotch**) this crate is the whole app. Text below is the original Coucou Lite description.

# Coucou Lite

The same Mochi, the same island, **without a webview**. One small Win32 window drawn
with [tiny-skia](https://github.com/linebender/tiny-skia), a tray icon, a named-pipe
relay for Claude Code's hooks and a few pollers. No WebView2, no Node, no Chromium.

| Measured on Windows 11 (125 % scaling) | Coucou (Tauri) | **Coucou Lite** |
|---|---|---|
| Processes | 9 (`coucou` + 8 × `msedgewebview2`) | **1** |
| Memory, island idle | 188 MB working set / 300 MB private | **22 MB / 6 MB** |
| Memory, a permission card on screen | — | **~26–43 MB** |
| CPU, island idle or hidden | ~0 % | **0.0 %** |
| CPU, Claude working (compact bar) | — | ~2 % of one core |
| Executable | 6.8 MB + the WebView2 runtime | **6.0 MB** (3.1 MB of it is the 28 sounds) |

Nothing runs while the island is hidden: no frame loop, no cursor poll, no timers. The
only thing alive is a 240 × 6 px invisible strip that wakes it on hover.

## What it does

Everything the Tauri build does, ported rather than rewritten:

- **Mochi**, drawn in code: plum helmet, cream face plate, ink eyes, headset, gold crown.
  The five crown medallions are a live status strip — ⚙ searching, ⚡ working, centre
  `!` approval / `?` question / `×` error, 🧠 thinking, ✓ finished. Same engine, same tweens,
  same particles as `windows/src/mochi/engine.ts`.
- **Claude Code**, live: every session step in the ticker, finished/error cards, and
  permission requests with **Allow / Deny** — answered over the same pipe, so
  `coucou-hook.exe` is unchanged.
- **Chat** with Claude from the island, with the dropped file as context.
- **Drop a file** on the island: Mochi turns into a mailbox and swallows it (the full
  sequence, ported from `UploadSequenceEngine`).
- **Integrations**: Stripe, GitHub, Vercel, n8n, Resend, Notion, Cal.com — same pollers,
  same cards. Keys live in the Windows Credential Manager.
- **Settings window**: hooks (with the exact diff and a dated backup before anything is
  written), API key, model, integrations, sound, auto-close, display, launch at startup.
- Tray icon (Open · Settings · Pause · Quit), single instance, per-monitor DPI.

## How it stays light

- **Layered window with per-pixel alpha** (`UpdateLayeredWindow`): transparent pixels let
  the mouse through by themselves, so there is no click-through bookkeeping at all.
- **Immediate-mode UI** (`ui.rs`): each frame the views draw themselves and ask the widgets
  whether they were clicked. No widget tree to keep in sync with the state.
- **Fonts**: Segoe UI and friends are read from `C:\Windows\Fonts` on first use and parsed
  in place with `ttf-parser`; glyphs are rasterised by tiny-skia and cached. CJK fallbacks
  load only when a character needs them.
- **Sounds** are embedded and paged in by the OS only when one plays; Windows mixes them.
- Frames render at 60 fps during transitions and 30 fps for ambient motion, and the
  static glows are cached bitmaps.

## Build

Requirements: Rust (stable, MSVC) and the Visual Studio Build Tools.

```powershell
cd windows
cargo build --release -p coucou-lite -p coucou-hook
# → target\release\coucou-lite.exe  and  target\release\coucou-hook.exe
```

`scripts\pack.ps1` builds both and zips them as `release\Coucou-Lite-<version>.zip`.
Keep `coucou-hook.exe` next to `coucou-lite.exe`: on launch Lite copies it to
`%LOCALAPPDATA%\Coucou\bin`, which is where the hooks point. Then open
**Settings → Claude Code → Install hooks…**.

Lite and the Tauri build serve the same pipe, so run one or the other.

## Design review without a window

```powershell
cargo build --release -p coucou-lite --features snapshot
target\release\coucou-lite.exe --snapshot lite\snap          # every view, state and the drop sequence
target\release\coucou-lite.exe --snapshot lite\snap --only approval
target\release\coucou-lite.exe --snapshot x --bench          # ms per frame
```

Time is driven by a manual clock, so every PNG is deterministic.

Other switches: `--settings` opens the settings window at start. `COUCOU_PIPE=name`
serves a different pipe, for testing beside a running instance.

## Layout

| File | |
|---|---|
| `gfx.rs`, `text.rs`, `icons.rs` | Canvas-2D-style drawing over tiny-skia, fonts, SVG-path icons |
| `mochi/` | `character.rs` (the look), `engine.rs` (motion), `greeting.rs`, `upload.rs` |
| `app.rs`, `fsm.rs`, `layout.rs`, `state.rs` | the island: modes, geometry, input, frame loop |
| `views.rs`, `cards.rs`, `ticker.rs`, `ui.rs`, `textfield.rs` | what is drawn and the little toolkit |
| `handlers.rs` | Claude Code hook events and integration updates → state |
| `platform.rs`, `ole.rs`, `tray.rs`, `settings_win.rs`, `sound.rs` | Win32 |
| `pipe.rs`, `hooks.rs`, `claude.rs`, `integrations.rs`, `files.rs`, `secrets.rs`, `settings.rs` | the backend, shared in spirit with `src-tauri` |
