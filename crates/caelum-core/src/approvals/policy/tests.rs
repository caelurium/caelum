use super::*;
use crate::approvals::{ApprovalBroker, ApprovalError};
use caelum_protocol::ApprovalDecision;
use serde_json::json;
use std::time::Duration;

fn request(provider: &str, details: Value) -> PermissionRequest {
    PermissionRequest {
        provider: provider.into(),
        project_path: "/checkout".into(),
        session_id: Some("session".into()),
        request_id: "request".into(),
        summary: "Read README.md".into(),
        description: "git status".into(),
        provider_details: details,
    }
}

#[test]
fn exact_codex_commands_require_the_session_root_and_one_shot_authority() {
    let valid = request(
        "codex",
        json!({
            "method":"item/commandExecution/requestApproval",
            "params":{"command":["git","status"],"threadId":"session","cwd":"/checkout","availableDecisions":["acceptForSession","accept"]}
        }),
    );
    assert_eq!(
        classify(&valid),
        Some(ApprovalRule::ExactCommand("git status".into()))
    );
    for (key, value) in [
        ("cwd", json!("/different")),
        ("threadId", json!("other")),
        ("command", json!("git reset --hard")),
        ("command", json!("git status; ls")),
        ("command", json!(["git", "status", "--", "a"])),
        ("command", json!(["sh", "-c", "git status"])),
        ("availableDecisions", json!(["acceptForSession"])),
        ("networkApprovalContext", json!({"host":"example.com"})),
        ("additionalPermissions", json!({"network":true})),
        ("grantRoot", json!("/checkout")),
        ("unrecognisedPrivilege", json!(true)),
    ] {
        let mut req = valid.clone();
        req.provider_details["params"][key] = value;
        assert!(classify(&req).is_none(), "{key}");
    }
    let mut edit = valid;
    edit.provider_details["method"] = "item/fileChange/requestApproval".into();
    assert!(classify(&edit).is_none());
}

#[test]
fn reads_require_one_literal_resource_and_an_identified_session() {
    let read = |p: &str| {
        request(
            "opencode",
            json!({"request":{"action":"read","sessionID":"session","resources":[p]}}),
        )
    };
    let rule = ApprovalRule::ReadFile("src/lib.rs".into());
    assert_eq!(classify(&read("/checkout/src/lib.rs")), Some(rule.clone()));
    for path in [
        "",
        ".",
        "..",
        "/checkout",
        "/checkout-other/src/lib.rs",
        "src/*",
        "src/../other",
        "src/",
    ] {
        assert!(classify(&read(path)).is_none(), "{path}");
    }
    for (key, value) in [
        ("action", json!("edit")),
        ("resources", json!(["a", "b"])),
        ("sessionID", json!("other")),
    ] {
        let mut req = read("src/lib.rs");
        req.provider_details["request"][key] = value;
        assert!(classify(&req).is_none());
    }
    let mut acp = request(
        "antigravity",
        json!({
            "sessionId":"session","toolCall":{"kind":"read","rawInput":{"path":"src/lib.rs"}},
            "options":[{"kind":"allow_always","optionId":"forever"},{"kind":"allow_once","optionId":"once"}]
        }),
    );
    assert_eq!(classify(&acp), Some(rule));
    acp.provider_details["toolCall"]["locations"] = json!([{"path":"other.txt"}]);
    assert!(classify(&acp).is_none());
    acp.provider_details["toolCall"]["locations"] = json!([]);
    acp.provider_details["options"] = json!([{"kind":"allow_always","optionId":"forever"}]);
    assert!(classify(&acp).is_none());
    acp.provider_details["options"] =
        json!([{"kind":"allow_always","optionId":"same"},{"kind":"allow_once","optionId":"same"}]);
    assert!(classify(&acp).is_none());
}

#[test]
fn a_stale_or_modified_offer_cannot_settle_a_reused_request() {
    let broker = ApprovalBroker::default();
    for action in ["read", "edit"] {
        let req = request(
            "opencode",
            json!({"request":{"action":action,"sessionID":"session","resources":["README.md"]}}),
        );
        let old = broker.request(req.clone()).unwrap();
        let old_prompt = broker.pending().pop().unwrap();
        drop(old);
        let current = broker.request(req).unwrap();
        let prompt = broker.pending().pop().unwrap();
        let mut forged = prompt.clone();
        forged.repeatable = Some(ApprovalRule::ReadFile("elsewhere".into()));
        for stale in [old_prompt, forged] {
            assert_eq!(
                broker.resolve_prompt(&stale, ApprovalDecision::AllowOnce),
                Err(ApprovalError::UnknownRequest)
            );
        }
        assert_eq!(broker.pending(), std::slice::from_ref(&prompt));
        broker
            .resolve_prompt(&prompt, ApprovalDecision::Deny)
            .unwrap();
        assert_eq!(current.wait(Duration::ZERO), ApprovalDecision::Deny);
    }
}
