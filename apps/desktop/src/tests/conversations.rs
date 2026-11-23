use super::*;
use crate::{
    live::{live, live_mut},
    panels::Screen,
};
use caelum_core::application::{Command, Entry, EntryKind, Event};

fn overview_shortcut(rt: &mut Runtime<Workspace>) {
    key(
        rt,
        Key::Command('j'),
        Mods {
            cmd: true,
            ..Default::default()
        },
    );
    advance(rt, 0.4);
}

fn tile_shows(rt: &Runtime<Workspace>, key: &str, content: &str) -> bool {
    let path = element(rt, key).path;
    rt.elements().iter().any(|e| {
        !e.exiting
            && e.path.starts_with(&path)
            && e.text.as_deref().is_some_and(|t| t.contains(content))
    })
}

#[test]
fn overview_preserves_the_reading_place_and_native_draft_through_escape_and_switches() {
    let mut rt = design_runtime("session-feed", (800.0, 620.0));
    rt.update(|s, _| {
        let sessions = &mut live_mut(s).app.sessions;
        sessions[1].conversation.set_working(true);
        let session = &mut sessions[0];
        session.conversation.entries = (0..60)
            .map(|i| {
                Entry::new(
                    format!("note-{i}"),
                    EntryKind::Assistant,
                    format!("Notebook entry {i}"),
                    true,
                )
            })
            .collect();
        session.conversation.set_working(true);
    });
    advance(&mut rt, 0.4);
    fill(&mut rt, "live-draft-0", "Keep this café draft");
    rt.update(|s, tx| {
        s.live.as_mut().unwrap().drafts[0].set_selection(5, 9);
        tx.scroll_center_in_instant("live-rows-0", "live-entry-0-note-25");
    });
    advance(&mut rt, 0.5);
    let position = live(&rt.state).reading_position(0).offset[1];
    assert!(position > 100.0);
    let reading_line = "live-entry-0-note-25";
    let before_open = element(&rt, reading_line).rect[1];
    overview_shortcut(&mut rt);
    assert!(element(&rt, "overview-search").focused);
    rt.paste("not here");
    advance(&mut rt, 0.15);
    assert!(rt.find("overview-session-0").is_none());
    key(&mut rt, Key::Escape, Mods::default());
    advance(&mut rt, 0.35);
    assert!(live(&rt.state).panels.overlay().is_none());
    assert_eq!(live(&rt.state).drafts[0].selected(), "this");
    assert!(!live(&rt.state).app.sessions[0].stopping());
    assert!(element(&rt, "live-draft-0").focused);
    assert!((element(&rt, reading_line).rect[1] - before_open).abs() < 2.0);

    overview_shortcut(&mut rt);
    click(&mut rt, "overview-session-1");
    assert_eq!(live(&rt.state).app.selected, Some(1));
    overview_shortcut(&mut rt);
    click(&mut rt, "overview-session-0");
    advance(&mut rt, 0.5);
    assert!(
        rt.find(reading_line).is_some(),
        "lost the visible message at {position}; now {:?}",
        live(&rt.state).reading_position(0)
    );
    assert!(
        (element(&rt, reading_line).rect[1] - before_open).abs() < 2.0,
        "expected y {before_open}, got {:?}",
        element(&rt, reading_line).rect
    );
    let l = rt.state.live.as_mut().unwrap();
    assert_eq!(l.app.selected, Some(0));
    assert_eq!(l.drafts[0].text, "Keep this café draft");
    assert_eq!(l.drafts[0].selected(), "this");
    assert!(!l.app.commands().iter().any(|(_, c)| matches!(
        c,
        Command::Start(..) | Command::Send(..) | Command::Cancel(_)
    )));
}

