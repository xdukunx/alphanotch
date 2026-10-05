# AlphaNotch for Windows: one-command install

A notch-style island (Coucou Lite, native, no WebView) plus a YASB "adaptive island" status bar.
The island shows now playing, a timer, to-dos, today's agenda, stocks, weather, a teleprompter,
and what your AI coding agents (Claude Code, Antigravity, OpenCode) are doing.

## Install

Open **PowerShell** (a normal one, not "as administrator") and run:

```powershell
irm https://raw.githubusercontent.com/xdukunx/alphanotch/main/install.ps1 | iex
```

Want to look first? Dry run: it only analyses your PC and prints the plan, and changes nothing.

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/xdukunx/alphanotch/main/install.ps1))) -DryRun
```

## What it does

1. **Analyses your PC**: Windows version, winget, free space, YASB (installed? version? your own config?),
   the JetBrainsMono Nerd Font, Rust, Visual Studio Build Tools, Claude Code, Antigravity, OpenCode.
2. **Shows the plan** and asks once. It also asks **what name to greet you with** and, if you already have a
   personal YASB config, whether to replace it with the AlphaNotch layout (yours is backed up first).
3. Installs only what is missing:
   - YASB (or upgrades it: the adaptive bar needs 2.0.7+) and the Nerd Font, with winget
   - Rust and Visual Studio Build Tools (C++), because **Coucou Lite is built on your PC from source**
     (several GB and 5-10 minutes the first time; later runs are much faster)
4. Installs Coucou Lite to `%LOCALAPPDATA%\Coucou`, writes starter settings, and **starts it at every login**.
5. Applies the YASB layout and enables YASB autostart (unless you said keep).
6. Offers to add Coucou's hooks to **Claude Code** and **Antigravity** (it shows how many lines it adds, takes a
   dated backup, never touches another tool's hooks).

No prebuilt binary is downloaded. Everything it does is in [`install.ps1`](install.ps1); the log is in
`%TEMP%\alphanotch-install.log`.

## Options

Run through a script block to pass options, e.g.:

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/xdukunx/alphanotch/main/install.ps1))) -Yes -Name "Budi" -Yasb keep
```

| Option | Meaning |
|---|---|
| `-DryRun` | analyse and print the plan, change nothing |
| `-Yes` | do not ask, accept the plan |
| `-Name "Budi"` | the name the island greets you with |
| `-Yasb auto\|apply\|keep\|skip` | `apply` installs/updates YASB and the layout; `keep` leaves YASB's config alone; `skip` ignores YASB |
| `-NoHooks` | do not touch Claude Code / Antigravity |
| `-NoAutostart` | do not start Coucou at login |
| `-NoStart` | do not start Coucou at the end |
| `-NoSettings` | do not write `%APPDATA%\Coucou\settings.json` |
| `-Branch`, `-Repo`, `-SourceDir`, `-InstallDir` | where to get the source and where to install it |

## Using it

Move the pointer to the top centre of the screen and click the small bar: the island opens.
The tabs are along the top (home, chat, drop a file, dashboard, stocks, weather, teleprompter);
the small arrow at the bottom folds it back.

- **Stocks**: IHSG and a watchlist. Type a code (`bbca`) in the field, Enter. Click a row, hover and press x to remove.
- **Weather**: located from your IP address by default (`weatherCity` in settings.json: `"auto"` or a city name).
- **Teleprompter**: your script is `%APPDATA%\Coucou\teleprompter.txt` (the Edit button opens it).
- **Name**: `displayName` in `%APPDATA%\Coucou\settings.json`.
- **Google Tasks / Calendar** (optional): see [docs/GOOGLE-SETUP.md](docs/GOOGLE-SETUP.md). It needs your own Google OAuth client.

## Uninstall

```powershell
irm https://raw.githubusercontent.com/xdukunx/alphanotch/main/uninstall.ps1 | iex
```

It asks before each part: hooks (removed with a backup), the app and its autostart, your data, and the YASB layout
(your old config is restored from its backup). YASB itself stays installed (`winget uninstall AmN.yasb` removes it).

## Requirements

Windows 10 1809 / Windows 11, 64-bit, [winget](https://learn.microsoft.com/windows/package-manager/winget/)
(included with current Windows), an internet connection, and roughly 3 GB free (about 8 GB if the C++ Build Tools are needed).

## Notes and limits

- Tested on Windows 11. The clean-machine path (installing YASB, Rust and the Build Tools from nothing) relies on winget
  and has not been exercised on a fresh PC yet: run the dry run first, and send the log if something fails.
- This is an unofficial personal fork of [Coucou](https://github.com/Louis-CFM/coucou). See [NOTICE-FORK.md](NOTICE-FORK.md).
