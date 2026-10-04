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
