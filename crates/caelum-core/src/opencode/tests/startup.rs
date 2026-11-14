use super::*;

#[cfg(unix)]
#[test]
fn initial_effort_is_admitted_without_a_model_switch_before_the_first_prompt() {
    let directory = TestDirectory::new();
    let project = directory.0.join("project");
    fs::create_dir_all(&project).unwrap();
    let executable = directory.0.join("fake-opencode");
    crate::test_support::write_executable(&executable, include_str!("fixtures/managed_server.py"));
    let provider = OpenCodeProvider::with_executable(
        Arc::new(ProcessRegistry::default()),
        ApprovalBroker::default(),
        QuestionBroker::default(),
        &executable,
    );
    let requests = || {
        fs::read_to_string(project.join("requests.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|request| !request["path"].as_str().unwrap().contains("permission"))
            .collect::<Vec<_>>()
    };
    for (model, effort) in [
        (Some("local/model/a"), Some("max")),
        (None, Some("high")),
        (None, Some("")),
        (Some("local/model/a"), None),
    ] {
        let before = requests().len();
        let session = provider
            .start_session_with_options(&project, model, effort, Some("plan"))
            .unwrap()
            .session;
        assert_eq!(session.model.as_deref(), Some("local/model/a"));
        assert_eq!(session.effort.as_deref(), Some(effort.unwrap_or("")));
        assert_eq!(session.agent.as_deref(), Some("plan"));
        provider
            .send_prompt_with_options(&project, &session.id, "Hello", None, model, effort)
            .unwrap();
        let sent = requests();
        assert_eq!(sent.len(), before + 2, "unexpected model switch: {sent:?}");
        assert_eq!(sent[before]["path"], "/api/session");
        assert_eq!(
            sent[before]["body"]["model"],
            model_ref("local/model/a", effort).unwrap()
        );
        assert_eq!(sent[before + 1]["path"], "/api/session/ses_fake/prompt");
        assert_eq!(sent[before + 1]["body"], json!({"text":"Hello", "id":null}));
    }
    provider.shutdown();
}
