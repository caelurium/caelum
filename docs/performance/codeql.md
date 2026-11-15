# CodeQL caching and measurement

The advanced workflow analyzes Actions, Python and Rust on standard Ubuntu
runners, with the complete default query suites on every PR/main change and a
weekly schedule. It retains Rust's `none` build mode and default all-features
extraction, and lets CodeQL use the runner's available threads and memory.

CodeQL's built-in dependency cache currently has no Rust implementation. The
Rust extractor does support `cargo_target_dir`: its internal build-script and
macro compilation can reuse Cargo artifacts instead of a fresh scratch
directory. The workflow gives it `target/codeql` and caches that directory plus
Cargo downloads with `Swatinem/rust-cache`. Keys include the CodeQL version,
Rust environment, runner architecture and dependency manifests. Each run still
extracts the current source, finalizes a fresh database and runs every query.

The cache contains no analysis database or previous scan results. Cold caches
perform the same analysis. Python/Actions retain the action's default incremental
and TRAP-cache behaviour. No paths, source files or queries are excluded.

## Activation

GitHub rejects advanced SARIF uploads while default setup is configured. During
PR evaluation, leave default setup enabled: it continues to publish required
security results, while this workflow runs the full analysis and saves its SARIF
as artifacts. Analysis failures still fail the workflow.

After merging this workflow, switch the repository from default to advanced
setup, enable publication, then dispatch a full main scan:

```sh
gh api --method PATCH repos/{owner}/{repo}/code-scanning/default-setup \
  -f state=not-configured
gh variable set CODEQL_ADVANCED --body enabled
gh workflow run codeql.yml --ref main
```

Confirm all three languages publish successfully in code scanning before relying
on the new workflow. Retain the existing CodeQL ruleset requirement. To roll back,
delete `CODEQL_ADVANCED` and re-enable default setup for Actions, Python and Rust
with the default suite. This PR does not itself change repository scan settings.

## Measurement

Before this change, 30 successful PR scans had completion p50/p95 of 445/499 s.
A sampled Rust scan spent approximately 162 s extracting, 48 s finalizing and
244 s running queries, already using four threads. Caching internal Cargo output
addresses only part of extraction; it cannot remove query execution or macOS CI
queue time and does not establish the overall two-minute feedback goal.

Compare cold and warm runs of the same revision and CodeQL version. Record the
cache key/hit, extraction, finalization, query and total job durations from the
logs. Inspect SARIF query IDs, source coverage, diagnostics and findings in both
runs. Repeat on source changes before claiming a new p50/p95. The uploaded SARIF
and run logs make failures and coverage available during the staged migration.
