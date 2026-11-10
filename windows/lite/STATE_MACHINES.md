# State machines in Coucou Lite

Every piece of state in `lite/src` that moves between named situations. Check it before
adding a boolean next to an existing one: if the new flag only means something in one state
of a machine below, it belongs inside that machine.

Machines marked **explicit** are an `enum` with their own transition code. Machines marked
**implicit** are still spread over plain fields; the note says what to watch for.

## The island

| Machine | Where | States | Moves on |
|---|---|---|---|
| Open / close (explicit) | `fsm.rs` `Fsm` | `Hidden`, `Petit`, `Home`, `Coucou` | hover, alerts, the home→petit and petit→hidden deadlines, `pinned` while an answer is awaited |
| Size and content | `layout.rs` `Mode`, `View` | `Hidden` / `Compact` / `Expanded`, then one `View` per card | `App::set_view`, `App::alert`, `App::surface`, `App::collapse` |
| Full-screen apps (explicit) | `fullscreen.rs` `GameScreen` | `Normal`, `Hidden`, `Moved` | `next_game_screen(cur, busy, other_screen)`; side effects of entering and leaving each state are in `App::set_game_screen` |
| Frame loop (implicit) | `app.rs` `running`, `platform` timers | stopped, or running every 16 / 33 / 50 / 66 ms | `ensure_running`; stops when the `busy` expression in `App::frame` is false. Anything that animates must appear in that expression or set `ui.animating` |
| Strip at the screen edge (implicit) | `app.rs` `collapsed`, `Timer::CollapseToStrip` | full window, 6 px wake strip | only meaningful while `Mode::Hidden` |

## Mochi

| Machine | Where | States | Moves on |
|---|---|---|---|
| Activity (explicit) | `mochi/engine.rs` `BotState` | `Idle`, `Working`, `Thinking`, `Searching`, `Approval`, `Question`, `Error`, `Finished`, `RateLimit`, `Sleeping`, `Dizzy` | hook events (`handlers.rs`), `Timer::TaskIdle` |
| Mouse on Mochi (explicit) | `buddy.rs` `Pet` | `Idle`, `Pressed { since }`, `Petting { last }` | mouse down sets `Pressed`; `pet_step` turns it into `Petting` after 0.3 s; mouse up from `Pressed` is a slap |
| Running from the cursor (implicit) | `app.rs` `flee_off`, `flee_until`, `flee_cool` | at home, fled, cooling down | `buddy.rs` after a chase score of 18 |

## Claude Code sessions

| Machine | Where | States | Moves on |
|---|---|---|---|
| Waiting for a person (implicit) | `state.rs` `pending_approval`, `pending_question`; `Timer::ApprovalTimeout`, `Timer::QuestionTimeout` | none, approval, question, or both | permission and AskUserQuestion hooks; the answer or the timeout clears each. A second approval is declined, and so is a question while either is open; an approval can still arrive while a question waits |
| Pill badge (explicit) | `state.rs` `PillBadge` | `Approval`, `Finished`, `Error` | set on the event, cleared by `Timer::TaskIdle` |
| Session card steps (implicit) | `state.rs` `Task::steps`, `diffs`, `topic` | — | `steps` and `diffs` stay the same length (`push_step` realigns); `topic` comes from the latest prompt |

## Pages and cards

| Machine | Where | States | Moves on |
|---|---|---|---|
| Dashboard timer row (explicit) | `dashboard.rs` `TimerMenu` | `Closed`, `Presets`, `Custom` | timer button toggles `Closed`/`Presets`, the gear opens `Custom`, `‹` goes back, starting a timer closes it |
| Care and coach cards (implicit) | `app.rs` `reminder`, `rem_next`, `rem_last_any`, `prayer_again` | no card, one card | `reminder_tick` every 30 s: at most one card, care cards at least 25 min apart, held while nobody has touched the keyboard for 5 min |
| Due-task card | `reminders.rs` `ReminderCard::task` | — | the card carries the task it is about; there is no separate field to keep in step |
| Lyrics (explicit) | `lyrics.rs` `State` | `Loading`, `Synced`, `Plain`, `NotFound`, `Failed` | the LRCLIB fetch |
| Google link (explicit) | `gtasks.rs` `Status` | `NoClient`, `NeedsLogin`, `LoggingIn`, `Linked`, `Error` | sign-in flow |
| Stock poller (implicit) | `stocks.rs` loop, `ACTIVE`, `next_wait` | idle, every 60 s, every 10 min, every 3 min after a 429 | `set_active` from the reminder tick and `set_game_screen` |
| Stock code status (explicit) | `stocks.rs` `Status` | `Trading`, `Halt`, `Suspended` | `status(q, all, now)`, read from prices during the session |
| Stock range data | `stocks.rs` `Detail` | `loading`, `error` and `candles` may overlap on purpose: a refresh keeps showing the old candles | `detail(sym, range)` |
| Overlay (implicit) | `app.rs` `overlay`, `Timer::OverlayEnd` | none, one overlay | volume, Caps, charging, headphones, Wi-Fi; never while `quiet()` |

## Adding to this

- A new flag that only matters in one state of a machine above goes into that machine's enum.
- Keep the transition rule a pure function when it can be one (`next_game_screen`, `pet_step`,
  `next_wait`, `stocks::status`) and test it in the same file. Side effects stay in the `App`
  method that applies it.
- Update this table in the same commit.
