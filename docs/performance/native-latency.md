# Native latency baseline

`CAELUM_LATENCY_TRACE` enables content-free JSONL tracing in the desktop. It is
disabled by default: no writer, clocks, diagnostic timers or payload serialization
run without an explicit capture. The output must name a **new** file in an existing
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
the existing six-session, 2,000-entry-per-session workload and synthetic deltas.
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
the 80 navigation/draft samples are missing, abandoned or dropped.

These are **handler-to-native-draw** measurements. The harness does not simulate
OS pointer/keyboard delivery. Rune's drawn callback observes a submitted/presented
frame, not physical display scanout. `frame.update` includes runtime update,
layout and paint; `frame.encode` measures CPU encoding/submission, not GPU time.
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
HTTP/tool samples. Compare repeated runs individually rather than pooling away
outliers, and measure capture overhead before interpreting very small differences.
