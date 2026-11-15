# Native latency baseline

`CAELUM_LATENCY_TRACE` enables content-free JSONL tracing in the desktop. It is
disabled by default: ordinary runs start no recorder writer, clocks, diagnostic
timers or payload serialization. The output must name a **new** file in an existing
directory. A bounded, nonblocking queue feeds a background writer. Captures stop
accepting records after 250,000 attempts; queue overflow and the record limit are
reported as dropped records in the final summary. Capture errors do not block UI
or provider work.

Records contain static operation labels, timestamps, counts, local request numbers
and process-random anonymous owner/event/tool IDs. They omit prompts, drafts, tool
names, arguments, output, project paths, server URLs and credentials. Anonymous IDs
are useful only within one process. Keep raw captures under the ignored
`apps/desktop/profile-results/` directory.

## Repeatable packaged workload

Build before measuring, then stop other builds/tests and keep the same power mode,
window size, display, theme and foreground visibility across runs:

```sh
nix develop -c make app
python3 scripts/profile/native_latency.py \
  --app target/aarch64-apple-darwin/release/bundle/macos/Caelum.app \
  --workload small --runs 3 \
  --output apps/desktop/profile-results/latency-baseline
python3 scripts/profile/native_latency.py \
  --app target/aarch64-apple-darwin/release/bundle/macos/Caelum.app \
  --workload streaming --runs 3 \
  --output apps/desktop/profile-results/latency-baseline
```

Use the host target triple. Each isolated app exits after about 19 seconds. The
small fixture has two sessions with 100 entries each. The streaming fixture uses
the existing six-session, 2,000-entry-per-session fixture, with one synthetic delta
per session every 50 ms for 19 seconds (2,280 deltas). The replay uses fixed
deadlines rather than the fixture's poll-driven streaming, keeping the amount of
work independent of rendering speed.
After the first drawn frame and a two-second warmup, both perform 80 session
selections and 80 draft insertions, alternating every 100 ms through the native
application handle. They start no provider and do not load or save a workspace.
The existing macOS sampler records CPU/RSS for 15 seconds during each run; this
resource sample covers warmup and most interactions, not the full process lifetime.
Repeat with `--no-trace` and a separate output directory for a resource-only
observer-overhead comparison. This disables both the recorder and encoded-frame
hook while keeping the same workload and CPU/RSS sampler. It does not establish
the latency cost of tracing; differences below run-to-run noise are inconclusive.
Reports include the packaged executable's SHA-256, and traced runs fail if any of
the 80 navigation/draft samples or 2,280 streaming deltas are missing, abandoned
or dropped.

These are **handler-to-native-draw** measurements. The harness does not simulate
OS pointer/keyboard delivery. Rune's drawn callback observes a submitted/presented
frame, not physical display scanout. `frame.update` includes runtime update,
layout and paint; `frame.encode` measures CPU encoding/submission, not GPU time.
`frame.cpu` adds update and encode for each matched frame before taking percentiles.
Do not describe either as hardware input-to-photon latency. Startup begins when
the recorder is initialized in `main`, excluding executable/LaunchServices startup.

## Actual workspace and provider timings

Launch the packaged app with a separate state snapshot and trace path, then use
the normal navigation and composer controls:

```sh
open -n target/aarch64-apple-darwin/release/bundle/macos/Caelum.app \
  --env CAELUM_STATE="$PWD/apps/desktop/profile-results/workspace.json" \
  --env CAELUM_LATENCY_TRACE="$PWD/apps/desktop/profile-results/interactive.jsonl"
```

Capture cold restoration, warm switches, sending, background execution and typing
during output. Quit normally to drain the writer. Note the workload sizes and
provider/model configuration alongside results. Composer edit timing begins in
the native change callback, after editing has occurred. Navigation records both
the first drawn frame and the first drawn frame with loaded history. Superseded
navigation is abandoned rather than attributed to the new selection.

For a read-only provider baseline using the existing session-load example:

```sh
nix develop -c cargo build --locked --release -p caelum-core --example session_load
CAELUM_LATENCY_TRACE="$PWD/apps/desktop/profile-results/provider.jsonl" \
  target/release/examples/session_load path/to/workspace.json selected 1
```

Use a separate process/file for each repeat. This example omits native rendering
and completes persistence in memory; its stage timings must not be presented as
native or disk-write measurements. It may start an owned provider if no registered
service is available. The supplied workspace is read only.

## Interpret the report

```sh
python3 scripts/profile/latency.py apps/desktop/profile-results/interactive.jsonl
```

- `request.*.admission`: command creation to runtime submission, including any
  persistence gate and intervening UI scheduling. This is not pure fsync time.
- `request.*.queue`, `.worker`, `.delivery`, `.apply`: runtime queue wait, worker
  execution, result-to-core-apply wait, and synchronous core application.
- `http.*`: OpenCode JSON API round trips, including body read and JSON decode.
  HTTP counts include failed attempts; response bytes cover decoded-response input,
  not transport headers. Service-discovery response size is not recorded. SSE
  connection time is included in `connection.checkout`, not HTTP request counts.
- `provider.*.delivery` and `.drawn`: scoped SSE event receipt to core apply and
  selected-conversation draw. Only recognized OpenCode events with native event
  IDs can be correlated; hidden transcripts produce no draw sample. Delivery
  includes event forwarding, branch bookkeeping and the UI inbox.
