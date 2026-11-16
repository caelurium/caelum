# CodeQL caching and measurement

The advanced workflow analyzes Actions, Python and Rust on standard Ubuntu
runners, with the complete default query suites on every PR/main change and a
weekly schedule. It retains Rust's `none` build mode and default all-features
extraction, and lets CodeQL use the runner's available threads and memory.

CodeQL's built-in dependency cache currently has no Rust implementation. The
Rust extractor does support `cargo_target_dir`: its internal build-script and
macro compilation can reuse Cargo artifacts instead of a fresh scratch
directory. The workflow gives it a directory under the runner's temporary root,
outside the source checkout, and caches that directory plus Cargo downloads with
`Swatinem/rust-cache`. Keeping it outside the checkout prevents warm-cache
generated Rust from being treated as additional repository source. Keys include the CodeQL version,
Rust environment, runner architecture and dependency manifests. Each run still
extracts the current source, finalizes a fresh database and runs every query.

The cache contains no analysis database or previous scan results. Cold caches
perform the same analysis. Python/Actions retain the action's default incremental
and TRAP-cache behaviour. No paths, source files or queries are excluded.

## Activation

GitHub rejects advanced SARIF uploads while default setup is configured. During
PR evaluation, leave default setup enabled: it continues to publish
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
on the new workflow. The current `main` ruleset requires the Rust, native and
packaging checks; it has no CodeQL requirement. Keep those gates during the
cutover. If CodeQL becomes required, use the advanced workflow's published check
contexts and ensure it also runs on merge-group commits before enabling a queue.
To roll back, delete `CODEQL_ADVANCED` and re-enable default setup for Actions,
Python and Rust with the default suite. This PR does not itself change repository
scan settings.

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

## Hosted result, 15 November 2026

[Run 43760144672](https://github.com/caelurium/caelum/actions/runs/43760144672)
analyzed `806c44c` with CodeQL 2.27.4 on standard Ubuntu. Attempt 1 had a cold
extractor cache; attempt 2 reran the Rust job at the same revision and restored
`v0-rust-codeql-rust-extractor-2.27.4-Linux-x64-e88ea626-cfc57aa6` as an exact hit.

| Rust phase | Cold cache (seconds) | Warm cache (seconds) |
| --- | --- | --- |
| Extraction | 125.23 | 94.05 |
| Finalization | 37.87 | 39.30 |
| Queries | 195.99 | 198.17 |
| Complete job | 397 | 364 |

The warm run reduced extraction time by about **25%**, and complete job time by
about **8%** in this pair. Query execution remains the largest phase. This is a
single same-revision comparison, not a new p50/p95 or the two-minute CI target.

Cold and warm SARIF had identical rule IDs, source-file lists, extraction/query
metrics, findings and warning/error notifications: 26 rule IDs, 222 successfully
extracted repository files, 60,919 source lines and no extraction errors. Rule IDs
also matched main's default Rust setup. Actions and Python jobs passed with their
default suites. An earlier in-checkout cache experiment added generated Rust
files to warm scans; the external cache location removes that discrepancy without
adding source exclusions. Both corrected runs retained complete coverage.
