# Native system-font layout

Caelum names Rune's macOS system-font fallbacks explicitly, in the same order:
`System Font, '.SF NS', system-ui`. Other platforms retain `system-ui`, and the
monospace slot retains `ui-monospace`.

In pinned Rune `e5e9ea9`, `TextSystem::primary_font` discards generic CSS families
when predicting the first font's tracking. With `system-ui` alone, the actual
layout reaches SF through Rune's named symbol fallbacks. Rune then discovers the
missing point-size tracking correction and shapes the entire paragraph again.
The cost grows with the streamed paragraph. Naming the already-used native face
lets Rune supply its tracking correction on the first pass.

The typography regression retains the same faces and wrapping for its tested
sizes, weights and fallback runs. Resolving the face early also supplies SF's
weight-specific hidden italic-axis coordinates; italic outlines can therefore
change to the named font instance. The regression covers faces, line geometry and
unchanged upright/monospace instances at 1×, 2×
and 3×, across sizes and weights, with Unicode, emoji and inline code.

All 24 before/after native PNG pairs were byte-identical: `messages`, `working`,
`compact`, `inspector`, `settings` and `tools`, in light/dark appearance at 800 ×
620 points and 1×/2× scale. Representative 2× light/dark previews were also inspected
visually. These fixture images do not establish identical italic outlines at every
weight.

## Attribution, 15 November 2026

The baseline is `785caaf`, after transcript row-key reuse; the production change is
`3241106`. A symbol-enabled release used `CARGO_PROFILE_RELEASE_STRIP=none` and
`CARGO_PROFILE_RELEASE_DEBUG=line-tables-only`, with the normal thin-LTO release
optimization. The existing fixed-deadline native `streaming` workload applied
2,280 deltas across six 2,000-entry histories and completed 80 selections and 80
edits. `/usr/bin/sample` sampled the main thread for 15 seconds at a requested
1 ms interval, starting near launch. `RUNE_COMMIT_BUDGET_MS=0.1` supplied rate-limited
phase diagnostics. No builds or tests ran alongside either capture.

| Inclusive main-thread samples | Before | After |
| --- | ---: | ---: |
| All stacks, including waits | 9,953 | 9,800 |
| Runtime frame step | 5,360 | 5,029 |
| Commit | 4,139 | 3,567 |
| Text layout | 2,643 | 1,802 |
| Initial shaping call (`text.rs:727`) | 1,296 | 1,724 |
| Corrective shaping call (`text.rs:751`) | 1,277 | 0 |
| Markdown construction | 119 | 147 |

These rows overlap; **do not sum them**. The baseline's corrective shaping call
accounts for about 24% of sampled frame-step stacks. Its absence from the after
sample supports the proposed mechanism, rather than proving that every possible
fallback run now takes one pass. Mixed-font content can still require correction.
Sampling and phase timing substantially inflate elapsed time and alter frame
counts, so these captures are attribution evidence, not before/after latency
measurements. The ordinary release captures below exclude both observers.

## Release measurement controls

An initial five alternating before/after pairs per workload used preserved normal
release bundles: `785caaf` and the production change in `3241106`. Streaming ran
before the small-history control in each pair. The candidate bundles were built from the
final production configuration before the test-only refinements and commit.
The [content-free measurement record](native-text-layout-2026-11-15.json) retains
per-run percentiles, counts, CPU/RSS, binary hashes and attribution counts.

The initial streaming result was worse: median per-run frame p95 **11.49 →
13.63 ms**, with the candidate slower in four of five pairs. That result is retained
in the record rather than discarded. Subsequent external WindowServer observations
found that losing the front-window position coincided with a sharp slowdown on the
baseline too (about 11.5 → 16.0 ms p95). The window manager also supplied **800 ×
948 points**, rather than the requested 1280 × 1000. The initial captures did not
record either condition, so they cannot establish matched foreground comparisons.

The native runner now records bounds and front-window position every 0.5 seconds
through exit and fails a run if those observations change after launch warmup.
This adds an external observer to both builds; it neither changes the packaged
applications nor repeatedly activates their windows. The content-free record keeps
the initial uncontrolled captures separately from the window-observed comparison.

