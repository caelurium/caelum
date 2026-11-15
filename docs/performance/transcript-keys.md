# Transcript row-key reuse

The transcript supplies Rune's `virtual_rows_version` with a fingerprint of the
selected session slot and its **ordered visible entry IDs**. The fingerprint is
computed during the existing visibility scan. Streaming text, draft edits and
other content-only updates can then reuse Rune's retained key list rather than
formatting and comparing a key for every historical row on each commit.

Same-length replacements, reorders and changes to which entries are visible
change the fingerprint. Hidden entries are excluded, so prepending a hidden entry
can shift source positions without invalidating visible identities. The current
view still supplies fresh row builders and source positions: unchanged keys do
not suppress content, handler, layout or media updates. Header/tail keys retain
their existing session identity. There is no additional retained cache.

This removes redundant key work; the visibility scan and identity hashing are
still linear in history length, and mounted rows still rebuild normally.

## Measurements, 15 November 2026

The comparison uses baseline `2560264` and the production change in `0554a2d`,
release builds on an Apple M5 Pro / 24 GiB, macOS 26.5.1, battery power and
automatic power mode. Builds/checks finished before measurement. Other desktop
activity was not controlled. Raw captures and preserved binaries are under ignored
`apps/desktop/profile-results/transcript-keys-2026-11-15/`.
The [content-free measurement record](transcript-keys-2026-11-15.json) preserves
individual runs, binary hashes, sample/commit counts and timing ranges.

### Headless CPU work

Five pairs alternate order (before/after, after/before, and so on), using the
unchanged navigation and background-redraw profiles. Each phase measures 360
frames at 1280 × 1000 points against six 2,000-entry histories. Values are medians
of the individual runs' percentiles, in **milliseconds**:

| Workload | p50 before → after | p95 before → after |
| --- | --- | --- |
| Background streams | 0.124 → 0.124 | 0.132 → 0.134 |
| Background streams + typing, all frames | 0.126 → 0.124 | 0.713 → 0.574 |
| Typing frames only | 0.690 → 0.560 | 0.809 → 0.643 |
| Foreground + background streams | 0.870 → 0.781 | 1.066 → 0.989 |
| Session switching | 0.47 → 0.47 | 2.41 → 2.32 |
| Session switching + streaming | 1.39 → 1.63 | 3.20 → 3.18 |

Typing-frame p95 fell about **21%** and foreground-stream p95 about **7%**.
Typing-frame p95 ranges overlap: 0.753–0.835 ms before, 0.635–0.858 ms after;
the first two after runs were slower than their paired before runs. Foreground
streaming ranged from 1.053–1.184 ms before to 0.961–1.030 ms after.

Streaming-switch p50 varied from 1.31–1.46 ms before and 1.21–2.02 ms after;
its median increased, and p95 was essentially unchanged. These results do not
establish a session-switching improvement. Idle/hover p95 stayed around 0.12–0.14
ms. Commit counts were identical: background streams 0, typing 60, foreground
streams 360, switches 120, and streaming switches 240. This saves work within
commits rather than reducing their number or delaying events.

### Packaged native controls

Three alternating pairs per workload used the existing native latency replay,
with tracing enabled on both builds. Requested window size was 1280 × 1000 points
on the built-in Retina display in dark appearance; refresh rate was not measured.
Every run completed all 80 selections and 80 edits, and each streaming run applied
all 2,280 deltas. Footers matched with no dropped records or abandoned interactions.
The two unfinished fixture commands in each trace are intentionally unexecuted.

| Workload / metric | Median per-run p95 before → after | Per-run p95 range before → after |
| --- | --- | --- |
| Small / selection → drawn | 11.18 → 11.46 ms | 10.93–11.29 → 10.48–11.49 ms |
| Small / edit → drawn | 9.86 → 10.24 ms | 9.77–10.80 → 9.78–10.65 ms |
| Small / frame CPU | 3.70 → 3.64 ms | 3.63–3.78 → 3.58–3.79 ms |
| Six streams / selection → drawn | 20.86 → 17.66 ms | 17.84–21.29 → 16.37–20.62 ms |
| Six streams / edit → drawn | 18.08 → 14.99 ms | 14.78–18.70 → 12.86–18.18 ms |
| Six streams / frame CPU | 11.57 → 11.51 ms | 11.40–11.67 → 11.47–11.63 ms |

Native frame CPU is essentially unchanged. The apparent streaming interaction
gain is dominated by the second pair; edit latency was slightly worse in the
other two pairs. The overlapping ranges and small-history control do not
establish a consistent native draw-latency improvement. Frame CPU p95 remains
above the 8.3 ms budget for 120 Hz. Resource samples and complete per-run
percentiles remain in the record; tracing overhead was not remeasured in this pass.

## Reproduction

Preserve a release app bundle and release desktop test executable from each
revision before measuring. Stop builds and tests, then alternate before/after
executables on the same machine and power mode.

```sh
nix develop -c make app
nix develop -c cargo test --locked --release -p caelum-desktop --no-run
python3 scripts/profile/native_latency.py \
  --app path/to/Caelum.app --workload streaming --runs 1 \
  --output apps/desktop/profile-results/transcript-keys/streaming-before-1
```

Repeat with `--workload small` as a control. Use a new output directory for each
capture. Both native fixtures perform 80 selections and 80 draft insertions;
streaming adds 2,280 deltas across six 2,000-entry histories. See the
[native latency baseline](native-latency.md) for the exact boundaries, capture
validation and CPU/RSS sampling. These are handler-to-native-draw measurements,
not hardware input-to-photon timings or provider/tool execution measurements.

Run the existing profiles through each preserved test executable:

```sh
path/to/desktop-tests --exact tests::navigation::session_hover_switch_profile \
  --ignored --nocapture --test-threads=1
path/to/desktop-tests --exact tests::redraw::background_redraw_profile \
  --ignored --nocapture --test-threads=1
```

These report headless CPU work separately from native drawing.

## Regression coverage

Native-runtime regressions cover equal-count replacement/reorder, equal-count
visibility swaps, reasoning completion, fresh text/copy actions under unchanged
keys, scroll anchors through hidden and visible history prepends, and copy/draft
ownership across sessions sharing provider entry IDs.
