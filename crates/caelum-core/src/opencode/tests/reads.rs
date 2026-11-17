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

fn resume_reply(service: &Service, path: &Path) -> Result<Reply, String> {
    relocation_reply(
        service,
        AppCommand::Resume(SessionKey {
            scope: Scope {
                provider: Provider::OpenCode,
                project: path.to_string_lossy().into_owned(),
            },
            id: "ses_read".into(),
        }),
    )
}

fn relocation_reply(service: &Service, command: AppCommand) -> Result<Reply, String> {
    let mut providers = ProviderRuntime::default();
    providers.opencode = service.provider.clone();
    let (wake, wakes) = mpsc::channel();
    let mut runtime = Runtime::with_providers(providers, move || {
        let _ = wake.send(());
    });
    let verify = matches!(command, AppCommand::VerifyOpenCodeMove { .. });
    runtime.submit(1, command).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(reply) = runtime.drain().into_iter().find_map(|event| match event {
            Event::Completed(1, reply) => Some(reply),
            _ => None,
        }) {
            let requests = service.requests();
            if verify {
                assert_eq!(requests, ["GET /api/session/ses_read"]);
            }
            assert!(requests.iter().all(|request| request.starts_with("GET ")));
            assert!(!requests.iter().any(|request| request.ends_with("/message")));
            return reply;
        }
        wakes
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
    }
}

#[test]
fn relocation_from_discovery_verifies_the_id_and_directory_without_accessing_the_old_project() {
    for case in ["moved", "moved again", "different id", "relative directory"] {
        let service = Service::new();
        let destination = service
            .path()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let previous = service.path().join("deleted");
        let id = "ses_read";
        let from = SessionKey {
            scope: Scope {
                provider: Provider::OpenCode,
                project: previous.to_str().unwrap().into(),
            },
            id: id.into(),
        };
        let metadata = match case {
            "moved" => json!({"archived":true, "active_error":true}),
            "moved again" => json!({"directory": service.path().parent().unwrap()}),
            "different id" => json!({"id": "ses_other"}),
            _ => json!({"directory":"."}),
        };
        service.configure(metadata);
        let reply = relocation_reply(
            &service,
            AppCommand::VerifyOpenCodeMove {
                from: from.clone(),
                destination: destination.clone(),
            },
        );
        match case {
            "moved" => {
                let Ok(Reply::SessionMoved(change)) = reply else {
                    panic!("expected the verified destination")
                };
                assert_eq!(change.project.path, destination);
                assert_eq!(change.from, from);
                assert!(change.archived);
                assert!(!change.linked_worktree);
            }
            "moved again" => assert!(matches!(reply, Ok(Reply::Done))),
            _ => assert!(reply.is_err()),
        }
    }
}

fn git(path: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repository(path: &Path) {
    git(path, &["init", "--quiet"]);
    fs::create_dir(path.join("src")).unwrap();
    fs::write(path.join("src/example.txt"), "fixture\n").unwrap();
    git(path, &["add", "src/example.txt"]);
    git(
        path,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    );
}

#[test]
fn resume_locates_a_moved_session_only_in_a_verified_linked_checkout() {
    let service = Service::new();
    repository(service.path());
    let destination = TestDirectory::new();
    git(
        service.path(),
        &[
            "worktree",
            "add",
            "--detach",
            destination.0.to_str().unwrap(),
        ],
    );
    service.configure(json!({"directory": destination.0}));
    // Mutations and ordinary history still reject the stale checkout.
    assert!(matches!(
        service.provider.cancel_turn(service.path(), "ses_read"),
        Err(OpenCodeError::SessionNotInProject)
    ));
    assert!(matches!(
        service
            .provider
            .session_history(service.path(), "ses_read", None),
        Err(OpenCodeError::SessionNotInProject)
    ));
    service.requests();
    let Reply::SessionMoved(change) = resume_reply(&service, service.path()).unwrap() else {
        panic!("expected a verified relocation, not history under the old checkout")
    };
    assert!(change.linked_worktree);
    assert_eq!(change.from.id, "ses_read");
    assert_eq!(change.from.scope.project, service.path().to_string_lossy());
    assert_eq!(
        Path::new(&change.project.path),
        destination.0.canonicalize().unwrap()
    );
}

#[test]
fn cross_project_move_needs_an_open_destination_and_invalid_metadata_is_rejected() {
    for wrong in ["unrelated", "id", "missing"] {
        let service = Service::new();
        repository(service.path());
        let destination = TestDirectory::new();
        if wrong == "unrelated" {
            repository(&destination.0);
        } else {
            git(
                service.path(),
                &[
                    "worktree",
                    "add",
                    "--detach",
                    destination.0.to_str().unwrap(),
                ],
            );
        }
        service.configure(json!({"directory": destination.0}));
        if wrong == "id" {
            service.configure(json!({"id":"ses_other"}));
        }
        if wrong == "missing" {
            service.configure(json!({"directory":null}));
        }
        let reply = resume_reply(&service, service.path());
        if wrong == "unrelated" {
            let Ok(Reply::SessionMoved(change)) = reply else {
                panic!("expected an open-project hint, not a linked worktree")
            };
            assert!(!change.linked_worktree);
            assert_eq!(
                Path::new(&change.project.path),
                destination.0.canonicalize().unwrap()
            );
        } else {
            assert!(reply.is_err(), "{wrong}");
        }
    }
}

#[test]
fn relocation_keeps_repository_subdirectory_scopes() {
    for same_subpath in [true, false] {
        let service = Service::new();
        repository(service.path());
        let destination = TestDirectory::new();
        git(
            service.path(),
            &[
                "worktree",
                "add",
                "--detach",
                destination.0.to_str().unwrap(),
            ],
        );
        let source = service.path().join("src");
        // Install the same fake connection for the scoped source directory.
        let slot = Arc::new(ServerSlot::default());
        slot.connect(&service.provider.inner.processes, |_| {
            Ok(service.server.clone())
        })
        .unwrap();
        service
            .provider
            .inner
            .lock_servers()
            .insert(source.canonicalize().unwrap(), slot);
        service.configure(json!({"directory": if same_subpath { destination.0.join("src") } else { destination.0.clone() }}));
        let Ok(Reply::SessionMoved(change)) = resume_reply(&service, &source) else {
            panic!("expected a move")
        };
        assert_eq!(change.linked_worktree, same_subpath);
    }
}
