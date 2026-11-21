use super::*;
use caelum_core::application::{EntryKind, Provider};
use serde_json::json;

fn row(index: usize, id: &str) -> String {
    format!("live-entry-{index}-opencode-{id}")
}

fn shown(rt: &Runtime<Workspace>, index: usize, id: &str) -> bool {
    rt.elements()
        .iter()
        .any(|e| !e.exiting && e.key.as_deref() == Some(row(index, id).as_str()))
}

#[test]
fn transcript_keys_follow_same_length_replacements_reorders_and_visibility() {
    let mut rt = live_runtime_size((800.0, 620.0));
    rt.update(|s, _| {
        s.live.as_mut().unwrap().app.sessions[0].conversation.history(
            Provider::OpenCode,
            &[
                json!({"id":"a","type":"synthetic","text":"First row"}),
                json!({"id":"b","type":"synthetic","text":"Second row"}),
                json!({"id":"hidden","type":"synthetic","text":"<system-reminder>Hidden</system-reminder>"}),
            ],
            false,
        );
    });
    advance(&mut rt, 0.3);
    assert!(shown(&rt, 0, "a") && shown(&rt, 0, "b"));
    assert!(!shown(&rt, 0, "hidden"));

    rt.update(|s, tx| {
        let entries = &mut s.live.as_mut().unwrap().app.sessions[0]
            .conversation
            .entries;
        entries[0].id = "opencode-replacement".into();
        entries[0].text = "Replacement row".into();
        tx.instant();
    });
    advance(&mut rt, 0.3);
    assert!(!shown(&rt, 0, "a"));
    assert!(shown(&rt, 0, "replacement"));
    assert!(rt.elements().iter().any(|e| {
        !e.exiting
            && e.path.contains(&row(0, "replacement"))
            && e.text.as_deref() == Some("Replacement row")
    }));

    rt.update(|s, tx| {
        s.live.as_mut().unwrap().app.sessions[0]
            .conversation
            .entries
            .swap(0, 1);
        tx.instant();
    });
    advance(&mut rt, 0.3);
    assert!(element(&rt, &row(0, "b")).rect[1] < element(&rt, &row(0, "replacement")).rect[1]);

    rt.update(|s, tx| {
        let entries = &mut s.live.as_mut().unwrap().app.sessions[0]
            .conversation
            .entries;
        entries[0].text = "<system-reminder>Now hidden</system-reminder>".into();
        entries[2].text = "Now visible".into();
        tx.instant();
    });
    advance(&mut rt, 0.3);
    assert!(!shown(&rt, 0, "b"));
    assert!(shown(&rt, 0, "replacement") && shown(&rt, 0, "hidden"));

    rt.update(|s, _| {
        let entry = &mut s.live.as_mut().unwrap().app.sessions[0]
            .conversation
            .entries[2];
        *entry = caelum_core::application::Entry::new(
            entry.id.clone(),
            EntryKind::Reasoning,
            String::new(),
            false,
        );
    });
    advance(&mut rt, 0.3);
    assert!(shown(&rt, 0, "hidden"));
    rt.update(|s, _| {
        let entry = &mut s.live.as_mut().unwrap().app.sessions[0]
            .conversation
            .entries[2];
        *entry = caelum_core::application::Entry::new(
            entry.id.clone(),
            EntryKind::Reasoning,
            String::new(),
            true,
        );
    });
    advance(&mut rt, 0.3);
    assert!(!shown(&rt, 0, "hidden"));
}

#[test]
fn transcript_keys_keep_updates_pagination_and_navigation_with_their_owner() {
    let mut rt = live_runtime_size((800.0, 620.0));
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    rt.update(|s, _| {
        for (index, session) in s.live.as_mut().unwrap().app.sessions.iter_mut().enumerate() {
            let messages: Vec<_> = (0..60)
                .map(|i| json!({"id":format!("answer-{i}"),"type":"assistant","finish":"stop","content":[{"type":"text","text":format!("Session {index}, answer {i}")}]}))
                .collect();
            session.conversation.history(Provider::OpenCode, &messages, false);
        }
    });
    fill(&mut rt, "live-draft-0", "Keep this draft");
    rt.update(|_, tx| tx.scroll_center_in_instant("live-rows-0", row(0, "answer-25")));
    advance(&mut rt, 0.5);
    assert_eq!(rt.scroll_at_bottom("live-transcript-0"), Some(false));
    assert!(shown(&rt, 0, "answer-25"), "initial jump");
    let before = element(&rt, &row(0, "answer-25")).rect[1];

    rt.update(|s, _| {
        s.live.as_mut().unwrap().app.sessions[0]
            .conversation
            .entries[25]
            .text = "Updated answer".into();
    });
    advance(&mut rt, 0.3);
    assert!(shown(&rt, 0, "answer-25"), "text update");
    click(&mut rt, "copy-response-0-opencode-answer-25");
    assert_eq!(
        clipboard.text().as_deref().map(String::as_str),
        Some("Updated answer")
    );
    assert!((element(&rt, &row(0, "answer-25")).rect[1] - before).abs() < 1.0);

    rt.update(|s, _| {
        s.live.as_mut().unwrap().app.sessions[0].conversation.history(
            Provider::OpenCode,
            &[json!({"id":"hidden","type":"synthetic","text":"<system-reminder>Hidden</system-reminder>"})],
            true,
        );
    });
    advance(&mut rt, 0.3);
    assert!(shown(&rt, 0, "answer-25"), "hidden prepend");
    assert!((element(&rt, &row(0, "answer-25")).rect[1] - before).abs() < 1.0);
    click(&mut rt, "copy-response-0-opencode-answer-25");
    assert_eq!(
        clipboard.text().as_deref().map(String::as_str),
        Some("Updated answer")
    );

    rt.update(|s, _| {
        let older: Vec<_> = (0..10)
            .map(|i| json!({"id":format!("older-{i}"),"type":"user","text":format!("Earlier prompt {i}")}))
            .collect();
        s.live.as_mut().unwrap().app.sessions[0].conversation.history(Provider::OpenCode, &older, true);
    });
    advance(&mut rt, 0.5);
    assert!(shown(&rt, 0, "answer-25"), "visible prepend");
    assert!((element(&rt, &row(0, "answer-25")).rect[1] - before).abs() < 1.0);
    assert_eq!(rt.scroll_at_bottom("live-transcript-0"), Some(false));

    click(&mut rt, "live-session-1");
    rt.update(|_, tx| tx.scroll_center_in_instant("live-rows-1", row(1, "answer-25")));
    advance(&mut rt, 0.5);
    click(&mut rt, "copy-response-1-opencode-answer-25");
    assert_eq!(
        clipboard.text().as_deref().map(String::as_str),
        Some("Session 1, answer 25")
    );
    click(&mut rt, "live-session-0");
    rt.update(|_, tx| tx.scroll_center_in_instant("live-rows-0", row(0, "answer-25")));
    advance(&mut rt, 0.5);
    assert!(shown(&rt, 0, "answer-25"), "return to original session");
    click(&mut rt, "copy-response-0-opencode-answer-25");
    assert_eq!(
        clipboard.text().as_deref().map(String::as_str),
        Some("Updated answer")
    );
    assert_eq!(
        rt.state.live.as_ref().unwrap().drafts[0].text,
        "Keep this draft"
    );
}
