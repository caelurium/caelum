use crate::{Workspace, live::live_mut};
use caelum_core::application::{Provider, Scope};
use std::time::Duration;

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
    live.latency.replay = true;
    live.sync_editors();
    state
}

pub(super) fn run(handle: rune::AppHandle<Workspace>) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(2));
        // Native app updates run on the UI thread. This deliberately measures the
        // application-handler boundary, not OS input delivery or physical scanout.
        for step in 0..160 {
            handle.update(move |state, _| {
                let live = live_mut(state);
                if step % 2 == 0 {
                    let mark = live.latency.begin("navigate");
                    let index = (step / 2 + 1) % live.app.sessions.len();
                    live.app.select(index);
                    live.latency.track(mark, Some(index), true);
                } else {
                    let mark = live.latency.begin("edit");
                    let index = live.app.selected.unwrap();
                    live.drafts[index].insert("Synthetic input. ");
                    live.latency.track(mark, Some(index), false);
                }
            });
            std::thread::sleep(Duration::from_millis(100));
        }
        std::thread::sleep(Duration::from_secs(1));
        handle.quit();
    });
}
