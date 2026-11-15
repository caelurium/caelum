# OpenCode provider read reduction

The application restores OpenCode metadata and the initial history page in one
worker request. One fresh metadata read validates both the session ID and checkout,
one active-session read supplies working state, and one message read supplies the
page. The workspace applies the bundled result under the original session revision.
This removes a second metadata/activity pair and the resume-to-history handoff.

Ownership-only operations also stop reading activity they never consume. Session
diffs, prompt admission, agent selection, inbox operations, title regeneration,
side questions, cancellation and shell reads retain fresh ownership checks. History,
session lists, archive/delete and checkout-idle checks retain fresh activity reads.
There is no cross-request metadata/activity cache or new single-flight mechanism.

## Measurements, 15 November 2026

Five alternating before/after release pairs compared `2d7d56c` with `b2d23a1` on
macOS 26.5.1, Apple M5 Pro / 24 GiB, battery power with automatic power mode.
OpenCode 2.2.7 was already warm. No builds or tests ran alongside the captures.
Both executables used the unchanged `session_load` example and a frozen workspace
with 10 projects and 41 OpenCode sessions, 31 archived. The oldest open session
consistently returned 50 messages / 500,922 JSON bytes / 166 transcript entries.

| Metric | Median before → after | Range before → after |
| --- | --- | --- |
| Selected-history core readiness | 30.00 → 21.96 ms | 15.19–31.28 → 17.17–23.30 ms |
| Direct resume, separate-call control | 2.63 → 2.58 ms | 2.10–6.96 → 2.21–2.86 ms |
| Direct history, separate-call control | 5.47 → 4.45 ms | 4.25–8.41 → 3.96–4.85 ms |
| Direct conversation reconciliation | 0.22 → 0.23 ms | 0.21–0.26 → 0.19–0.37 ms |

Core readiness improved by **27%** in this sample. Ranges overlap, and the first
before run was faster than its after pair; all five pairs are retained in the
[content-free measurement record](provider-reads-2026-11-15.json). These are
warm-service headless timings, excluding native rendering, executable startup and
disk persistence. They do not establish native input latency, cold-start p95 or
model/tool execution speed. The unchanged direct history path also varied between
builds, so the wall-clock difference is not an isolated HTTP-cost estimate.

Parent-linked traces show the session-loading path changes from two metadata,
two activity and one message read to one of each: **5 → 3 reads**, excluding
unchanged connection initialization. Each completed session-diff request drops
from three reads to two. Traced initial resume commands also contain one service
info and one form-recovery read on both builds.

Before, resume and history completion elapsed medians were 7.06 and 12.56 ms;
after, the combined resume median was 21.49 ms. These include worker queueing and
provider work but exclude command admission and result application. The old
history-command admission gap was 3.98–10.71 ms (median 6.48 ms); bundling removes
that handoff. The observed gain is in readiness and request count, not a reduction
in the combined worker elapsed time.

Whole-process traces recorded 48–58 HTTP attempts before and 37–39 after, including
12–15 versus four activity reads. These totals include the separate direct control
and partial restoration, so they are not equal-work settled-workspace totals.
There were nine unapplied requests before and ten after, including two unsubmitted
shell/inbox follow-ups after bundled readiness. Every capture finished with zero
dropped records. Raw traces, outputs and the comparison remain under ignored
`apps/desktop/profile-results/provider-read-dedup-2026-11-15/`.

## Reproduction

Build `session_load` at `2d7d56c` and `b2d23a1`, preserving each executable. Use one
read-only saved workspace and run the binaries alternately, after builds/checks
finish. Each traced invocation needs a **new trace path** and exactly one run:

```sh
nix develop -c cargo build --locked --release -p caelum-core --example session_load
CAELUM_LATENCY_TRACE=apps/desktop/profile-results/after-1.trace.jsonl \
  target/release/examples/session_load path/to/workspace.json oldest-open 1 \
  > apps/desktop/profile-results/after-1.json
python3 scripts/profile/latency.py \
  apps/desktop/profile-results/after-1.trace.jsonl
```

The example's direct resume and history calls remain separate as a control. Its
restoration path uses the real workspace/runtime and stops at selected-history
readiness. That endpoint excludes pending background completions and, with the
bundled path, newly queued shell/inbox recovery. Whole-process HTTP totals therefore
describe partial restoration, not a fully settled workspace. Compare HTTP records
by their parent request for the exact work removed from each command.

## Regressions

The local HTTP fixture verifies three reads for a runtime resume, fresh metadata
and activity on retries/pagination, ownership rejection after a previously valid
session moves, and busy/error rejection before archive/delete. Ownership-only
operations succeed even when the activity endpoint fails, while still validating
session ID and checkout before reading or mutating session data.

Workspace regressions cover immediate bundled readiness, inbox recovery before
queued prompts, late completions after navigation/live text/title changes,
cancellation, edited options, session/project/provider mismatches, failed-resume
retries, exhausted pagination and restored failure notices.
