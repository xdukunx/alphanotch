# Coucou Lite — handoff for the next Claude session

Native, webview-free Windows build of Coucou (`windows/lite`, crate `coucou-lite`). Read `README.md` here
for what it is and the measured numbers; this file is what is *not* obvious from the code.

## State (2026-10-04)
- Based on upstream **v0.1.6** (`louis-cfm/coucou`). Works end to end on Windows 11: island, Claude Code hooks over the
  named pipe (Allow/Deny round trip verified with a real click), greeting, drop sequence, chat, integrations,
  settings window, tray, sounds, `coucou_agent` agent pills.
- **Not tested with real credentials:** chat to Claude, integration pollers. **Not tested live:** a real OLE file drag
  (the sequence itself is verified through `--snapshot`).
- The Tauri app in `windows/src*` is untouched apart from the new Mochi look in `src/mochi/*.ts`
  (`character.ts` is the shared kit). Lite and Tauri serve the same pipe: run one or the other.

## Decisions the user made (don't undo)
- Mochi = **full face + gold crown only**. No scarf/neck, no graduation cap or tassel.
- Headset = ear cups at the sides drawn **behind** the helmet (+ thin band). Never in front of the face.
- Crown medallions are a status strip: honeycomb searching, bolt working, centre `!`/`?`/`×` attention,
  brain thinking, trend/✓ finished (`MEDAL_FOR_STATE` in `mochi/character.rs`).
- The idle bar keeps the notch height (**32 px**). Shrink the character to fit; never grow the notch or let it hang out.
- Lightness is the point: nothing may run while the island is hidden.

## Build / test
```powershell
cd windows
cargo build --release -p coucou-lite -p coucou-hook            # or: powershell -ExecutionPolicy Bypass -File lite\scripts\pack.ps1
cargo build --release -p coucou-lite --features snapshot --target-dir target-snap
target-snap\release\coucou-lite.exe --snapshot lite\snap [--only approval]   # deterministic PNGs of every view
target-snap\release\coucou-lite.exe --snapshot x --bench                     # ms per frame
```
Live testing next to a running Tauri app: start with `COUCOU_PIPE=coucou-lite-test` and talk to
`\\.\pipe\coucou-lite-test` (JSON line per event; a `PermissionRequest` waits for `allow`/`deny` on the same pipe).
`--settings` opens the settings window at start. Panics are written to `%LOCALAPPDATA%\Coucou\coucou.log`
(release is `panic = "abort"`).

## Pitfalls already paid for
- **Fonts must load lazily.** Parsing CJK fallbacks eagerly cost 2.4 GB. `text.rs` loads a face on first use and a
  fallback only when a character needs it, and uses `ttf-parser` (in place) + tiny-skia rasterisation.
- A spring can overshoot through zero while the island resizes → `f32::clamp` panics. Entry points that draw the
  character return early when the size is not positive.
- tiny-skia `Mask`s are full-window allocations: avoid clips in per-frame paths. Static glows use
  `Gfx::cached_layer` with a hand-written blit (the generic pixmap pipeline was ~10× slower).
- A window with `WS_EX_LAYERED` + per-pixel alpha is click-through where alpha = 0; the hidden wake strip and the
  "button held over the panel" drag catcher therefore paint alpha 1/255.
- Bash heredocs on this machine collapse `\\` and truncate some long scripts: write patch scripts with the file tool.
- Frame pacing: 16 ms during transitions, 33 ms for ambient motion (`Platform::set_frame_interval`).

## Docked mode and the dashboard (2026-10-05)
Added on top of upstream v0.1.6, for running Coucou inside a status bar (YASB) as a "dynamic island".
- `stayVisible` (settings.json + Settings → General → "Stay on screen"): the compact bar never folds to hidden
  (`Machine::stay_visible` in `fsm.rs`). Default off. Costs ~0.4 s CPU per 3 min with an activity showing.
- `activity.rs`: live activities in the compact bar, one at a time: timer > media (system media session, with
  artwork, seek and transport) > download (`.crdownload`/`.part` in Downloads). Timer is started with a pipe
  event `{"hook_event_name":"CoucouTimer","minutes":25}` (0 cancels) or from the dashboard. Clicking the slot
  toggles play/pause, or cancels the timer.