The host was an Apple M5 Pro / 24 GiB, macOS 26.5.1, on battery in automatic power
mode, in dark appearance on the built-in Retina display. Refresh rate and other
desktop activity were not controlled. Builds and tests were stopped. Both builds
used latency tracing and the same 15-second CPU/RSS sampler; tracing overhead was
not remeasured here. WindowServer sampling is periodic, not continuous keyboard
focus tracking.

### Matched-window results

A second batch attempted five alternating pairs per workload with the new observer.
Six of its 20 captures failed the window check. Both runs of a pair are excluded
if either fails or their bounds differ, leaving pairs **1, 2 and 5** for each
workload. All attempts remain in the record; none were replaced or excluded based
on timing. Accepted runs observed the same position and **800 × 948-point** size.

Every trace in both batches completed 80 selections, 80 readiness observations and
80 edits, with matching footers, no drops or anomalies, and all 2,280 deltas in each
streaming run. The two unfinished fixture requests per capture are intentionally
unexecuted. The small control has two 100-entry sessions; streaming has six
2,000-entry sessions. Values below are medians of per-run percentiles, in **ms**:

| Workload / metric | p50 before → after | p95 before → after | Per-run p95 range before → after |
| --- | --- | --- | --- |
| Six streams / update + encode | 0.92 → 1.26 | **11.42 → 9.69** | 11.40–11.58 → 9.67–9.85 |
| Six streams / selection → drawn | 20.20 → 15.82 | 21.94 → 19.64 | 18.40–21.97 → 17.49–20.21 |
| Six streams / edit → drawn | 12.95 → 12.29 | 19.08 → 16.34 | 14.01–19.13 → 15.93–17.17 |
| Small / update + encode | 1.13 → 1.10 | 3.82 → 3.68 | 3.64–3.88 → 3.61–3.74 |
| Small / selection → drawn | 10.99 → 8.11 | 12.50 → 11.74 | 12.15–12.99 → 10.69–12.67 |
| Small / edit → drawn | 9.80 → 7.14 | 11.41 → 10.94 | 10.57–11.80 → 10.13–11.17 |

Streaming frame p95 fell **14.9–15.2% in every accepted pair**, with disjoint ranges.
The frame tail still exceeds 8.3 ms; this does not establish consistently smooth
120 Hz. Selection/edit medians improved, but both were worse in pair 2 and their
ranges overlap. The small frame-time result is modest and also overlaps. With 80
interactions, nearest-rank p99 equals the maximum and is not a resolved tail estimate.

This is a **frame-tail improvement, not a total CPU saving**. Streaming frame p50
increased, and drawn-frame counts rose from 1,853–1,860 to 1,900–1,911 for the same
applied workload. Median per-run process CPU was **33.03% → 33.76%** (ranges
32.76–33.24% → 33.51–34.02%; 100% is one fully occupied core). Median RSS was
129.36 → 129.75 MiB; median sampled peak RSS was 132.77 → 134.59 MiB. Small-workload
CPU was 22.08% → 21.72%, with overlapping ranges, and median RSS was
98.72 → 98.45 MiB. These 15-second samples do not establish sustained resource use.

Raw bundles, stacks, traces and preview comparisons remain under ignored
`apps/desktop/profile-results/native-text-layout-2026-11-15/`.

## Reproduction

Preserve a normal release app bundle from each revision, then run the existing
native workload with builds/tests stopped, alternating before/after order:

```sh
nix develop -c make app
python3 scripts/profile/native_latency.py \
  --app path/to/Caelum.app --workload streaming --runs 1 \
  --output apps/desktop/profile-results/native-text-layout/streaming-before-1
```

Repeat with `--workload small` as a control. The
[native latency baseline](native-latency.md) describes trace validation and the
window checks. Compare complete pairs with matching observed bounds and retain
failed captures. The CPU/RSS sampler covers 15 seconds. Native `frame.cpu` is the
existing report label for Rune's elapsed update + encode durations, not a thread
CPU clock; interactions
start at the application handler and end at the native draw callback, excluding
OS input delivery and physical display scanout. The synthetic fixture omits
provider/network/storage work and real tool execution.

For attribution only, build another bundle with the two Cargo environment
overrides above, launch it with `--latency-workload streaming` and
`RUNE_COMMIT_BUDGET_MS=0.1`, and attach `/usr/bin/sample PID 15 1 -mayDie -file FILE`.
Keep raw stacks and traces under ignored `apps/desktop/profile-results/`.
