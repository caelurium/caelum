use crate::{Workspace, live::live_mut};
use caelum_core::application::{Provider, Scope};
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub(super) enum Scenario {
    Small,
    Streaming,
}

pub fn workspace(streaming: bool) -> Workspace {
    let mut state = if streaming {
        crate::evaluation::workspace()
    } else {
        let mut state = Workspace::live(None);
        let live = state.live.as_mut().unwrap();
        live.fixture = true;
        live.app.projects.push(caelum_protocol::ProjectInfo {
            name: "Latency workload".into(),
            path: "/evaluation".into(),
        });
        for _ in 0..2 {
            let index = live.app.new_session(Scope {
                project: "/evaluation".into(),
                provider: Provider::Codex,
            });
            let messages: Vec<_> = (0..50).map(|n| serde_json::json!({
                "id":format!("turn-{n}"),"status":"completed","items":[
                    {"id":format!("user-{n}"),"type":"userMessage","content":[{"text":"Review this synthetic change."}]},
                    {"id":format!("answer-{n}"),"type":"agentMessage","text":"Keep **session ownership** and draft state intact."}
                ]
            })).collect();
            live.app.sessions[index]
                .conversation
                .history(Provider::Codex, &messages, false);
        }
        live.app.select(0);
        state
    };
    let live = state.live.as_mut().unwrap();
    // Keep the demanding fixture, but drive its deltas from fixed deadlines,
    // independently of the frame/poll rate, so faster rendering cannot add load.
    live.evaluation = false;
    live.fixture = true;
    live.latency
        .replay
        .set(super::Replay::Pending(if streaming {
            Scenario::Streaming
        } else {
            Scenario::Small
        }));
    live.sync_editors();
    state
}

pub(super) fn run(handle: rune::AppHandle<Workspace>, scenario: Scenario) {
    std::thread::spawn(move || {
        let start = Instant::now();
        // Native app updates run on the UI thread. This deliberately measures the
        // application-handler boundary, not OS input delivery or physical scanout.
        for tick in 1..=380 {
            let deadline = start + Duration::from_millis(tick * 50);
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
            let step = ((40..360).contains(&tick) && tick % 2 == 0).then(|| (tick - 40) / 2);
            if matches!(scenario, Scenario::Small) && step.is_none() {
                continue;
            }
            handle.update(move |state, _| {
                let live = live_mut(state);
                if matches!(scenario, Scenario::Streaming) {
                    crate::evaluation::tick(&mut live.app);
                    if let Some(mark) =
                        caelum_core::diagnostics::mark("workload", "stream", tick, 0)
                    {
                        mark.record("applied", 6);
                    }
                }
                let Some(step) = step else { return };
                if step % 2 == 0 {
                    let mark = live.latency.begin("navigate");
                    let index = (step as usize / 2 + 1) % live.app.sessions.len();
                    live.app.select(index);
                    live.latency.track(mark, Some(index), true);
                } else {
                    let mark = live.latency.begin("edit");
                    let index = live.app.selected.unwrap();
                    live.drafts[index].insert("Synthetic input. ");
                    live.latency.track(mark, Some(index), false);
                }
            });
        }
        handle.quit();
    });
}
