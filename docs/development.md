# Development

## Prerequisites

- macOS 13 or later and Xcode Command Line Tools, or the [Linux desktop prerequisites](linux.md)
- Rust **1.95.0**, including `rustfmt` and `clippy` (`rust-toolchain.toml`)
- Python 3.11+ for previews, packaging and release-script checks
- A configured [Codex](codex.md) or [OpenCode](opencode.md) installation for live sessions
- Authenticated GitHub CLI (`gh`) for pull requests

`nix develop` supplies the Rust, Python and Make tools. macOS platform tools must
be installed on the host. Rune and rune-kit are revision-pinned; all four crates
share the root `Cargo.lock`. The Linux shell supplies desktop build libraries;
the host supplies the desktop session, portals and Vulkan driver.

## Commands

```sh
make dev PROJECT=.                # Run the native desktop
make app                          # Build the native macOS bundle or Linux AppImage
make cli                          # Build the standalone CLI
make cli-run PROJECT=.            # Open a project in the installed app
make previews                     # Capture offline native reference scenes
make check                        # Formatting, lint, Rust and Python tests
```

Use `cargo run --locked -p caelum-desktop -- --design-fixture` for an offline UI,
or `--evaluation-workload` for the long-history/concurrent-stream fixture. Use
`CAELUM_STATE=/absolute/path/to/isolated/workspace.json` for live development with
separate saved data. On macOS, close the window to keep sessions running; quit
with ⌘Q. On Linux, closing the window quits; Ctrl+Q also quits.

On macOS, `make dev` runs a signed debug `Caelum Dev.app` with a checkout-specific
bundle identity, so native notifications belong to the running development app.
The release app keeps its separate notification permissions and URL registration.
Use `python3 scripts/dev/macos_app.py --build-only` to prepare the debug bundle,
or pass application arguments after `--` to build and run it directly.

See [contributing](contributing.md), [architecture](architecture.md),
[performance](performance/README.md) and [releasing](releasing.md).
