use super::*;
use crate::application::{composer::with_context, Attachment, Scope};
use serde_json::json;
use std::fs;

fn owner(provider: Provider) -> SessionKey {
    SessionKey {
        scope: Scope {
            project: "/example".into(),
            provider,
        },
        id: "session".into(),
    }
}
fn page(data: Vec<Value>, next: Option<&str>) -> SessionHistoryPage {
    SessionHistoryPage {
        data,
        next_cursor: next.map(str::to_owned),
        model: None,
        agent: None,
        effort: None,
        is_working: Some(false),
    }
}

#[test]
fn exports_all_pages_oldest_first_with_the_stated_scope_and_without_touching_the_transcript() {
    let dir = tempfile::tempdir().unwrap();
    for provider in Provider::ALL {
        let prompt = with_context(
            "Review café",
            &[Attachment::new(
                "a.rs".into(),
                "snapshot bytes".into(),
                None,
                false,
            )],
        );
        let (old, latest) = match provider {
            Provider::Codex => (
                json!({"id":"first","itemsView":"full","items":[{"id":"u","type":"userMessage","content":[{"type":"text","text":prompt}]}]}),
                json!({"id":"last","items":[{"id":"t","type":"commandExecution","command":"git status","aggregatedOutput":"tree clean"},{"id":"a","type":"agentMessage","text":"Finished"}]}),
            ),
            Provider::OpenCode => (
                json!({"id":"first","type":"user","text":prompt}),
                json!({"id":"last","type":"assistant","content":[{"id":"t","type":"tool","name":"shell","state":{"status":"completed","input":"git status","content":[{"type":"text","text":"tree clean"}]}},{"type":"text","text":"Finished"}]}),
            ),
            Provider::Antigravity => (
                json!({"id":"first","type":"user","text":prompt}),
                json!({"id":"last","type":"tool","name":"shell","status":"completed","input":"git status","output":"tree clean\nFinished"}),
            ),
        };
        for format in [ExportFormat::Json, ExportFormat::Markdown] {
            let path = dir.path().join(format.extension());
            let mut reads = Vec::new();
            save(&owner(provider), "My title", format, &path, |cursor| {
                reads.push(cursor.map(str::to_owned));
                Ok(if cursor.is_some() {
                    page(vec![old.clone()], None)
                } else {
                    page(vec![latest.clone()], Some("opaque"))
                })
            })
            .unwrap();
            assert_eq!(reads, vec![None, Some("opaque".into()), None]);
            let text = fs::read_to_string(path).unwrap();
            assert!(text.contains("tree clean") && text.contains("Finished"));
            if format == ExportFormat::Json {
                let doc: Value = serde_json::from_str(&text).unwrap();
                assert_eq!(doc["records"], json!([old, latest]));
                assert_eq!(doc["session"]["scope"]["provider"], json!(provider));
            } else {
                assert!(text.find("Review café").unwrap() < text.find("tree clean").unwrap());
                assert!(text.contains("a.rs") && !text.contains("snapshot bytes"));
            }
        }
    }
}

#[test]
fn broken_or_changing_pagination_never_replaces_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("existing");
    std::fs::write(&path, "keep me").unwrap();
    for failure in ["cycle", "overlap", "changed", "error", "partial"] {
        let mut n = 0;
        let result = save(
            &owner(Provider::Codex),
            "Title",
            ExportFormat::Json,
            &path,
            |_| {
                n += 1;
                let record = |id: &str| json!({"id":id,"items":[],"itemsView":"full"});
                Ok(match n {
                    1 => page(vec![record("tip")], Some("older")),
                    2 => match failure {
                        "cycle" => page(vec![record("a")], Some("older")),
                        "overlap" => page(vec![record("tip")], None),
                        "partial" => page(
                            vec![json!({"id":"a","items":[],"itemsView":"summary"})],
                            None,
                        ),
                        "error" => return Err("offline".into()),
                        _ => page(vec![record("a")], None),
                    },
                    _ => page(vec![record("new tip")], None),
                })
            },
        );
        assert!(result.is_err(), "{failure}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep me");
    }
}
