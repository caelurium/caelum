use super::{agent_target, fixture};
use crate::{
    remote::VmProvider,
    remote::{harness::Harness, Run, RunStatus, Speaker, Thread, ToolState, TurnEnd},
};
use serde_json::json;
use wiremock::{
    matchers::{path, query_param},
    Mock, ResponseTemplate,
};

#[tokio::test]
async fn a_response_can_gain_tools_at_the_same_event_id_before_the_turn_finishes() {
    let (server, boat) = fixture().await;
    let target = agent_target();
    let run = Run {
        provider: VmProvider::Codex,
        conversation: "chat".into(),
        prompt: "p".into(),
        id: "run".into(),
    };
    let mut thread = Thread::default();
    let mut cursor = Some("initial".to_owned());
    for (i, done) in [false, false, true].into_iter().enumerate() {
        let status = Mock::given(path("/sandboxes/vm-1/prompts/p"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"type":"status","id":"vm-1","promptRun":{"id":"run","sandboxId":"vm-1","promptId":"p","conversationId":"chat","status":if done {"finished"} else {"running"},"done":done}})))
            .expect(1).mount_as_scoped(&server).await;
        let mut events = Vec::new();
        if i == 0 {
            events.push(json!({"type":"prompt","id":"p","timestamp":10,"taskId":"p","conversationId":"chat"}));
        }
        let mut data = json!({"content":"Checking tests","is_streaming":!done});
        if i > 0 {
            data["tools"] =
                json!([{"use":{"id":"call","name":"Bash","input":{"command":"cargo test"}}}]);
            if done {
                data["tools"][0]["result"] = json!({"content":"ok","is_error":false});
            }
        }
        events.push(json!({"type":"response","id":"reply","timestamp":11,"taskId":"p","conversationId":"chat","data":data}));
        let guard = Mock::given(path("/sandboxes/vm-1/events")).and(query_param("sort","asc"))
            .and(query_param("conversation","chat")).and(query_param("cursor",if i == 0 {"initial"} else {"MTA6cA"}))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"type":"events.list","id":"vm-1","events":events,"pageInfo":{"nextCursor":null,"hasMore":false,"limit":100}})))
            .expect(1).mount_as_scoped(&server).await;
        let page = boat.follow(&target, &run, cursor.as_deref()).await.unwrap();
        assert!(!page.more);
        assert_eq!(
            page.status,
            if done {
                RunStatus::Ended(TurnEnd::Finished)
            } else {
                RunStatus::Running
            }
        );
        cursor = page.cursor;
        for l in page.lines {
            thread.append(l);
        }
        assert_eq!(thread.lines.len(), if i == 0 { 1 } else { 2 });
        if i > 0 {
            assert_eq!(
                thread.lines[1].speaker,
                Speaker::Tool(if done {
                    ToolState::Complete
                } else {
                    ToolState::Running
                })
            );
        }
        drop((status, guard));
    }
    assert!(!thread.lines[0].partial);
    assert_eq!(cursor.as_deref(), Some("MTE6cmVwbHk"));
}
