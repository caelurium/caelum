use super::*;
use caelum_core::application::{
    Command, Event, ExportFormat, ForkBoundary, Reply, SessionKey, Summary,
};
use serde_json::json;

#[test]
fn rename_dialog_and_export_save_panel_keep_their_captured_session() {
    let mut rt = live_runtime();
    rt.update(|s, _| s.live.as_mut().unwrap().app.sessions[0].remote_id = Some("parent".into()));
    fill(&mut rt, "live-draft-0", "preserve this draft");
    fill(&mut rt, "live-draft-0", "/rename");
    key(&mut rt, Key::Enter, Mods::default());
    advance(&mut rt, 0.1);
    fill(&mut rt, "rename-title", "My café session");
    key(&mut rt, Key::Enter, Mods::default());
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        let (id, owner) = l
            .app
            .commands()
            .into_iter()
            .find_map(|(id, c)| match c {
                Command::Rename(key, title) if title == "My café session" => Some((id, key)),
                _ => None,
            })
            .unwrap();
        assert_eq!(owner.id, "parent");
        l.apply(Event::Completed(id, Ok(Reply::Done)));
        assert_eq!(l.drafts[0].text, "preserve this draft");
        assert_eq!(l.app.sessions[0].title, "My café session");
    });
    fill(&mut rt, "live-draft-0", "/export json");
    key(&mut rt, Key::Enter, Mods::default());
    assert!(rt.file_dialog().is_some());
    rt.update(|s, _| s.live.as_mut().unwrap().app.select(1));
    rt.resolve_file_dialog(Some("/example/conversation.json".into()))
        .unwrap();
    advance(&mut rt, 0.1);
    let l = rt.state.live.as_mut().unwrap();
    assert!(l.app.commands().iter().any(|(_,c)|matches!(c,Command::Export(owner,title,ExportFormat::Json,path) if owner.id=="parent" && title=="My café session" && path==std::path::Path::new("/example/conversation.json"))));
    assert_eq!(l.app.selected, Some(1));
    assert_eq!(l.drafts[0].text, "preserve this draft");
}

#[test]
fn edit_opens_a_prefilled_independent_composer_and_rewind_requires_a_separate_confirmation() {
    let mut rt = live_runtime_size((800., 620.));
    rt.update(|s, _| s.live.as_mut().unwrap().app.sessions[0].remote_id = Some("parent".into()));
    for command in ["/rewind", "/edit"] {
        fill(&mut rt, "live-draft-0", command);
        key(&mut rt, Key::Enter, Mods::default());
        rt.update(|s,_|{
            let l=s.live.as_mut().unwrap();
            let page=l.app.commands().into_iter().find(|(_,c)|matches!(c,Command::CodexControlHistory(k,_) if k.id=="parent")).unwrap().0;
            l.apply(Event::Completed(page,Ok(Reply::History(caelum_protocol::SessionHistoryPage{
                data:vec![json!({"id":"turn","status":"completed","items":[{"id":"user","type":"userMessage","content":[{"type":"text","text":"Original prompt"}]}]})],next_cursor:None,is_working:Some(false),agent:None,model:None,effort:None
            }))));
        });
        advance(&mut rt, 0.1);
        key(&mut rt, Key::Enter, Mods::default());
        advance(&mut rt, 0.1);
        if command == "/rewind" {
            assert!(rt.find("rewind-history").is_some());
            assert!(rt.find("rewind-files").is_none());
            assert!(
                rt.state
                    .live
                    .as_mut()
                    .unwrap()
                    .app
                    .commands()
                    .iter()
                    .all(|(_, c)| !matches!(c, Command::Rewind { .. }))
            );
            key(&mut rt, Key::Escape, Mods::default());
        } else {
            rt.update(|s, _| {
                let l = s.live.as_mut().unwrap();
                let (id, source, text, attachments) = l
                    .app
                    .commands()
                    .into_iter()
                    .find_map(|(id, c)| match c {
                        Command::EditBranch(k, ForkBoundary::CodexBefore(turn), text, a)
                            if turn == "turn" =>
                        {
                            Some((id, k, text, a))
                        }
                        _ => None,
                    })
                    .unwrap();
                let child = Summary {
                    key: SessionKey {
                        scope: source.scope.clone(),
                        id: "child".into(),
                    },
                    title: "Edited branch".into(),
                    updated_at: None,
                    working: false,
                    options: Default::default(),
                };
                l.apply(Event::Completed(
                    id,
                    Ok(Reply::EditableBranch {
                        source,
                        child,
                        text,
                        attachments,
                    }),
                ));
            });
            let child_index = caelum_core::application::Provider::ALL.len();
            assert_eq!(
                rt.state.live.as_ref().unwrap().drafts[child_index].text,
                "Original prompt"
            );
            assert_eq!(
                rt.state.live.as_ref().unwrap().app.selected,
                Some(child_index)
            );
        }
    }
}
