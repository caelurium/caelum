# Linux desktop

Caelum ships native **x86_64** (Intel/AMD) and **aarch64** (ARM64) AppImages.
Builds target **glibc 2.35 or newer** (Ubuntu 22.04 baseline), with X11 or Wayland
and a Vulkan-capable graphics driver. A desktop D-Bus session is required.
File and folder dialogs require `xdg-desktop-portal` and a backend for your desktop,
such as GTK, GNOME or KDE. Notifications use `org.freedesktop.Notifications`.

## Install and update

Download the matching AppImage and `SHA256SUMS` from the
[GitHub release](https://github.com/caelurium/caelum/releases). For example,
substituting the downloaded version:

```sh
VERSION=v1.2.3 # Replace with the downloaded release tag.
sha256sum --ignore-missing -c SHA256SUMS
chmod +x "Caelum-${VERSION}-x86_64.AppImage"
"./Caelum-${VERSION}-x86_64.AppImage" --install
```

Use `aarch64` on ARM64. `--install` copies the app to
`${XDG_DATA_HOME:-$HOME/.local/share}/caelum/Caelum.AppImage`, installs its icon and
application-menu entry, and registers `caelum://` project links for your user.
It requires `xdg-mime` (the `xdg-utils` package) and `update-desktop-database`
(`desktop-file-utils`). Launch the installed app, then use **Settings → About →
Check for updates** to download, verify, install and restart in-app. You can also
re-run `--install` with a newer download. See [updates and recovery](updates.md).

The AppImage can also launch directly. If FUSE is unavailable, use:

```sh
"./Caelum-${VERSION}-x86_64.AppImage" --appimage-extract-and-run
```

Add `--install` to that command to install without FUSE. To launch an installed
image without FUSE, set `APPIMAGE_EXTRACT_AND_RUN=1` in your desktop session.

Install and authenticate [Codex](codex.md) or [OpenCode](opencode.md) for live
sessions, and `gh` for GitHub operations. These tools must be visible on the
desktop session's `PATH`, which can differ from an interactive shell's `PATH`.

## Desktop behaviour

- Ctrl replaces Command in shortcuts. Ctrl+, opens Settings; Ctrl+O opens a
  project; Ctrl+W closes the window; Ctrl+Q quits.
- Closing the last window quits, stops owned providers and saves drafts.
- Repeated launches and project links activate the existing workspace. Separate
  `CAELUM_STATE` profiles have independent D-Bus identities and persistence locks.
- Notification clicks select the originating project/provider/session while the
  app is running. Notifications are requested only when there is work to deliver.

## Build and verify

The Linux workflow lists the Ubuntu development packages. Install Rust 1.95.0,
Python 3.11+ and those packages, or use `nix develop` for development:

```sh
make dev PROJECT=.
make check
```

AppImages are packaged on native Ubuntu 22.04 runners with locked Cargo
dependencies. `make app` on that baseline runs the same packaging script locally.
The package audit rejects newer glibc requirements and Nix store linkage.
AppImage libraries use relative ELF RUNPATHs, leaving provider subprocesses'
library environment intact. The host supplies glibc and its Vulkan/graphics stack.
The packager includes distribution copyright/source notices for bundled libraries.

`appimagetool` 1.9.1 is SHA-256 pinned for each architecture. Its embedded runtime
is reused with inherited update/signature metadata cleared, so packaging does not
fetch a mutable runtime release. Source: [AppImage/appimagetool](https://github.com/AppImage/appimagetool/tree/1.9.1).

CI runs desktop Clippy and tests on both architectures, then packages and launches
each AppImage under Xvfb/Openbox and headless Weston with Mesa software Vulkan.
Evidence artifacts include light/dark Home and Settings captures, native frame
traces and logs. The X11 smoke exercises cold/warm project links, clipboard paste,
the real portal folder picker, draft restoration, window close and installed
desktop-entry relaunch. Hardware-driver behaviour still depends on the host GPU.

The update smoke check also downloads a real AppImage from a local server, verifies
its checksum, atomically replaces an isolated installation, relaunches the native
app and checks startup acknowledgment, drafts, attachments and queued prompts.