#[test]
fn overview_updates_the_question_in_place_and_opens_the_tile_chosen_with_arrows() {
    for (size, down) in [((800.0, 620.0), 4), ((1280.0, 1000.0), 5)] {
        let mut rt = design_runtime("conversation-overview", size);
        let tile = "overview-session-1";
        let rect = element(&rt, tile).rect;
        assert!((element(&rt, "overview-session-5").rect[2] - rect[2]).abs() <= 1.0);
        assert!(tile_shows(&rt, tile, "Should a manual disconnect"));
        assert!(tile_shows(
            &rt,
            "overview-session-3",
            "tracing where the editor loses focus"
        ));
        assert!(tile_shows(&rt, "overview-session-3", "Running · 3m"));
        assert!(rt.find("overview-session-0").is_none());
        assert!(rt.find("overview-session-2").is_none());
        assert!(tile_shows(
            &rt,
            "overview-session-4",
            "Allow the agent to start the preview"
        ));
        assert!(tile_shows(
            &rt,
            "overview-session-5",
            "Compare the cold and incremental"
        ));
        rt.update(|s, _| {
            let l = s.live.as_mut().unwrap();
            l.app.sessions[1].last_activity = Some(u64::MAX);
            l.app.sessions[1].conversation.set_working(true);
            l.drafts[1].replace_text("Return to this draft");
            let mut wrong_project = l.app.questions[0].clone();
            wrong_project.project_path = "/projects/rune".into();
            l.apply(Event::Questions(vec![wrong_project]));
        });
        advance(&mut rt, 0.25);
        assert_eq!(element(&rt, tile).rect, rect);
        assert!(tile_shows(&rt, tile, "Running"));
        assert!(!tile_shows(&rt, tile, "Return to this draft"));
        key(&mut rt, Key::Down, Mods::default());
        key(&mut rt, Key::Enter, Mods::default());
        advance(&mut rt, 0.3);
        assert_eq!(live(&rt.state).app.selected, Some(down));
        assert_eq!(live(&rt.state).panels.screen, Screen::Conversation);
    }
}

#[test]
fn overview_streams_visible_excerpts_and_keeps_filtered_updates_for_when_the_card_returns() {
    let mut rt = design_runtime("conversation-overview", (800.0, 620.0));
    let card = "overview-session-3";
    let delta = |id: &str, text: &str| {
        Event::Codex(caelum_protocol::CodexEvent {
            project_path: "/projects/rune".into(),
            method: "item/agentMessage/delta".into(),
            params: serde_json::json!({"threadId":"fixture-3","itemId":id,"delta":text}),
            request_id: None,
        })
    };
    rt.update(|s, _| assert!(live_mut(s).apply_events([delta("step", "Checking the editor")])));
    advance(&mut rt, 0.1);
    rt.update(|s, _| assert!(live_mut(s).apply_events([delta("step", " after sleep.")])));
    advance(&mut rt, 0.1);
    assert!(tile_shows(&rt, card, "Checking the editor after sleep."));
    rt.update(|s, _| {
        let l = live_mut(s);
        assert!(l.apply_events([delta("step", &" progress".repeat(40))]));
        assert!(!l.apply_events([delta("step", " beyond the excerpt")]));
    });
    fill(&mut rt, "overview-search", "Reconnect");
    assert!(rt.find(card).is_none());
    rt.update(|s, _| {
        assert!(!live_mut(s).apply_events([delta("final-step", "Found the focus change.")]))
    });
    fill(&mut rt, "overview-search", "keyboard focus");
    assert!(tile_shows(&rt, card, "Found the focus change."));
    let now = live(&rt.state).feed.displayed_at();
    rt.update(|s, tx| {
        let l = live_mut(s);
        assert!(!l.panels.tick_overview(now + 59_999, tx));
        l.feed.set_display_clock(now + 60_000);
        assert!(l.panels.tick_overview(now + 60_000, tx));
    });
    advance(&mut rt, 0.1);
    assert!(tile_shows(&rt, card, "Running · 4m"));
    fill(&mut rt, "overview-search", "absent");
    rt.update(|s, tx| assert!(!live_mut(s).panels.tick_overview(now + 120_000, tx)));
}

#[test]
fn overview_adds_and_removes_background_sessions_live_under_the_current_search() {
    let mut rt = design_runtime("session-feed", (800.0, 620.0));
    overview_shortcut(&mut rt);
    assert!(tile_shows(
        &rt,
        "conversation-overview",
        "No active conversations"
    ));
    fill(&mut rt, "overview-search", "keyboard focus");
    assert!(tile_shows(
        &rt,
        "conversation-overview",
        "No matching active conversations"
    ));
    let event = |method: &str| {
        Event::Codex(caelum_protocol::CodexEvent {
            project_path: "/projects/rune".into(),
            method: method.into(),
            params: serde_json::json!({"threadId":"fixture-3", "turn":{"id":"turn"}}),
            request_id: None,
        })
    };
    rt.update(|s, _| assert!(live_mut(s).apply_events([event("turn/started")])));
    advance(&mut rt, 0.25);
    assert!(tile_shows(&rt, "overview-session-3", "Running"));
    rt.update(|s, _| assert!(live_mut(s).apply_events([event("turn/completed")])));
    advance(&mut rt, 0.25);
    assert!(rt.find("overview-session-3").is_none());
    assert!(tile_shows(
        &rt,
        "conversation-overview",
        "No matching active conversations"
    ));
    key(&mut rt, Key::Enter, Mods::default());
    assert!(element(&rt, "overview-search").focused);
    assert_eq!(live(&rt.state).app.selected, Some(0));
}

