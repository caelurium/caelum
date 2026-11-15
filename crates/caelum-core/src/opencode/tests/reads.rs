use super::*;
use crate::application::{
    Command as AppCommand, Event, Provider, ProviderRuntime, Reply, Runtime, Scope, SessionKey,
};

struct Service {
    provider: OpenCodeProvider,
    server: Arc<OpenCodeServer>,
    directory: TestDirectory,
}

impl Service {
    fn new() -> Self {
        let directory = TestDirectory::new();
        let executable = directory.0.join("fake-opencode");
        crate::test_support::write_executable(
            &executable,
            include_str!("fixtures/reads_server.py"),
        );
        let provider = OpenCodeProvider::with_executable(
            Arc::new(ProcessRegistry::default()),
            ApprovalBroker::default(),
            QuestionBroker::default(),
            executable,
        );
        let server = provider
            .server(&Project::open(&directory.0).unwrap())
            .unwrap();
        let service = Self {
            provider,
            server,
            directory,
        };
        service.requests();
        service
    }

    fn configure(&self, config: Value) {
        self.server
            .request_json(Method::POST, "/test/config", &[], Some(config))
            .unwrap();
    }

    fn requests(&self) -> Vec<String> {
        serde_json::from_value(
            self.server
                .request_json(Method::GET, "/test/requests", &[], None)
                .unwrap(),
        )
        .unwrap()
    }

    fn path(&self) -> &Path {
        &self.directory.0
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.provider.shutdown();
    }
}

#[test]
fn runtime_resume_returns_metadata_and_history_with_three_reads() {
    let service = Service::new();
    service.configure(json!({"active": true}));
    let mut providers = ProviderRuntime::default();
    providers.opencode = service.provider.clone();
    let (wake, wakes) = mpsc::channel();
    let mut runtime = Runtime::with_providers(providers, move || {
        let _ = wake.send(());
    });
    let key = SessionKey {
        scope: Scope {
            provider: Provider::OpenCode,
            project: service.path().to_string_lossy().into_owned(),
        },
        id: "ses_read".into(),
    };
    runtime.submit(1, AppCommand::Resume(key.clone())).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let (summary, page) = loop {
        let reply = runtime.drain().into_iter().find_map(|event| match event {
            Event::Completed(1, reply) => Some(reply.unwrap()),
            _ => None,
        });
        if let Some(reply) = reply {
            let Reply::Resumed(summary, page) = reply else {
                panic!("resume must include history")
            };
            break (summary, page);
        }
        wakes
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
    };
    assert_eq!(summary.key, key);
    assert!(summary.working);
    assert_eq!(summary.options.model.as_deref(), Some("local/original"));
    assert_eq!(page.model, summary.options.model);
    assert_eq!(page.effort.as_deref(), Some("high"));
    assert_eq!(page.agent.as_deref(), Some("build"));
    assert_eq!(page.is_working, Some(true));
    assert_eq!(page.data[0]["id"], "older");
    assert_eq!(page.data[1]["id"], "newer");
    assert_eq!(page.next_cursor.as_deref(), Some("opaque/+?="));
    assert_eq!(
        service.requests(),
        [
            "GET /api/session/ses_read",
            "GET /api/session/active",
            "GET /api/session/ses_read/message",
        ]
    );
}

#[test]
fn resume_failure_and_older_pages_read_fresh_metadata_and_activity() {
    let service = Service::new();
    let provider = &service.provider;
    service.configure(json!({"history_error": true}));
    assert!(provider
        .resume_with_history(service.path(), "ses_read")
        .is_err());
    service.configure(json!({"history_error": false, "active": true, "model": "replacement"}));
    let (session, first) = provider
        .resume_with_history(service.path(), "ses_read")
        .unwrap();
    assert!(session.session.is_working);
    assert_eq!(first.model.as_deref(), Some("local/replacement"));
    service.configure(json!({"active": false}));
    let older = provider
        .session_history(service.path(), "ses_read", first.next_cursor.as_deref())
        .unwrap();
    assert_eq!(older.is_working, Some(false));
    assert!(older.next_cursor.is_none());
    assert_eq!(
        service.requests(),
        [
            "GET /api/session/ses_read",
            "GET /api/session/active",
            "GET /api/session/ses_read/message",
        ]
        .repeat(3)
    );
}

