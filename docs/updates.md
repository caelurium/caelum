# Application updates

Open **Settings → About → Check for updates**. When a newer stable release is
available, choose **Download update**, **Install update**, then **Restart Caelum**.
Downloads show their size/progress and can be cancelled, including while verifying
or preparing the application. Failed downloads and installations offer a retry.
Release checks are manual; Caelum does not poll for updates in the background.

Restart waits for the latest drafts, captured attachments and queues to be saved.
Owned agent processes stop during shutdown. Sessions restore using the normal
workspace recovery rules, with queued work paused for review before resuming.
If saving fails, resolve the storage error and retry the restart.

## Supported installations

- **macOS, Apple Silicon and Intel:** a user-owned `Caelum.app` in a writable
  directory, usually `/Applications` or `~/Applications`. Move the app out of its
  mounted DMG and launch it from the installed location first.
- **Linux, ARM64 and x86_64:** the official AppImage installed with `--install` at
  `${XDG_DATA_HOME:-$HOME/.local/share}/caelum/Caelum.AppImage`. Launch that installed
  copy before updating. Restart supports extraction mode when FUSE is unavailable.
- **Package-manager, portable, another user's or read-only installations:** the
  updater explains the required action and offers the release in your browser.
  Use the owning package manager for Homebrew/Nix/distribution packages, or install
  a user-owned official package. The updater never requests elevated privileges.

## Verification and recovery

Downloads must match the official GitHub release, platform and architecture, and
pass SHA-256 verification against that release's `SHA256SUMS`. macOS bundles also
pass bundle identity/version, architecture and code-signature checks; Linux checks
the AppImage type, architecture and reported application version. Release checksums
are fetched over HTTPS from the same official release, not a separate signing service.

Staging happens beside the installed application, so replacement is a single
same-filesystem atomic exchange. Network failures, incomplete downloads, failed
checksums and preparation errors leave the current app intact. Allow enough disk
space for the download, staged app and previous version.

The previous installation stays in `.Caelum.app.update/payload` (macOS) or
`.Caelum.AppImage.update/payload` (Linux) beside the app until the new version draws
its first frame and restores the workspace. An early restart failure restores and
launches the previous version. If startup is slow, the backup remains until startup
completes; a later download cannot overwrite an unacknowledged recovery copy.

If a crash or power loss prevents automatic recovery, quit Caelum first. Move the
current installation aside, then move the adjacent `payload` back to `Caelum.app`
or `Caelum.AppImage`. On macOS, the `payload` directory is the entire previous app
bundle. Keep the recovery directory until the restored app launches successfully.
Your workspace data lives separately and is not replaced by the updater.

## Packaged lifecycle check

On a native host with a desktop session, after building the release package:

```sh
# macOS (use the matching target and version)
python3 scripts/release/smoke_update.py \
  --app target/aarch64-apple-darwin/release/bundle/macos/Caelum.app \
  --package target/release/Caelum-v1.2.3-aarch64.dmg

# Linux, inside an isolated X11/D-Bus desktop session
xvfb-run -a dbus-run-session -- python3 scripts/release/smoke_update.py \
  --package target/release/Caelum-v1.2.3-x86_64.AppImage
```

The check uses temporary installations, a local HTTP server and offline providers.
It verifies real package replacement/relaunch, startup cleanup, graceful quit and
restoration of drafts, attachments and queues. macOS packaging and both Linux CI
architectures run it. Using the same version on both sides keeps this check
independent of network release availability; semantic-version selection has separate
core regression coverage.
