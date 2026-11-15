# Native preview capture

`make previews` builds the desktop once and captures the complete scene and
interaction catalogue in both themes and viewport sizes. Captures run in a
bounded pool of up to four processes (limited by CPU count). To reduce local
resource usage or compare sequential behaviour:

```sh
nix develop -c python3 apps/desktop/preview/capture.py \
  apps/desktop/preview-results --workers 1
```

Each capture owns its PNG, tree JSON, input script and log. A child has a 60-second
timeout. A rerun removes that capture's previous outputs before starting, and a
successful child must produce a PNG and parseable tree. Failures retain logs,
allow the remaining captures to finish, and fail the overall command. CI uploads
these artifacts on failure as well as success. `manifest.json` records every
expected capture, outcome and elapsed time in deterministic catalogue order.

## Local measurement, 15 November 2026

The workload was all **700 states**, using the unchanged desktop source at
`2d7d56c`, its optimized development binary, scale 1, both themes, and the existing
800×620/1280×1000 viewport rules. The host was a 15-core Apple M5 Pro with 24 GiB
RAM. No builds or other tests ran concurrently. The original sequential script
took 93.99 s including a 16.31 s build.

The bounded implementation was then measured without rebuilding, in worker
order 1, 2, 4, 4, 2, 1. These timings include child startup, capture, output
validation and manifest creation:

| Workers | Runs (seconds) | Median (seconds) | Completed per run |
| --- | --- | --- | --- |
| 1 | 72.08, 73.83 | 72.95 | 700/700 |
| 2 | 35.29, 35.24 | 35.27 | 700/700 |
| 4 | 20.02, 20.28 | 20.15 | 700/700 |

Four workers reduced capture elapsed time by about **72%** against the matched
one-worker control. These are local capture measurements, not hosted CI p50/p95
or app input-latency measurements. Hosted runner queue/setup and compilation
remain separate costs.

All runs produced exactly the baseline's PNG filenames and complete tree/log
sets. Byte comparisons found 1–4 differing PNGs per run and 14–17 differing trees:
existing fixture attachment IDs contain process/time values, shell durations
vary by 0.1 s, and session ordering can vary across a wall-clock boundary. These
differences were inspected alongside native light/dark images; byte-for-byte
determinism is not claimed. Keep per-run manifests and captures in ignored
`apps/desktop/profile-results/` when repeating the workload.

## Hosted observation

The preceding sequential
[CI run 43748407072](https://github.com/ch55secake/caelum/actions/runs/43748407072)
captured the same 700-state catalogue in a 470 s capture step. The first parallel
[CI run 43758940877](https://github.com/ch55secake/caelum/actions/runs/43758940877)
used three workers (the hosted CPU count) and took 318 s, including its 0.85 s
Cargo build check. Desktop/core Rust source and the catalogue were unchanged
between these revisions. All CI jobs passed. This single hosted pair observed a
32% lower capture-step duration; runner variability and queue time still require
more samples before estimating end-to-end percentiles.
