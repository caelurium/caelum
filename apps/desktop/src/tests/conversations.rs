use super::*;
use crate::{live::live, panels::Screen};
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
        let session = &mut s.live.as_mut().unwrap().app.sessions[0];
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
    for (size, down) in [((800.0, 620.0), 2), ((1280.0, 1000.0), 3)] {
        let mut rt = design_runtime("conversation-overview", size);
        let tile = "overview-session-1";
        let rect = element(&rt, tile).rect;
        assert!(tile_shows(&rt, tile, "Should a manual disconnect"));
        rt.update(|s, _| {
            let l = s.live.as_mut().unwrap();
            l.app.sessions[1].last_activity = Some(u64::MAX);
            l.drafts[1].replace_text("Return to this draft");
            let mut wrong_project = l.app.questions[0].clone();
            wrong_project.project_path = "/projects/rune".into();
            l.apply(Event::Questions(vec![wrong_project]));
        });
        advance(&mut rt, 0.25);
        assert_eq!(element(&rt, tile).rect, rect);
        assert!(tile_shows(&rt, tile, "Unsent draft"));
        assert!(tile_shows(&rt, tile, "Return to this draft"));
        key(&mut rt, Key::Down, Mods::default());
        key(&mut rt, Key::Enter, Mods::default());
        advance(&mut rt, 0.3);
        assert_eq!(live(&rt.state).app.selected, Some(down));
        assert_eq!(live(&rt.state).panels.screen, Screen::Conversation);
    }
}

#[test]
fn overview_windows_large_projects_and_finds_sessions_outside_the_sidebar_filter() {
    let mut rt = design_runtime("session-feed-many", (800.0, 620.0));
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        l.feed.project = Some("/projects/caelum".into());
        l.panels.sessions.replace_text("no match");
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
        crate::remote::scene(s, "remote-machine");
        s.live.as_mut().unwrap().panels.set_overlay(None);
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
    assert!(tile_shows(&rt, &vm, "Keep this VM draft"));
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
}

#[test]
fn request_shortcut_leads_to_the_actual_question_and_the_title_browses_prompts() {
    let mut rt = design_runtime("conversation-overview", (800., 620.));
    click(&mut rt, "overview-requests");
    key(&mut rt, Key::Enter, Mods::default());
    advance(&mut rt, 0.3);
    assert_eq!(live(&rt.state).app.selected, Some(1));
    assert!(rt.find("question-submit-retry-choice").is_some());
    click(&mut rt, "live-session-0");
    let before = live(&rt.state).drafts[0].text.clone();
    click(&mut rt, "conversation-heading");
    assert_eq!(live(&rt.state).panels.overlay(), Some(Screen::Timeline));
    assert_eq!(live(&rt.state).commands.owner, Some(0));
    assert_eq!(live(&rt.state).drafts[0].text, before);
}
