# Long-session scrollback

The transcript's visible-row observer maintains the mounted-entry set and current
timeline prompt. Those are bookkeeping for later handlers and polling; changing
them does not itself change the view. Previously, crossing a visible row boundary
returned `true`, requesting a full application view rebuild. That repeated the
history visibility scan and construction/layout of the mounted rows.

The observer now returns a view change only when it queues an image request whose
loading button must become disabled. It still schedules the existing poll; newly
visible patch inputs are prepared there and redraw when their document changes.
Rune's virtualization continues to mount newly encountered rows.

Regression coverage checks that scrolling within mounted rows updates the timeline
prompt without a view commit, and that newly revealed patch previews, image busy
and error states, and session-owned reading positions still update. Existing
transcript identity, history prepend, copy, streaming-anchor and editor tests remain
registered.

## Workload and measurement boundaries

The [native latency runner](native-latency.md#long-session-scrollback) now supports
`scroll` and `scroll-streaming`. Both use a selected 3,000-entry history containing
variable-length Markdown, Unicode, code and tool calls, with every third tool
expanded. Five other sessions contain 2,000 entries each. The streaming variant
applies 1,900 background deltas during the 19-second run.

The native replay requests 1,920 24-point scroll steps on fixed 120 Hz deadlines:
eight seconds up through history and eight seconds back down, after two seconds of
warmup. The view is not explicitly invalidated by scroll requests. Coalesced
requests retain their accumulated distance, and subsequent requests use the drawn
offset to account for virtual-height corrections. Offset movement and all request
completions are validated. A separate CPU-only wheel replay uses the same content
and stays in transcript padding so nested tool scrollers cannot consume the input.

Active-pass `frame.cpu` means elapsed native update + encode, not thread CPU time.
Draw-callback intervals and observed request steps expose scheduling/coalescing;
they do not measure physical scanout or hardware dropped frames. The replay omits
OS input delivery, trackpad momentum, scrolling inside tool output, real providers
and network/storage work. It does not establish that every source of subtle
scrollback lag is resolved.

Native commit counts cover the difference between the first and last observed
draw callback in each pass. Whole-capture frame metrics retain launch and settling;
the active-pass distributions exclude them. Resource samples cover 15 seconds,
including warmup and most scrolling, rather than the full process lifetime.

## Rebuild attribution

One before/after CPU-only wheel replay made the same 1,920 moves and reached the
same starting, turnaround and ending offsets in both builds. View commits fell
**376 → 81** when idle and **377 → 93** with background streams. This isolates the
removed rebuild requests; it is not a repeated native latency result. Newly mounted
virtual windows and actual UI changes still require commits.

## Native results, 15 November 2026

The baseline is `fc91bac` (base `60115a0` plus the scrollback harness); the production
change is `b84b805`. Both normal release bundles were built from those sources
before committing, so their capture metadata reports `60115a0` and a dirty tree.
The [content-free measurement record](scrollback-2026-11-15.json) retains executable
hashes, all 24 native attempts, per-run distributions, counts and resource/window
observations. Four preliminary captures are retained separately from the paired
batch; one failed its front-window check.

The primary comparison attempted five alternating before/after pairs per workload.
All 20 captures passed the workload and window checks, with the same observed
**800 × 948-point** bounds. No pairs were excluded. Every capture completed all
1,920 requests, the streaming captures applied all 1,900 deltas, and trace footers
matched without drops or anomalies. Two unexecuted fixture requests per capture
are intentional; no provider or tool process runs.

The host was an Apple M5 Pro / 24 GiB, macOS 26.5.1, on AC power in automatic mode,
with dark appearance on the built-in Retina display. Builds/tests were stopped.
Refresh rate and unrelated desktop activity were not controlled. Both builds used
the same latency, resource and window observers; their overhead was not remeasured.
Periodic WindowServer observations do not prove continuous keyboard focus.

Values below are medians of per-run percentiles, in **milliseconds**. “Up” visits
older rows; “return” scrolls back through the visited region.

| Workload / pass | Frame p50 before → after | p95 before → after | p99 before → after | Per-run p95 range before → after |
| --- | --- | --- | --- | --- |
| Idle / up | 1.06 → 1.04 | 3.91 → 4.01 | 4.30 → 4.63 | 3.88–4.03 → 3.85–4.45 |
| Idle / return | 1.01 → 1.00 | **3.67 → 1.46** | 3.98 → 4.15 | 3.59–3.69 → 1.42–1.53 |
| Background streams / up | 1.11 → 1.10 | 4.12 → 4.24 | 4.53 → 4.82 | 4.00–4.35 → 4.07–4.85 |
| Background streams / return | 1.08 → 1.07 | **3.86 → 1.58** | 4.11 → 4.39 | 3.74–4.26 → 1.43–1.68 |

Return-pass frame p95 improved **57.9–60.8%** in every idle pair and **55.8–63.0%**
in every background-streaming pair. First-pass frame p95 did **not** improve:
medians rose about 3%, with overlapping ranges, and median frame p99 rose in both
passes. The expensive newly mounted-row frames remain a follow-up target.

Median native view commits fell **327 → 68** on the idle outward pass and
**322 → 68** with background streams; both return passes fell **180 → 45**.
Outward draw-callback interval p99 improved **16.41 → 13.52 ms** when idle and
**16.51 → 13.60 ms** with background streams, with disjoint per-run ranges. Return
interval p99 was **14.03 → 13.41 ms** and **14.50 → 13.57 ms**, respectively.
These are application draw-cadence observations, not display presentation timings.
All active-pass update + encode samples were below 8.33 ms, but callback tails and
request coalescing still prevent a claim of consistently smooth 120 Hz.

Median sampled process CPU while scrolling fell **25.43% → 21.33%** without streams
and **27.98% → 23.89%** with background streams: approximately **16.1%** and **14.6%**
relative reductions.
Per-run ranges were 24.62–25.79% → 20.91–21.85% and 27.24–28.31% → 23.35–24.82%;
100% is one fully occupied core. Median RSS was **126.81 → 126.25 MiB** and
**127.19 → 126.95 MiB**, with overlapping ranges. These short samples do not
establish sustained memory or energy behavior.

All 12 light/dark 2× PNG pairs were byte-identical at 800 × 620 points: `messages`,
`tools`, `patch-long`, `images`, `compact` and `inspector`. Representative messages,
patch and image captures were visually inspected. Raw bundles, traces, reports and
previews remain under ignored `apps/desktop/profile-results/scrollback-2026-11-15/`.

## Reproduction

Preserve a release bundle from each revision, including the identical workload
harness in the baseline. Stop builds/tests and alternate before/after order:

```sh
nix develop -c make app
python3 scripts/profile/native_latency.py \
  --app path/to/Caelum.app --workload scroll --runs 1 \
  --output apps/desktop/profile-results/scrollback/pair-1-before
```

Repeat with `scroll-streaming`, reversing order in alternate pairs. Retain every
attempt. Exclude a complete pair if either capture fails workload/window checks or
their observed bounds differ; never filter on timings. The native runner samples
CPU/RSS for 15 seconds and WindowServer every 0.5 seconds through app exit.

For the isolated CPU-only replay:

```sh
nix develop -c cargo test --locked --release -p caelum-desktop \
  transcript_scrollback_profile -- --ignored --nocapture --test-threads=1
```

This replay uses an 800 × 948-point viewport at 2× scale and a virtual 120 Hz clock.
It measures wheel dispatch, background event application and runtime frame work,
excluding native encoding and presentation. Its timings cannot replace the native
comparison.
