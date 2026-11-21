use super::*;
use caelum_core::application::{Command, Event, Provider, Task};
use serde_json::json;

fn history() -> Runtime<Workspace> {
    let mut rt = live_runtime_size((800.0, 620.0));
    rt.update(|s, _| {
        let live = s.live.as_mut().unwrap();
        live.fixture = true;
        let session = &mut live.app.sessions[0];
        session.scope.provider = Provider::OpenCode;
        let messages: Vec<_> = (0..60).map(|n| json!({
            "id":format!("prompt-{n}"),"type":"user","text":format!("Prompt {n}: preserve the reading position.")
        })).collect();
        session.conversation.history(Provider::OpenCode, &messages, false);
    });
    advance(&mut rt, 0.6);
    rt
}

#[test]
fn scrolling_within_mounted_rows_updates_the_timeline_without_rebuilding_the_view() {
    let mut rt = history();
    let prompt = rt.state.live.as_ref().unwrap().current_prompt.clone();
    let commits = rt.stats.commits;
    let [left, top, _, height] = element(&rt, "live-transcript-0").rect;
    rt.wheel(left + 2.0, top + height / 2.0, 120.0);
    advance(&mut rt, 0.1);
    assert_eq!(rt.scroll_at_bottom("live-transcript-0"), Some(false));
    assert_ne!(rt.state.live.as_ref().unwrap().current_prompt, prompt);
    assert_eq!(
        rt.stats.commits, commits,
        "bookkeeping alone is not a view change"
    );
    let current = rt.state.live.as_ref().unwrap().current_prompt.clone();
    fill(&mut rt, "live-draft-0", "/timeline");
    key(&mut rt, Key::Enter, Mods::default());
    assert_eq!(
        rt.state.live.as_ref().unwrap().commands.timeline_current,
        current
    );
}

#[test]
fn scrolling_still_loads_patch_previews_and_image_request_state() {
    let mut rt = history();
    rt.update(|s, _| {
        let session = &mut s.live.as_mut().unwrap().app.sessions[0];
        session.conversation.history(Provider::OpenCode, &[
            json!({"id":"work","type":"assistant","content":[
                {"id":"lazy","type":"tool","name":"apply_patch","state":{"status":"completed","input":{"path":"src/example.rs","patch":"@@ -1 +1 @@\n-old();\n+new();"},"content":[{"type":"text","text":"Updated example"}]}}
            ]}),
            json!({"id":"image","type":"user","text":"Inspect the reference","files":[{"mime":"image/png","uri":"https://example.test/reference.png","name":"Reference"}]}),
        ], true);
    });
    advance(&mut rt, 0.5);
    assert!(
        !rt.state
            .live
            .as_ref()
            .unwrap()
            .patches
            .contains_key(&(0, "opencode-tool-lazy".into()))
    );
    rt.update_if(|_, tx| {
        tx.scroll_center_in_instant("live-rows-0", "live-entry-0-opencode-tool-lazy");
        false
    });
    advance(&mut rt, 0.3);
    let preview = element(&rt, "patch-preview-0-opencode-tool-lazy");
    assert!(preview.visible[3] > 0.0);
    assert_eq!(
        rt.state.live.as_ref().unwrap().patches[&(0, "opencode-tool-lazy".into())]
            .diff
            .files()[0]
            .path(),
        "src/example.rs"
    );

    rt.update_if(|_, tx| {
        tx.scroll_center_in_instant("live-rows-0", "live-entry-0-opencode-image");
        false
    });
    advance(&mut rt, 0.3);
    let image = rt
        .elements()
        .into_iter()
        .find(|e| !e.exiting && e.role == "button" && e.label.as_deref() == Some("Loading image…"))
        .unwrap();
    assert!(
        !image.focusable,
        "queueing a visible image must disable its button"
    );
    let commands = rt.state.live.as_mut().unwrap().app.commands();
    let images: Vec<_> = commands
        .iter()
        .filter(|(_, c)| matches!(c, Command::Task(Task::Image { .. })))
        .collect();
    assert_eq!(images.len(), 1, "the visible source is requested once");
    let request = images[0].0;
    rt.update(|s, _| {
        s.live
            .as_mut()
            .unwrap()
            .apply(Event::Completed(request, Err("Image unavailable".into())));
    });
    advance(&mut rt, 0.3);
    assert!(rt.elements().iter().any(|e| !e.exiting
        && e.role == "button"
        && e.label.as_deref() == Some("Retry image")
        && e.focusable));
    let position = rt.state.live.as_ref().unwrap().reading_position(0).offset[1];
    rt.update(|s, _| s.live.as_mut().unwrap().app.select(1));
    advance(&mut rt, 0.3);
    rt.update(|s, _| s.live.as_mut().unwrap().app.select(0));
    advance(&mut rt, 0.3);
    assert!((rt.state.live.as_ref().unwrap().reading_position(0).offset[1] - position).abs() < 1.0);
}