fn ownership_operations(
    provider: &OpenCodeProvider,
    path: &Path,
) -> Vec<Result<(), OpenCodeError>> {
    vec![
        provider.regenerate_title(path, "ses_read"),
        provider.session_details(path, "ses_read").map(|_| ()),
        provider.cancel_turn(path, "ses_read"),
        provider.select_agent(path, "ses_read", "plan"),
        provider.send_prompt_with_options(
            path,
            "ses_read",
            "Hello",
            None,
            Some("local/new"),
            Some("high"),
        ),
        provider.run_command(
            path,
            "ses_read",
            &OpenCodeCommandInput {
                name: "review".into(),
                text: String::new(),
                files: Vec::new(),
            },
            None,
            None,
        ),
        provider.enqueue_prompt(path, "ses_read", "msg_queued", "Next"),
        provider.inbox(path, "ses_read").map(|_| ()),
        provider.cancel_queued_prompt(path, "ses_read", "msg_queued"),
        provider
            .generate_from_context(path, "ses_read", "Question")
            .map(|_| ()),
        provider.background_shells(path, "ses_read").map(|_| ()),
        provider
            .shell_action(&path.to_string_lossy(), "ses_read", "sh_read", false)
            .map(|_| ()),
        provider
            .shell_action(&path.to_string_lossy(), "ses_read", "sh_read", true)
            .map(|_| ()),
    ]
}

#[test]
fn ownership_only_operations_do_not_depend_on_activity_reads() {
    let service = Service::new();
    service.configure(json!({"active_error": true}));
    let results = ownership_operations(&service.provider, service.path());
    let count = results.len();
    for result in results {
        result.unwrap();
    }
    let requests = service.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|r| *r == "GET /api/session/ses_read")
            .count(),
        count
    );
    assert!(!requests.iter().any(|r| r.contains("/active")));
    assert!(requests.contains(&"POST /api/session/ses_read/model".into()));
    assert!(requests.contains(&"POST /api/session/ses_read/agent".into()));
    assert!(requests.contains(&"DELETE /api/shell/sh_read".into()));
}

#[test]
fn ownership_is_revalidated_before_history_and_every_operation() {
    let service = Service::new();
    let provider = &service.provider;
    provider
        .resume_with_history(service.path(), "ses_read")
        .unwrap();
    service.requests();
    // Prior successful ownership must not authorize a moved or mismatched session.
    for invalid in [
        json!({"directory": "/another-checkout"}),
        json!({"id": "ses_other"}),
        json!({"id": null}),
    ] {
        service.configure(json!({"directory": service.path(), "id": "ses_read"}));
        service.configure(invalid);
        assert!(provider
            .resume_with_history(service.path(), "ses_read")
            .is_err());
        let results = ownership_operations(provider, service.path());
        let count = results.len() + 1;
        assert!(results.into_iter().all(|result| result.is_err()));
        assert_eq!(service.requests(), vec!["GET /api/session/ses_read"; count]);
    }
    assert!(matches!(
        provider.resume_with_history(service.path(), "../invalid"),
        Err(OpenCodeError::InvalidSessionId)
    ));
    assert!(service.requests().is_empty());
}

#[test]
fn busy_checks_still_use_fresh_activity_and_fail_closed() {
    let service = Service::new();
    let provider = &service.provider;
    assert!(
        !provider
            .resume_session(service.path(), "ses_read")
            .unwrap()
            .session
            .is_working
    );
    service.configure(json!({"active": true, "archived": true}));
    service.requests();
    assert!(provider
        .set_session_archived(service.path(), "ses_read", true)
        .is_err());
    assert!(provider.delete_session(service.path(), "ses_read").is_err());
    assert!(provider.list_sessions(service.path()).unwrap().sessions[0].is_working);
    assert!(provider
        .ensure_checkout_idle(&service.path().canonicalize().unwrap())
        .is_err());
    let requests = service.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|r| *r == "GET /api/session/active")
            .count(),
        4
    );
    assert!(requests.iter().all(|r| r.starts_with("GET ")));

    service.configure(json!({"active_error": true}));
    assert!(provider
        .set_session_archived(service.path(), "ses_read", true)
        .is_err());
    assert!(provider.delete_session(service.path(), "ses_read").is_err());
    assert_eq!(
        service.requests(),
        ["GET /api/session/ses_read", "GET /api/session/active"].repeat(2)
    );
    service.configure(json!({"active_error": false, "active": false}));
    provider
        .set_session_archived(service.path(), "ses_read", true)
        .unwrap();
    provider.delete_session(service.path(), "ses_read").unwrap();
    assert_eq!(
        service.requests(),
        [
            "GET /api/session/ses_read",
            "GET /api/session/active",
            "PATCH /api/session/ses_read",
            "GET /api/session/ses_read",
            "GET /api/session/active",
            "DELETE /api/session/ses_read",
        ]
    );
}
