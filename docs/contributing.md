# Contributing

Keep changes focused and follow the existing Rust modules. Application state,
provider behavior and filesystem/Git/GitHub operations belong in `caelum-core`;
native editors, focus and rendering belong in `apps/desktop`.

## Rust structure

The repository's [agent guidelines](../AGENTS.md) define module ownership,
state-machine and UI performance conventions. Aim for 800 non-test lines per
Rust file; 1,500 is the hard cap. Test files and inline test modules have a
1,000-line cap. Split by capability, with tests alongside the feature they cover.

`make structure` checks both `apps/` and `crates/`, including new non-ignored
Rust files. It excludes inline `#[cfg(test)]` items from production counts and
measures tests separately. Folder checks encourage capability modules: related
prefixed siblings belong together, and folders warn above 40 source files and
fail above 60. Test folders and standalone program directories are excluded.

Existing oversized files have explicit ceilings in
`scripts/structure/exceptions.txt`. New exceptions and increases fail against
the PR's base revision. Extract the area being changed before adding behaviour
to an over-cap file, in a separate structural PR. After reducing a violation,
run `python3 scripts/structure/check.py --update-exceptions` to lower its ceiling
or remove it. The checker verifies these reductions too.

## Checks

On macOS, from the repository root:

```sh
make structure
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace -- --test-threads=4
python3 -m unittest discover -s scripts/release -p 'test_*.py'
python3 -m unittest discover -s scripts/profile -p 'test_*.py'
python3 -m unittest discover -s scripts/dev -p 'test_*.py'
```

On Linux, select `-p caelum-core -p caelum-protocol -p caelum-cli` in place of
`--workspace`. `make check` chooses the packages for the host automatically.

The desktop test modules drive Rune's real runtime with keyboard/pointer
input and injected application events. It covers draft ownership, focus,
streaming, approvals/questions, bounded long histories and large reviews.
Core tests use fake provider processes and local HTTP servers for deterministic
protocol, recovery and lifecycle checks. Real-service smoke tests are opt-in;
see the [desktop development guide](../apps/desktop/README.md).

For visual changes, run `make previews` and inspect light/dark, inspector,
compact and settings scenes. Native frame measurements are described in
[performance](performance/README.md).

CI uses GitHub-hosted `ubuntu-24.04` runners for structure and portable Rust
checks and Apple Silicon `macos-15` runners for native desktop tests on every PR.
It captures previews for desktop UI, assets, fixture or CI workflow changes.
Python release/profile checks run separately. The release workflow checks main
commits and builds release assets; PR bundle checks verify the app, DMG and CLI
archive when Rust, app, packaging or CI/release workflow inputs change.

Rust CI jobs upload `cargo-timings-*` artifacts for 14 days, including reports
available after a failed build. Open the timestamped HTML files to inspect each
Cargo invocation's compilation time and concurrency. Packaging and the macOS
development helper also leave these reports in `target/cargo-timings/` locally.
Compare matching revisions, runner architectures, profiles and cache-hit states;
job queue/setup time, test execution, preview capture and packaging smoke checks
are separate from Cargo's build timings.

Use Conventional Commits and the repository's pull-request template.
