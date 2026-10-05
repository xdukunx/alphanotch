# About this fork

This repository is an **unofficial, personal fork** of [Coucou](https://github.com/Louis-CFM/coucou) by Louis Raille.
The `windows/lite` crate (Coucou Lite: a native, WebView-free Windows build), the dashboard pages, the Google
integration, the installer and the YASB layout are additions made on top of upstream v0.1.6.

## Licensing

- The **source code** is under the [MIT License](LICENSE), as upstream.
- The names **"Coucou"** and **"Mochi"**, the **Mochi character**, the app icon, the **sounds** and the
  images/videos under `docs/media/` and `design/` are **not** covered by MIT. They remain the property of
  Louis Raille: see [LICENSE-ASSETS.md](LICENSE-ASSETS.md).

## What that means here

- This repo publishes **source only**. It does **not** publish prebuilt binaries or releases.
- The installer (`install.ps1`) **builds the app on the user's own PC** from this repository, which is what
  `LICENSE-ASSETS.md` allows ("build and run Coucou from this repository, for yourself").
- If you want to **redistribute** a build (releases, an installer containing the exe, an app-store listing), you need
  Louis Raille's written permission, or you must replace the name, the character, the icon and the sounds with your own.
- If you are the upstream author and want something changed or removed here, please open an issue and it will be handled.
