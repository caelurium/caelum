use super::*;
use crate::remote::{Speaker, Thread, ToolState, Turn};

#[test]
fn preview_turns_become_saved_history_and_boat_receipts_are_untouched() {
    let run = json!({"id":"r","conversation":"boat-chat","prompt":"p"});
    let managed = json!({"draft":"next managed","lines":[],"cursor":"page","conversation":"boat-chat","turn":{"Following":run},"boat_model":"preview-model"});
    let mut native = managed.clone();
    native["native_session"] = "ses_preview".into();
    native["draft"] = "next VM message".into();
    native["lines"] = json!([{"speaker":{"Tool":"Running"},"text":"Read","partial":true}]);
    let mut pending = managed.clone();
    pending["turn"] = json!({"NativeSubmitting":{"conversation":"ses_pending","prompt":"msg_pending","id":"msg_pending"}});
    let mut snapshot =
        json!({"remote":{"checkouts":{"vm":{"thread":native,"other_threads":[pending,managed]}}}});
    retire_vm_sessions(&mut snapshot);
    let once = snapshot.clone();
    retire_vm_sessions(&mut snapshot);
    assert_eq!(snapshot, once);
    let checkout = &snapshot["remote"]["checkouts"]["vm"];
    assert_eq!(checkout["other_threads"][1], managed);
    let legacy: Thread = serde_json::from_value(managed).unwrap();
    assert_eq!(legacy.provider, crate::remote::VmProvider::OpenCode);
    assert!(legacy.model.is_none() && legacy.effort.is_none());
    assert_eq!(legacy.turn().run().unwrap().provider, legacy.provider);
    for value in [&checkout["thread"], &checkout["other_threads"][0]] {
        let t: Thread = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(t.turn(), &Turn::Idle);
        assert!(t.cursor().is_none() && t.conversation().is_none());
        assert_eq!(t.lines.last().unwrap().speaker, Speaker::Activity);
    }
    let t: Thread = serde_json::from_value(checkout["thread"].clone()).unwrap();
    assert_eq!(t.draft, "next VM message");
    assert_eq!(t.lines[0].speaker, Speaker::Tool(ToolState::Reported));
    assert!(!t.lines[0].partial);
}

#[test]
fn an_inconsistent_saved_run_requires_reconciliation_before_following() {
    let saved = json!({
        "draft": "next prompt",
        "lines": [],
        "cursor": "stale-page",
        "conversation": "selected-chat",
        "turn": {"Following": {"id": "r", "conversation": "other-chat", "prompt": "p"}},
    });
    let restored: Thread = serde_json::from_value(saved).unwrap();
    assert_eq!(restored.conversation(), Some("selected-chat"));
    assert_eq!(restored.turn(), &Turn::Submitting);
    assert!(restored.cursor().is_none());
    assert_eq!(restored.draft, "next prompt");
    let encoded = serde_json::to_value(&restored).unwrap();
    assert_eq!(encoded["conversation"], "selected-chat");
    assert_eq!(encoded["turn"], "Submitting");
    assert_eq!(serde_json::from_value::<Thread>(encoded).unwrap(), restored);
}
