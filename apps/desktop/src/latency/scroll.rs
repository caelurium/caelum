//! Native scrolling replay, isolated from providers and saved workspaces.
use crate::{Workspace, live::Live};
use caelum_core::application::{Event, Provider};
use caelum_core::diagnostics;
use serde_json::json;
use std::{
    cell::Cell,
    time::{Duration, Instant},
};

pub(crate) const STEPS: u64 = 1_920;
pub(crate) const DISTANCE: f32 = 24.0;

pub(crate) fn workspace() -> Workspace {
    let mut state = crate::evaluation::workspace();
    let live = state.live.as_mut().unwrap();
    live.evaluation = false;
    live.fixture = true;
    let turns: Vec<_> = (0..1_000).map(|n| {
        let paragraphs = (0..(n % 4 + 1)).map(|p| format!(
            "Paragraph {p}: preserve **session ownership**, `draft state`, and the reading position. Unicode stays readable: café, Ελληνικά, 日本語, ☁️.\n\n"
        )).collect::<String>();
        json!({"id":format!("turn-{n}"),"status":"completed","items":[
            {"id":format!("user-{n}"),"type":"userMessage","content":[{"text":format!("Review change {n} and explain the tradeoffs.")}]},
            {"id":format!("tool-{n}"),"type":"commandExecution","command":"cargo test --workspace","aggregatedOutput":format!("{}\ntest result: ok", "synthetic test ... ok\n".repeat((n % 16 + 1) as usize)),"status":"completed"},
            {"id":format!("answer-{n}"),"type":"agentMessage","text":format!("## Change {n}\n\n{paragraphs}```rust\nfn restore_{n}(session: &Session) {{\n    session.restore_draft();\n}}\n```\n\n- Keep the anchor stable\n- Ignore stale replies\n\n> A short review note.")}
        ]})
    }).collect();
    live.app.sessions[0].conversation = Default::default();
    live.app.sessions[0]
        .conversation
        .history(Provider::Codex, &turns, false);

    for n in (0..1_000).step_by(3) {
        live.expanded.insert((0, format!("codex-tool-{n}")));
    }
    live.sync_editors();
    state
}

pub(crate) fn background(live: &mut Live) -> bool {
    live.apply_events((1..6).map(|i| Event::Codex(caelum_protocol::CodexEvent {
        project_path: "/evaluation".into(),
        method: "item/agentMessage/delta".into(),
        params: json!({"threadId":format!("thread-{i}"),"itemId":"scroll-background","delta":" synthetic delta", "_created":1000}),
        request_id: None,
    })))
}

pub(super) fn label(step: u64) -> &'static str {
    if step < STEPS / 2 {
        "scroll_up"
    } else {
        "scroll_down"
    }
}

/// Keep a coalesced batch's deltas instead of losing all but its last request.
/// Once drawn, use the observed offset again to respect virtual-height anchoring.
#[derive(Default)]
pub(super) struct Observation {
    pending_y: Cell<Option<f32>>,
    step: Cell<Option<u64>>,
}

impl Observation {
    fn request(&self, live: &Live, tx: &mut rune::Tx, step: u64) {
        let position = live.reading_position(0);
        let delta = if step < STEPS / 2 {
            -DISTANCE
        } else {
            DISTANCE
        };
        let y = (self.pending_y.get().unwrap_or(position.offset[1]) + delta)
            .clamp(0.0, position.max[1]);
        self.pending_y.set(Some(y));
        self.step.set(Some(step));
        let mark = live.latency.begin(label(step));
        live.latency.track(mark, Some(0), false);
        tx.scroll_to_y("live-transcript-0", y);
    }

    pub(super) fn drawn(&self, live: &Live, commits: u64) {
        self.pending_y.set(None);
        if let Some(step) = self.step.get()
            && let Some(mark) = diagnostics::next("scroll", "position", 0)
        {
            mark.record("step", step);
            mark.record(
                "offset_mpt",
                (live.reading_position(0).offset[1] * 1000.0) as u64,
            );
            mark.record("commits", commits);
        }
    }
}

pub(super) fn run(handle: rune::AppHandle<Workspace>, streaming: bool) {
    std::thread::spawn(move || {
        let start = Instant::now();
        for tick in 1..=2_280 {
            let deadline = start + Duration::from_secs_f64(tick as f64 / 120.0);
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
            let step = (240..2_160).contains(&tick).then(|| tick - 240);
            let stream = streaming && tick % 6 == 0;
            if step.is_none() && !stream {
                continue;
            }
            handle.update_if(move |state, tx| {
                let live = crate::live::live_mut(state);
                let changed = stream && background(live);
                if stream && let Some(mark) = diagnostics::mark("workload", "stream", tick, 0) {
                    mark.record("applied", 5);
                }
                if let Some(step) = step {
                    live.latency.scroll.request(live, tx, step);
                    if let Some(mark) = diagnostics::mark("workload", "scroll", step, 0) {
                        mark.record("applied", 1);
                    }
                }
                changed
            });
        }
        handle.quit();
    });
}