#[test]
fn overview_windows_large_projects_and_finds_sessions_outside_the_sidebar_filter() {
    let mut rt = design_runtime("session-feed-many", (800.0, 620.0));
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        l.app.preferences.session_scope = caelum_core::application::SessionScope::CurrentProject;
        l.panels.sessions.replace_text("no match");
        for session in l.app.sessions.iter_mut().filter(|s| s.title.ends_with('9')) {
            session.conversation.set_working(true);
        }
        assert_eq!(l.app.overview_sessions("").len(), 200);
    });
    click(&mut rt, "open-conversations");
    advance(&mut rt, 0.4);
    let mounted = rt
        .elements()
        .into_iter()
        .filter(|e| {
            !e.exiting
                && e.key
                    .as_deref()
                    .is_some_and(|k| k.starts_with("overview-session-"))
        })
        .count();
    assert!((2..64).contains(&mounted), "{mounted} mounted");
    fill(&mut rt, "overview-search", "Historical session 1998");
    assert!(tile_shows(
        &rt,
        "conversation-overview",
        "No matching active conversations"
    ));
    fill(&mut rt, "overview-search", "Historical session 1999");
    key(&mut rt, Key::Enter, Mods::default());
    advance(&mut rt, 0.3);
    assert_eq!(rt.state.title(), "Historical session 1999");
    assert_eq!(live(&rt.state).panels.sessions.text, "no match");
    overview_shortcut(&mut rt);
    let selected = live(&rt.state).app.selected.unwrap();
    let tile = element(&rt, &format!("overview-session-{selected}"));
    assert!(tile.visible[3] >= tile.rect[3] - 1.0, "{tile:?}");
}

#[test]
fn overview_opens_the_scoped_vm_thread_and_preserves_its_draft() {
    let mut rt = design_runtime("", (800.0, 620.0));
    rt.update(|s, _| {
        crate::remote::scene(s, "remote-working");
        live_mut(s).app.sessions[0].conversation.set_working(true);
    });
    advance(&mut rt, 0.3);
    fill(
        &mut rt,
        "remote-draft-checkout-example",
        "Keep this VM draft",
    );
    overview_shortcut(&mut rt);
    let target = rt
        .elements()
        .into_iter()
        .find(|e| {
            !e.exiting
                && e.role == "button"
                && e.path.contains("conversation-overview")
                && e.label.as_deref() == Some("Add a health check and show me the changes.")
        })
        .expect("scoped VM tile");
    let vm = target.key.unwrap();
    assert!(tile_shows(&rt, &vm, "dev-01"));
    assert!(tile_shows(&rt, &vm, "Running cargo"));
    click(&mut rt, "overview-session-0");
    assert_eq!(live(&rt.state).panels.screen, Screen::Conversation);
    overview_shortcut(&mut rt);
    click(&mut rt, &vm);
    let l = live(&rt.state);
    assert_eq!(l.panels.screen, Screen::Remote);
    assert_eq!(
        l.app.remote.saved.selected.as_deref(),
        Some("checkout-example")
    );
    assert_eq!(
        element(&rt, "remote-draft-checkout-example")
            .text
            .as_deref(),
        Some("Keep this VM draft")
    );
    overview_shortcut(&mut rt);
    rt.update(|s, _| {
        let c = live_mut(s)
            .app
            .remote
            .saved
            .checkouts
            .get_mut("checkout-example")
            .unwrap();
        c.thread.turn = caelum_core::remote::Turn::Idle;
    });
    advance(&mut rt, 0.25);
    assert!(rt.find(&vm).is_none());
}

#[test]
fn request_shortcut_leads_to_the_actual_question_and_the_title_browses_prompts() {
    let mut rt = design_runtime("conversation-overview", (800., 620.));
    click(&mut rt, "overview-requests");
    key(&mut rt, Key::Enter, Mods::default());
    advance(&mut rt, 0.3);
    assert_eq!(live(&rt.state).app.selected, Some(1));
    assert!(rt.find("question-submit-retry-choice").is_some());
    reveal(&mut rt, "live-session-0");
    click(&mut rt, "live-session-0");
    assert_eq!(live(&rt.state).app.selected, Some(0));
    let before = live(&rt.state).drafts[0].text.clone();
    click(&mut rt, "conversation-heading");
    assert_eq!(live(&rt.state).panels.overlay(), Some(Screen::Timeline));
    assert_eq!(live(&rt.state).commands.owner, Some(0));
    assert_eq!(live(&rt.state).drafts[0].text, before);
}
