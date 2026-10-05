# AlphaNotch

A personal **Windows** customisation of [Coucou](https://github.com/Louis-CFM/coucou): a small notch-style island at the top
centre of the screen, built to sit inside a [YASB](https://github.com/amnweb/yasb) "adaptive island" status bar.
It is a hobby setup shared so a friend can get the same thing in one command, not a product.

> **Credit and licence.** AlphaNotch is built on Coucou by **Louis Raille** (MIT code). The name "Coucou", the Mochi character,
> the icon and the sounds are his and are not covered by MIT: read [NOTICE-FORK.md](NOTICE-FORK.md) and [LICENSE-ASSETS.md](LICENSE-ASSETS.md).
> This repo publishes source only, and the installer builds it on your own PC. It is not affiliated with or endorsed by the Coucou author.
> The original README is kept in [README.upstream.md](README.upstream.md).

## What it does

- **Now playing**: a round player with a progress ring (seek, shuffle, repeat, prev/next) from the Windows media session (Spotify, browsers...).
- **To-do list synced with Google Tasks** and **today's Google Calendar event** (optional: you bring your own Google OAuth client).
- **Timer** (5 / 15 / 25 / custom) that also shows in the closed bar; **downloads** shown while a browser downloads.
- **Stocks**: IHSG and your own watchlist, intraday charts.
- **Weather**: located from your IP address (or a city you set), 7-day outlook.
- **Teleprompter**: your script under the camera.
- **AI coding agents**: pills for **Claude Code** (CLI and Claude Desktop), **Antigravity** (OpenCode planned), with approvals from the island.
- Light: one native process (~30-50 MB, no WebView), idle-friendly (mouse moves only redraw when something under the cursor changes).

## Install (Windows 10/11)

```powershell
irm https://raw.githubusercontent.com/xdukunx/alphanotch/main/install.ps1 | iex
```

It **analyses your PC first** (YASB, fonts, Rust, Build Tools, Claude Code, Antigravity...), shows a plan, asks your name and one confirmation,
then installs only what is missing, builds AlphaNotch on your PC, starts it at every login and (optionally) applies the YASB layout and agent hooks.
Details, options, dry run and uninstall: [INSTALL-WINDOWS.md](INSTALL-WINDOWS.md).

## Configure

Settings live in `%APPDATA%\Coucou\settings.json` (the folder keeps the original name so existing hooks keep working).

| Key | Meaning |
|---|---|
| `displayName` | the name the greeting uses |
| `stayVisible` | keep the compact bar on screen when idle (default for the installer) |
| `autoCloseInterval` | seconds before the open island folds after the mouse leaves |
| `agentPills` | agent pills kept on the island, e.g. `["antigravity","opencode"]` |
| `weatherCity` | `"auto"` (from your IP) or a city name |
| `stocks` | watchlist, e.g. `["^JKSE","BBCA.JK"]` (`^JKSE` is the IHSG) |

Optional Google Tasks / Calendar: [docs/GOOGLE-SETUP.md](docs/GOOGLE-SETUP.md).

## Network and privacy

No telemetry, no account. The app only talks to services you use: Google (only if you connect it), Open-Meteo (weather), ipwho.is / geojs.io
(to estimate the city when `weatherCity` is `"auto"`), and Yahoo Finance (stock prices). Secrets are stored in the Windows Credential Manager.
The privacy policy used for the Google sign-in is at <https://xdukunx.github.io/alphanotch/privacy.html>.

## Layout of this repo

| Path | What |
|---|---|
| `windows/lite` | the app (Rust, native Win32 + tiny-skia); see `windows/lite/CLAUDE.md` for how it works |
| `windows/hook` | `coucou-hook`, the tiny relay agents call |
| `yasb/` | the YASB "adaptive island" config and stylesheet |
| `install.ps1`, `uninstall.ps1` | the installer |
| `NotchBuddy/`, `windows/src*`, `docs/` | upstream Coucou (macOS app, Tauri build, site), not used by this setup |
