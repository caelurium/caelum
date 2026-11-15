# Native packaging compilation

`package_macos_assets.sh` requests both shipping packages in one locked release
Cargo invocation through `package_macos_app.sh --with-cli`. Cargo resolves the
shared dependency features together; the desktop and CLI no longer rebuild
`caelum-core` separately with different feature sets. There is no unchecked
reuse or skip-build flag. The desktop-only helper remains available for `make app`.

Packaging still checks app metadata, architecture, ad-hoc signatures, allowed
dynamic libraries, extracted CLI version/help, mounted DMG contents and packaged
cold/warm URL handling, hidden-window reopening, draft restoration and quit.
The release workflow uses this same path on native ARM and Intel hosts.

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
only workspace crates were rebuilt in the matched runs. Native Intel timings
must be measured on an Intel host rather than inferred from ARM.
