use super::*;
use crate::remote::{runtime::Backend, Operation, Outcome};
use serde_json::json;

#[test]
fn routed_choices_keep_the_boat_ids_in_harness_order() {
    let lists = model_lists(
        serde_json::from_value(json!({
            "opencode": {"default":"route-b", "models":[
                {"id":"route-b","label":"Example model","sdkId":"vendor/same","reasoningEffort":{"supported":["none","","high","high","bad\nlevel","next-level"],"default":"high"}},
                {"id":"route-a","label":"Example model","sdkId":"vendor/same"},
                {"id":"route-a","label":"Duplicate entry"}
            ]},
            "codex": {"models":[{"id":"codex-only","label":"Example Codex","reasoningEffort":{"supported":["low"],"default":"unlisted"}}]},
            "another-harness": {"models":[{"id":"other","label":"Other"}]}
        }))
        .unwrap(),
    );
    assert_eq!(lists.len(), 2);
    let open = &lists[&Provider::OpenCode];
    assert_eq!(
        open.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        ["route-b", "route-a"]
    );
    assert_eq!(open[0].name, "Example model");
    assert_eq!(
        open[0]
            .efforts
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        ["none", "high", "next-level"]
    );
    assert_eq!(open[0].default_effort.as_deref(), Some("high"));
    assert!(open[1].efforts.is_empty());
    assert_eq!(lists[&Provider::Codex][0].id, "codex-only");
    assert!(lists[&Provider::Codex][0].default_effort.is_none());
}

#[test]
#[ignore = "reads Boat's live public catalogue; requires network access"]
fn live_public_vm_model_catalogue() {
    let Outcome::Models(lists) = Backend::default().execute(&Operation::Models).unwrap() else {
        panic!()
    };
    for provider in Provider::ALL {
        assert!(
            !lists[&provider].is_empty(),
            "no {} models",
            provider.name()
        );
    }
}
