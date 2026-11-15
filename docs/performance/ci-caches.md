# CI build caches

PR packaging and main release builds use the same `Swatinem/rust-cache` shared
key: `<target>-build`. This preserves the release workflow's existing namespace,
so the first PR can restore a main-produced cache. Native development, portable
tests and Clippy retain separate, explicit shared keys matching their previous
job IDs. Main's reusable CI produces those caches for subsequent PRs.

The action still hashes the runner OS/architecture, Rust environment, manifests
and dependencies. ARM and Intel release artifacts remain separate. Cargo still
validates restored artifacts; no build or check is bypassed on a cache hit.
Workflow changes also trigger the PR packaging job.

## Measurement

The pre-change audit on 15 November 2026 examined ten recent successful CI runs.
Native jobs had eight exact and two fallback hits. Packaging had six exact hits,
one fallback and three misses; its `macos-app` key could not restore the release
workflow's `<target>-build` entries. Main already held ARM and Intel release
caches. Across the larger audit, packaging step elapsed p50/p95 was 301/617 s.

Compare the cache restore log and packaging step duration on a PR changing only
this workflow against the same-revision baseline. Record the run URL, exact or
fallback hit, restored namespace and elapsed seconds separately from runner
queue/setup time. A cache hit is evidence of reuse, not an end-to-end speedup:
this action excludes workspace build products and incremental state by default,
so application compilation remains. Hosted timing needs repeated runs before
claiming a new p50/p95 or the overall feedback target.
