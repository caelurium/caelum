# Native packaging compilation

`package_macos_assets.sh` requests both shipping packages in one locked release
Cargo invocation through `package_macos_app.sh --with-cli`. Cargo resolves the
shared dependency features together; the desktop and CLI no longer rebuild
`caelum-core` separately with different feature sets. There is no unchecked
reuse or skip-build flag. The desktop-only helper remains available for `make app`.

Packaging still checks app metadata, architecture, ad-hoc signatures, allowed
dynamic libraries, extracted CLI version/help, mounted DMG contents and packaged
cold/warm URL handling, hidden-window reopening, draft restoration, quit and
updater installation/rollback. The release workflow uses this same path on
native Apple Silicon hosts and retains Cargo build timing reports.

## Measurement method

Use the same source revision, machine, toolchain, target and release profile.
Warm the external dependency artifacts for both variants first. Before each
timed run, clean only `caelum-protocol`, `caelum-core`, `caelum-desktop` and
`caelum-cli` with `cargo clean --release --target <target> -p <package> ...`.
This models the workspace compilation that remains after a dependency-cache
hit, rather than comparing an initially cold dependency build against a warm one.

Alternate the original separate-build asset script with the combined-build
script. Include the complete packaging/verification pipeline in elapsed time;
record Cargo durations separately from signing, DMG creation and smoke checks.
Retain logs and timing JSON in ignored `apps/desktop/profile-results/`. Stop other
builds and tests during measurement. Inspect compilation logs to confirm that
only workspace crates were rebuilt in the matched runs. macOS release packaging
supports only Apple Silicon; Linux uses its separate asset pipeline.

## Local result, 15 November 2026

The unchanged Rust source at `2d7d56c` was packaged for
`aarch64-apple-darwin` with Rust 1.95.0 on a 15-core Apple M5 Pro, 24 GiB RAM,
using the release profile (thin LTO, one codegen unit). After warming both
variants, the run order was separate, combined, combined, separate. Only
workspace crates compiled in these matched runs; `caelum-core` compiled twice
per separate run and once per combined run.

| Build strategy | Full packaging runs (seconds) | Median (seconds) |
| --- | --- | --- |
| Separate desktop and CLI | 113.24, 111.93 | 112.59 |
| Combined desktop and CLI | 94.35, 91.23 | 92.79 |

This reduced local warm-dependency packaging elapsed time by about **18%**.
Every run passed the complete asset and packaged lifecycle checks. The initial
warmup runs (137.16 s separate, 85.84 s combined) are excluded because the first
also rebuilt external dependencies. This is a small, same-host ARM comparison,
not a hosted CI percentile, cold-build result or Intel speedup claim.