- `dashboard.rs` (`View::Dashboard`, 262 px): greeting, Now Playing, timer presets, to-do (`todos.rs`, plain JSON in
  `%APPDATA%\Coucou\todos.json`), agenda. It is the home view when nothing is running and `stayVisible` is on;
  the 4th header tab always opens it. It redraws twice a second while open (`dash_t` in `app.rs`).
- `gtasks.rs`: Google Tasks (two-way) and today's Google Calendar (read-only). The user brings their own
  Desktop OAuth client: click the status line on the to-do card, it imports `client_secret*.json` from Downloads
  into the Credential Manager (and deletes the file), then a loopback + PKCE sign-in. Keys `google-client` and
  `google-refresh` are in the `secrets.rs` allow-list: **a key not in `KNOWN_KEYS` is refused silently**.
  A token issued before the calendar scope was added gives 403: the status line then asks for a new sign-in.
  Stars on to-dos are local only (Google Tasks has no such thing).

### Pitfalls found while building this
- BitmapTransform scales **then** crops: `Bounds` are in scaled pixels, or the decoder answers "parameter is incorrect".
- Do not `clip` in per-frame paths: `fading_text` fades glyph alpha at the edges instead (a clip allocates a window-sized mask).
- Windows keyboard focus is only taken for text fields (`set_activating`): the to-do field does it on click and
  releases it on blur, fold and view change.
- Built exe is copied to `%LOCALAPPDATA%\Coucou\` for autostart, together with `coucou-hook.exe` (the app looks
  for the hook next to itself). Re-copy both after a rebuild.

### Agent pills and hook installers (2026-10-05)
- `agentPills` in settings.json (e.g. `["antigravity","opencode"]`) keeps those agents' pills on the island even
  when idle; Claude Code always has its own. A standing pill is set back to idle on Stop/SessionEnd, never removed
  (`standing` in `handlers.rs`). Clicking a small pill in the compact bar opens the island on that agent.
- `agenthooks.rs`: `coucou-lite.exe --agent-hooks <claude|antigravity> <status|preview|install|uninstall>
  [--fingerprint <fp>] [--out <file>]`. `preview` prints the diff + a fingerprint, `install` refuses without that
  fingerprint, takes a dated backup, never touches other tools' entries (Antigravity's hooks.json also holds
  other apps' blocks, e.g. `catjang`). The exe has no console: use `--out`.
- Antigravity lifecycle events `PreInvocation`/`PostInvocation` map to UserPromptSubmit/PostToolUse (as the Mac relay does).
- OpenCode: nothing installed yet (it was not on the machine). Plugin API: `~/.config/opencode/plugin/<name>.js`,
  events `session.idle`, `tool.execute.before/after`, `permission.asked`. Write it once OpenCode can be tried.
- Dashboard right card: to-do always shown; the timer is one header button opening 5/15/25 + a gear for a custom length.

### More dashboard pages (2026-10-05): Stocks, Weather, Teleprompter
Header tabs 5-7 (`views.rs`), drawn by `pages.rs`; data in `stocks.rs` / `weather.rs`; both are background pollers started in `main.rs`.
- Stocks: Yahoo Finance's public chart endpoint (`range=1d&interval=5m`), unofficial, no key, may throttle or change.
  Watchlist = `stocks` in settings.json (default `^JKSE` = IHSG, BBCA/BBRI/BMRI/TLKM `.JK`); the add field normalises
  `bbca` → `BBCA.JK`, `ihsg` → `^JKSE`. Polled every 60 s only while the page is open (`stocks::touch()` each frame), every 10 min otherwise.
- Weather: Open-Meteo forecast every 20 min. `weatherCity` is "auto" (default: city-level position from the IP via ipwho.is, then geojs.io; re-detected each cycle so a moved laptop is noticed; the page shows "≈ dari IP") or a city name (Open-Meteo geocoder). Icons are drawn in code. Note: the app re-saves every setting it knows, so an old default can end up written in settings.json: check the file when a default seems ignored.
- Teleprompter: the script is `%APPDATA%\Coucou\teleprompter.txt` (created with a placeholder; **Edit** opens it in Notepad and the page reloads on save).
  Scrolls with the frame loop (`tp_step`), speed in px/s. Text fades at the edges instead of being clipped.
