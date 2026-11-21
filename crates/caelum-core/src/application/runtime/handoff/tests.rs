use super::*;
use crate::application::{composer::with_context, Attachment};
use serde_json::{json, Value};

fn page(data: Vec<Value>, next: Option<&str>) -> SessionHistoryPage {
    SessionHistoryPage {
        data,
        next_cursor: next.map(str::to_owned),
        is_working: Some(false),
        agent: None,
        model: None,
        effort: None,
    }
}

#[test]
fn chat_reads_older_pages_and_keeps_only_the_visible_messages_once_in_order() {
    let prompt = with_context(
        "Review the parser",
        &[Attachment::new(
            "local.rs".into(),
            "unpublished edits".into(),
            None,
            false,
        )],
    );
    let user = json!({"id":"u2","type":"user","text":prompt});
    let chat = collect(Provider::OpenCode, |cursor| Ok(match cursor {
        None => page(vec![user.clone(), json!({"id":"a2","type":"assistant","finish":"stop","content":[
            {"type":"reasoning","text":"private reasoning"},
            {"type":"tool","id":"tool","state":{"content":[{"text":"local file contents"}]}},
            {"type":"text","text":"It handles UTF-8."}]})], Some("older")),
        Some("older") => page(vec![
            json!({"id":"u1","type":"user","text":"Start here","files":[{"url":"data:image/png;base64,AAAA"}]}),
            json!({"id":"summary","type":"compaction","summary":"provider summary"}),
            user.clone(),
        ], None),
        _ => panic!("unexpected cursor"),
    })).unwrap();
    assert_eq!(
        chat,
        Chat {
            messages: vec![
                Message::User("Start here".into()),
                Message::User("Review the parser".into()),
                Message::Assistant("It handles UTF-8.".into())
            ],
            clipped: false
        }
    );
}

#[test]
fn codex_chat_uses_text_from_both_speakers_and_leaves_commands_and_images_behind() {
    let chat = collect(Provider::Codex, |_| Ok(page(vec![json!({
        "id":"turn","status":"completed","items":[
        {"type":"userMessage","id":"u","content":[{"type":"text","text":"Review"},{"type":"localImage","path":"local.png"}]},
        {"type":"commandExecution","id":"cmd","aggregatedOutput":"local output"},
        {"type":"agentMessage","id":"a","text":"Ready"}]
    })], None))).unwrap();
    assert_eq!(
        chat.messages,
        [
            Message::User("Review".into()),
            Message::Assistant("Ready".into())
        ]
    );
}

#[test]
fn chat_marks_oversized_unicode_history_and_refuses_ambiguous_pagination_or_a_running_turn() {
    let value = "本".repeat(22_000);
    let chat = collect(Provider::OpenCode, |_| {
        Ok(page(
            vec![json!({"type":"user","id":"u","text":value})],
            None,
        ))
    })
    .unwrap();
    let [Message::User(text)] = chat.messages.as_slice() else {
        panic!()
    };
    assert!(chat.clipped && text.len() <= 64 * 1024 && value.ends_with(text));
    assert!(collect(Provider::Codex, |_| Ok(page(vec![], Some("same")))).is_err());
    assert!(collect(Provider::OpenCode, |_| Ok(SessionHistoryPage {
        is_working: Some(true),
        ..page(vec![], None)
    }))
    .is_err());
}