- `tool.observed_call_to_result`: local receipt interval; the corresponding
  `tool.provider_call_to_result` uses only provider timestamps. These describe
  lifecycle-event boundaries, not necessarily exact process execution time.
- `tool.last_result_to_next_step` and `model.observed_step_to_first_token` use
  local receipt time, scoped by session and tool/assistant message identity.

The report includes p50/p95/p99/max, sample counts, frame outcomes, HTTP counts and
response sizes. Missing pairs never become zero-latency samples. Inspect abandoned
actions, unfinished operations, duplicate stages and dropped records before
comparing runs. A missing footer or dropped records marks the capture incomplete;
an interrupted JSON line is rejected. Native synthetic runs deliberately have no
HTTP/tool samples. Record counts must also match the footer's attempted/dropped
totals. Compare repeated runs individually rather than pooling away
outliers, and measure capture overhead before interpreting very small differences.

## Local baseline, 15 November 2026

[Content-free per-run results](native-latency-2026-11-15.json) retain p50/p95/p99/max,
frame and sample counts, CPU/RSS and provider request counts. Native measurements
use release revision `2fe56dc`, on an Apple M5 Pro with 24 GiB, macOS 26.5.1,
battery power in automatic mode, dark appearance and the built-in Retina display.
The requested window size was 1280 × 1000 points; actual display refresh was not
independently recorded. No builds or tests ran during measurement. Other desktop
activity was not controlled, so these are local observations rather than CI limits.

Three traced repeats and three resource-only repeats per workload used the same
packaged executable, alternating trace order (on/off, off/on, on/off). The small
workload has 200 initial entries total; streaming has 12,000 and exactly 2,280
applied deltas per run. Each traced run completed all 80 selections and 80 edits
with a matching footer, no dropped records and no abandoned interactions.

| Workload | Measurement | Median per-run p95 | Per-run p95 range |
| --- | --- | ---: | ---: |
| Small | Selection → drawn | 12.94 ms | 12.71–13.61 ms |
| Small | Draft insertion → drawn | 11.38 ms | 11.34–12.02 ms |
| Small | Frame CPU update + encode | 4.06 ms | 4.00–4.08 ms |
| Six streams | Selection → drawn | 25.41 ms | 21.19–26.74 ms |
| Six streams | Draft insertion → drawn | 22.16 ms | 17.93–23.32 ms |
| Six streams | Frame CPU update + encode | 13.67 ms | 11.55–16.10 ms |

Histories are already loaded, so readiness accompanies the selection frame. The
worst selection sample was 28.90 ms and the worst edit 25.59 ms. With 80 samples,
nearest-rank p99 equals the maximum; it is not a well-resolved tail estimate.
The warm-interaction goal of p95 below 50 ms is met in these fixtures, while the
streaming frame tail exceeds the 8.3 ms CPU budget for a 120 Hz target. That points
to native update work worth profiling before claiming consistently smooth 120 Hz.
Each synthetic trace has two intentionally unexecuted fixture commands (`task`
and `choices`), reported as unfinished requests; they are not provider timings.

### Observer overhead

For small histories, mean native CPU was 22.93–23.50% with tracing versus
20.86–23.06% without it (100% is one fully occupied core). Paired differences were
+0.44 to +2.07 percentage points; median RSS differed by +0.67 to +1.25 MiB.
The resource sampler ran in both modes. This measures the combined recorder/frame
hook effect plus run-to-run noise, not individual event cost or latency overhead.

Streaming CPU varied substantially: traced runs used 32.58–43.20%, and untraced
runs used 32.66%, 39.61% and 7.62%. In the last untraced run CPU fell to roughly 1%
after four seconds. Its presentation was not instrumented, so it is retained in
the artifact but cannot support an overhead estimate. Streaming observer overhead
remains inconclusive; the traced interaction counts and timings above are complete.

### Read-only provider baseline

Three separate captures of `session_load … oldest-open 1` used the same copied
workspace (10 projects, 41 saved OpenCode sessions, 31 archived), a warm registered
OpenCode 2.2.7 service, and core/example revision `600d791` (unchanged by `2fe56dc`).
Each history returned 50 messages / 500,922 JSON bytes / 166 transcript entries.
Core restoration readiness was **30.42 ms median**, range **25.11–34.32 ms**.

Selected-history command stages were admission 8.93–12.16 ms, worker queue wait
0.013–0.139 ms, worker execution 6.82–14.87 ms, delivery 0.019–0.043 ms and core
application 0.335–0.406 ms. Admission includes the example's command-drain and
completion loop; it is not an isolated desktop scheduling measurement.

Whole-process captures recorded 51–55 HTTP attempts, including both the isolated
direct load and competing restoration reads. They ended after selected-history
readiness and runtime shutdown, with nine requests not applied to the workspace;
these counts do not represent a fully settled workspace. All record footers
matched, with no drops. No real tool/model execution occurred in these captures,
so tool lifecycle and first-token distributions still need an interactive capture.

Raw native captures remain under ignored
`apps/desktop/profile-results/native-latency-2026-11-15-fixed/`; provider captures
are under `apps/desktop/profile-results/native-latency-2026-11-15/provider/`.
