use super::*;
use caelum_core::application::{Event, Provider};
use serde_json::json;

#[test]
fn background_completion_output_is_expandable_and_copyable_live_and_restored() {
    let families = spec().families;
    let _fonts = rune::text::with_families(families.0, families.1);
    let output = "Build finished café\n2 tests passed\n";
    for restored in [false, true] {
        let mut rt = live_runtime_size((800.0, 620.0));
        let clipboard = rune::clipboard::memory();
        rt.use_clipboard(&clipboard);
        rt.update(|s, _| {
            let live = s.live.as_mut().unwrap();
            live.fixture = true;
            live.app.sessions[0].scope.provider = Provider::OpenCode;
            live.app.sessions[0].remote_id = Some("ses_build".into());
            let data = json!({
                "id": "msg_finished", "type": "synthetic", "sessionID": "ses_build",
                "time": {"created": 2000}, "description": "cargo test",
                "metadata": {"source": "shell", "shellID": "sh_build", "state": "completed", "exit": 0},
                "text": format!("<shell id=\"sh_build\" state=\"completed\" command=\"cargo test\">\n{output}\n</shell>")
            });
            if restored {
                live.app.sessions[0].conversation.history(Provider::OpenCode, &[data], false);
            } else {
                live.apply(Event::OpenCode(caelum_protocol::OpenCodeEvent {
                    project_path: "/project".into(), event_type: "session.synthetic".into(),
                    id: Some("evt_finished".into()), created: Some(2000), data, location: None,
                }));
            }
        });
        advance(&mut rt, 0.4);
        let key = "tool-0-opencode-shell-sh_build";
        assert!(element(&rt, key).visible[3] > 0.0);
        click(&mut rt, key);
        advance(&mut rt, 0.3);
        assert_eq!(element(&rt, key).expanded, Some(true));
        assert!(rt.elements().iter().any(|e| {
            !e.exiting
                && e.visible[3] > 0.0
                && e.text
                    .as_deref()
                    .is_some_and(|text| text.contains("Build finished café"))
        }));
        assert!(!rt.elements().iter().any(|e| {
            !e.exiting
                && e.text
                    .as_deref()
                    .is_some_and(|text| text.contains("<shell") || text.contains("sh_build"))
        }));
        click(&mut rt, "tool-output-0-opencode-shell-sh_build-Output-copy");
        assert_eq!(clipboard.text().unwrap().as_str(), output);
    }
}

#[test]
fn an_open_output_reader_receives_final_output_after_navigation_and_cache_retirement() {
    let _fonts = rune::text::with_families("Inter", "Inter");
    for synthetic in [false, true] {
        let mut rt = design_runtime("shells", (800.0, 620.0));
        click(&mut rt, "view-shell-output");
        rt.update(|s, _| {
            let live = s.live.as_mut().unwrap();
            live.app.select(1);
            let data = if synthetic {
                json!({
                    "sessionID": "ses_fixture", "description": "cargo test --workspace",
                    "metadata": {"source": "shell", "shellID": "shell_fixture", "state": "completed", "exit": 0},
                    "text": "<shell id=\"shell_fixture\" state=\"completed\" command=\"cargo test --workspace\">\nFinal café output\n</shell>"
                })
            } else {
                json!({"sessionID": "ses_fixture", "shell": {
                    "id": "shell_fixture", "command": "cargo test --workspace", "status": "exited", "exit": 0,
                    "time": {"started": 1000, "completed": 2000}
                }, "output": {"output": "Final café output"}})
            };
            live.apply(Event::OpenCode(caelum_protocol::OpenCodeEvent {
                project_path: "/projects/caelum".into(),
                event_type: if synthetic { "session.synthetic" } else { "session.shell.ended" }.into(),
                id: Some("evt_finished".into()), created: Some(2000), data, location: None,
            }));
        });
        advance(&mut rt, 0.3);
        let live = rt.state.live.as_ref().unwrap();
        assert_eq!(live.app.selected, Some(1));
        assert!(live.app.sessions[0].shells.is_empty());
        assert!(
            live.app.services.checkouts["/projects/caelum"]
                .shell_output
                .is_empty()
        );
        let reader = element(&rt, "shell-output-shell_fixture");
        assert!(
            rt.elements()
                .iter()
                .any(|e| e.path.starts_with(&reader.path)
                    && e.visible[3] > 0.0
                    && e.text.as_deref() == Some("Final café output"))
        );
    }
}
