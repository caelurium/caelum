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
    for value in [&checkout["thread"], &checkout["other_threads"][0]] {
        let t: Thread = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(t.turn, Turn::Idle);
        assert!(t.cursor.is_none() && t.conversation.is_none());
        assert_eq!(t.lines.last().unwrap().speaker, Speaker::Activity);
    }
    let t: Thread = serde_json::from_value(checkout["thread"].clone()).unwrap();
    assert_eq!(t.draft, "next VM message");
    assert_eq!(t.lines[0].speaker, Speaker::Tool(ToolState::Reported));
    assert!(!t.lines[0].partial);
}
