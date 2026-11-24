use crate::application::*;
use serde_json::json;

fn workspace() -> Workspace {
    let mut app = Workspace::default();
    for (path, name) in [
        ("/alpha", "Alpha"),
        ("/beta", "Beta"),
        ("/linked", "Linked"),
    ] {
        app.projects.push(caelum_protocol::ProjectInfo {
            path: path.into(),
            name: name.into(),
        });
        let i = app.new_session(Scope {
            project: path.into(),
            provider: Provider::Codex,
        });
        app.sessions[i].last_activity = Some(1000 + i as u64);
        app.sessions[i].remote_id = Some("shared-id".into());
        app.sessions[i].title = format!("Session {name}");
    }
    app.project_roots.insert("/linked".into(), "/alpha".into());
    app.select(0);
    app.commands();
    app
}

#[test]
fn scope_filters_include_linked_checkouts_but_exclude_closed_archived_and_forgotten_sessions() {
    let mut app = workspace();
    assert_eq!(app.session_feed(None, "", false), [2, 1, 0]);
    app.preferences.session_scope = SessionScope::CurrentProject;
    assert_eq!(app.session_feed(None, "", false), [2, 0]);
    app.active_project = None;
    assert!(app.session_feed(None, "", false).is_empty());
    app.select(0);
    assert!(app.session_feed(None, "Beta", false).is_empty());
    assert_eq!(app.session_feed(Some("/beta"), "codex", false), [1]);
    app.sessions[2].archive_fixture(ArchiveFixture::Stable(true));
    assert_eq!(app.session_feed(None, "", true), [2]);
    app.sessions[2].archive_fixture(ArchiveFixture::Forgotten);
    assert!(app.session_feed(None, "", true).is_empty());
    app.preferences.session_scope = SessionScope::AllProjects;
    app.close_project("/beta");
    assert_eq!(app.session_feed(None, "", false), [0]);
}

#[test]
fn sidebar_ranks_active_sessions_before_newer_idle_sessions_within_the_current_scope() {
    let mut app = workspace();
    app.select(2);
    app.sessions[2].draft = "Keep this draft".into();
    app.sessions[0].conversation.set_working(true);
    app.apply(Event::Questions(vec![caelum_protocol::QuestionPrompt {
        provider: "codex".into(),
        project_path: "/beta".into(),
        session_id: "shared-id".into(),
        request_id: "choice".into(),
        title: "Which branch?".into(),
        fields: vec![],
    }]));
    let rows = |app: &Workspace| app.sidebar_sessions(None, "session", false);
    assert_eq!(rows(&app), [1, 0, 2].map(FeedSession::Local));

    app.preferences.session_scope = SessionScope::CurrentProject;
    assert_eq!(rows(&app), [0, 2].map(FeedSession::Local));
    app.preferences.session_scope = SessionScope::AllProjects;
    app.sessions[0].last_activity = app.sessions[1].last_activity;
    assert_eq!(rows(&app), [0, 1, 2].map(FeedSession::Local));
    app.sessions[0].conversation.set_working(false);
    assert_eq!(rows(&app), [1, 2, 0].map(FeedSession::Local));
    app.apply(Event::Questions(vec![]));
    assert_eq!(rows(&app), [2, 0, 1].map(FeedSession::Local));
}

#[test]
fn overview_order_and_membership_are_independent_of_recency_and_sidebar_scope() {
    let mut app = workspace();
    let all = [
        FeedSession::Local(0),
        FeedSession::Local(1),
        FeedSession::Local(2),
    ];
    app.preferences.session_scope = SessionScope::CurrentProject;
    app.sessions[1].last_activity = Some(u64::MAX);
    app.select(2);
    app.sessions[2].draft = "Return to this draft".into();
    app.sessions[1].unread = true;
    app.sessions[0].error = Some("The previous turn failed.".into());
    assert!(app.overview_sessions("").is_empty());
    assert!(app.overview_sessions("beta").is_empty());
    assert_eq!(
        app.sidebar_sessions(Some("/beta"), "beta", false),
        [FeedSession::Local(1)]
    );
    for session in &mut app.sessions {
        session.conversation.set_working(true);
    }
    assert_eq!(app.overview_sessions(""), all);
    assert_eq!(app.overview_sessions("beta"), [FeedSession::Local(1)]);
    app.close_project("/beta");
    app.sessions[2].archive_fixture(ArchiveFixture::Stable(true));
    assert_eq!(app.overview_sessions(""), [FeedSession::Local(0)]);
    app.sessions[0].archive_fixture(ArchiveFixture::Forgotten);
    assert!(app.overview_sessions("").is_empty());
}

#[test]
fn overview_keeps_queued_and_background_work_only_while_it_is_live() {
    let mut app = workspace();
    app.sessions[0].conversation.set_working(true);
    assert!(app.send(0, "Next prompt".into()));
    app.sessions[0].conversation.set_working(false);
    app.sessions[1].scope.provider = Provider::OpenCode;
    let item = json!({"type":"user", "payload":{"text":"Next on the provider"}});
    app.sessions[1].inbox.event(
        "session.inbox.enqueued",
        &json!({"inboxID":"next", "item":item}),
    );
    app.sessions[2].shells.push(
        serde_json::from_value(json!({"id":"build", "command":"cargo build", "startedAt":1}))
            .unwrap(),
    );
    assert_eq!(app.overview_sessions(""), [0, 1, 2].map(FeedSession::Local));

    app.sessions[0].queue_paused = true;
    let inbox = &mut app.sessions[1].inbox;
    inbox.settle("next");
    inbox.enqueue("failed".into(), "Undelivered prompt".into(), 12);
    inbox.complete_mutation("failed", 12, Err("Delivery failed".into()));
    assert_eq!(app.overview_sessions(""), [FeedSession::Local(2)]);
    app.sessions[2].disconnected = true;
    assert!(app.overview_sessions("").is_empty());
}

#[test]
fn overview_does_not_activate_idle_conversations_for_a_project_wide_approval() {
    let mut app = workspace();
    let other = app.new_session(app.sessions[0].scope.clone());
    app.approvals.push(caelum_protocol::ApprovalPrompt {
        attempt: "fixture".into(),
        repeatable: None,
        provider: "codex".into(),
        project_path: "/alpha".into(),
        session_id: None,
        request_id: "approval".into(),
        summary: "Allow the operation?".into(),
        description: String::new(),
    });
    assert!(app.overview_sessions("").is_empty());
    app.sessions[other].conversation.set_working(true);
    assert_eq!(app.overview_sessions(""), [FeedSession::Local(other)]);
    app.approvals[0].session_id = app.sessions[0].remote_id.clone();
    assert_eq!(
        app.overview_sessions(""),
        [0, other].map(FeedSession::Local)
    );
    app.approvals.clear();
    assert_eq!(app.overview_sessions(""), [FeedSession::Local(other)]);
}

#[test]
fn search_matches_project_names_branches_and_unicode_worktree_labels() {
    let mut app = workspace();
    app.projects[0].name = "Café".into();
    app.sessions[2].details = Some(caelum_protocol::SessionDetails {
        branch: Some("fix/scroll".into()),
        branches: vec![],
        branch_is_current: false,
        changes: None,
    });
    assert_eq!(app.session_feed(None, "CAFÉ", false), [2, 0]);
    assert_eq!(app.session_feed(None, "fix/scroll", false), [2]);
    app.sessions[0].last_activity = None;
    app.sessions[1].last_activity = Some(1002);
    assert_eq!(app.session_feed(None, "", false), [1, 2, 0]);
}

fn refresh_branch(app: &mut Workspace, project: &str, branch: Option<&str>) {
    app.apply(Event::Checkout(project.into()));
    let request = app
        .commands()
        .into_iter()
        .find(|(_, command)| {
            matches!(command, Command::Task(Task::Git { project: owner }) if owner == project)
        })
        .unwrap()
        .0;
    app.apply(Event::Completed(
        request,
        Ok(Reply::Task(TaskReply::Git(
            caelum_protocol::GitRepositoryStatus {
                project_path: project.into(),
                repository_root: Some(project.into()),
                branch: branch.map(str::to_owned),
                changes: vec![],
            },
            vec![],
        ))),
    ));
}

#[test]
fn active_session_branches_follow_checkout_changes_and_detach_without_losing_history() {
    for provider in Provider::ALL {
        let mut app = workspace();
        app.sessions[0].scope.provider = provider;
        app.sessions[0].details = Some(caelum_protocol::SessionDetails {
            branch: Some("feature/opened".into()),
            branches: vec!["feature/earlier".into()],
            branch_is_current: false,
            changes: None,
        });
        app.sessions[0].draft = "Keep my draft".into();
        app.select(1);
        assert_eq!(app.session_branch(&app.sessions[0]), Some("feature/opened"));
        refresh_branch(&mut app, "/linked", Some("feature/worktree"));
        for (branch, working) in [
            (Some("feature/agent"), true),
            (Some("feature/manual"), false),
            (None, false),
            (Some("feature/reattached"), false),
        ] {
            app.sessions[0].conversation.set_working(working);
            refresh_branch(&mut app, "/alpha", branch);
            assert_eq!(app.session_branch(&app.sessions[0]), branch);
            assert_eq!(app.session_branch(&app.sessions[1]), None);
            assert_eq!(
                app.session_branch(&app.sessions[2]),
                Some("feature/worktree")
            );
            assert!(app.session_feed(None, "feature/opened", false).is_empty());
            if let Some(branch) = branch {
                assert_eq!(app.session_feed(None, branch, false), [0]);
            }
        }
        assert_eq!(app.selected, Some(1));
        assert_eq!(app.sessions[0].draft, "Keep my draft");
        assert_eq!(app.sessions[0].last_activity, Some(1000));
        let details = app.sessions[0].details.as_ref().unwrap();
        assert_eq!(details.branch.as_deref(), Some("feature/opened"));
        assert_eq!(details.branches, ["feature/earlier"]);

        for (request, command) in app.commands() {
            if matches!(command, Command::Task(Task::Discover { .. })) {
                app.apply(Event::Completed(
                    request,
                    Ok(Reply::Task(TaskReply::Discovered(None))),
                ));
            }
        }
        app.refresh_feed_pull_requests(Some("/alpha"));
        let branches: std::collections::BTreeSet<_> = app
            .commands()
            .into_iter()
            .filter_map(|(_, command)| match command {
                Command::Task(Task::Discover { project, branch }) if project == "/alpha" => {
                    Some(branch)
                }
                _ => None,
            })
            .collect();
        for branch in ["feature/opened", "feature/earlier", "feature/reattached"] {
            assert!(branches.contains(branch), "missing PR branch {branch}");
        }
    }
}

#[test]
fn archived_session_rows_keep_the_recorded_branch_until_restored() {
    let mut app = workspace();
    app.sessions[0].details = Some(caelum_protocol::SessionDetails {
        branch: Some("feature/archived".into()),
        branches: vec!["feature/archived".into()],
        branch_is_current: false,
        changes: None,
    });
    app.sessions[0].archive_fixture(ArchiveFixture::Stable(true));
    refresh_branch(&mut app, "/alpha", Some("feature/current"));
    assert_eq!(
        app.session_branch(&app.sessions[0]),
        Some("feature/archived")
    );
    assert_eq!(app.session_feed(None, "feature/archived", true), [0]);
    app.sessions[0].archive_fixture(ArchiveFixture::Stable(false));
    assert_eq!(
        app.session_branch(&app.sessions[0]),
        Some("feature/current")
    );
}

#[test]
fn recency_and_scope_roundtrip_and_old_snapshots_keep_unknown_recency() {
    let mut original = workspace();
    original.preferences.session_scope = SessionScope::CurrentProject;
    let value = serde_json::to_value(original.snapshot()).unwrap();
    let mut app = Workspace::default();
    app.enable_store("/unused-feed-test/workspace.json".into());
    let load = app.commands()[0].0;
    app.apply(Event::Completed(
        load,
        Ok(Reply::Snapshot(Some(Box::new(
            serde_json::from_value(value.clone()).unwrap(),
        )))),
    ));
    assert_eq!(app.preferences.session_scope, SessionScope::CurrentProject);
    assert_eq!(app.session_feed(None, "", false), [2, 0]);
    let mut legacy = value;
    legacy["preferences"]
        .as_object_mut()
        .unwrap()
        .remove("sessionScope");
    for session in legacy["sessions"].as_array_mut().unwrap() {
        session.as_object_mut().unwrap().remove("last_activity");
    }
    let legacy: Snapshot = serde_json::from_value(legacy).unwrap();
    assert_eq!(legacy.preferences.session_scope, SessionScope::AllProjects);
    assert!(legacy.sessions.iter().all(|s| s.last_activity.is_none()));
}

#[test]
fn selection_refresh_and_streamed_tokens_do_not_change_recency_but_turn_boundaries_do() {
    let mut app = workspace();
    app.select(1);
    assert_eq!(app.sessions[0].last_activity, Some(1000));
    assert!(app.send(0, "Start a turn".into()));
    let started = app.sessions[0].last_activity;
    assert!(started > Some(1002));
    let event = |method: &str, params| {
        Event::Codex(caelum_protocol::CodexEvent {
            project_path: "/alpha".into(),
            method: method.into(),
            params,
            request_id: None,
        })
    };
    for text in ["one", "two", "three"] {
        app.apply(event(
            "item/agentMessage/delta",
            json!({"threadId":"shared-id","itemId":"answer","delta":text}),
        ));
        assert_eq!(app.sessions[0].last_activity, started);
        assert_eq!(app.session_feed(None, "", false), [0, 2, 1]);
    }
    let send = app
        .commands()
        .into_iter()
        .find(|(_, c)| matches!(c, Command::Send(..)))
        .unwrap()
        .0;
    app.apply(Event::Completed(send, Ok(Reply::Done)));
    app.apply(event(
        "turn/completed",
        json!({"threadId":"shared-id", "turn":{"id":"turn"}}),
    ));
    assert!(app.sessions[0].last_activity >= started);

    let resume = app
        .commands()
        .into_iter()
        .find(|(_, c)| matches!(c, Command::Resume(..)))
        .unwrap()
        .0;
    let key = app.sessions[0].key().unwrap();
    let summary = Summary {
        key,
        updated_at: Some(42),
        title: String::new(),
        working: false,
        options: Options::default(),
    };
    let completed = app.sessions[0].last_activity;
    app.apply(Event::Completed(resume, Ok(Reply::Session(summary))));
    assert_eq!(app.sessions[0].last_activity, completed);
}

#[test]
fn discovery_retains_provider_recency_and_external_opencode_turns_bump_only_at_boundaries() {
    let mut app = Workspace::default();
    app.projects.push(caelum_protocol::ProjectInfo {
        path: "/alpha".into(),
        name: "Alpha".into(),
    });
    app.discover_open_sessions();
    let commands = app.commands();
    let at = 1_700_000_000_000;
    for (request, command) in commands {
        if let Command::List(scope) = command {
            let updated_at = Some(
                at + if scope.provider == Provider::Codex {
                    0
                } else {
                    1000
                },
            );
            app.apply(Event::Completed(
                request,
                Ok(Reply::Sessions(vec![Summary {
                    key: SessionKey {
                        scope,
                        id: "same-id".into(),
                    },
                    updated_at,
                    title: "Discovered".into(),
                    working: false,
                    options: Options::default(),
                }])),
            ));
        }
    }
    let index = app
        .sessions
        .iter()
        .position(|s| s.scope.provider == Provider::OpenCode)
        .unwrap();
    assert_eq!(app.sessions[index].last_activity, Some(at + 1000));
    let event = |kind: &str, created, delta| {
        Event::OpenCode(caelum_protocol::OpenCodeEvent {
            project_path: "/alpha".into(),
            event_type: kind.into(),
            data: json!({"sessionID":"same-id", "assistantMessageID":"answer", "delta":delta}),
            id: None,
            created: Some(created),
            location: None,
        })
    };
    app.apply(event("session.text.delta", at + 60_000, "First"));
    assert_eq!(app.sessions[index].last_activity, Some(at + 60_000));
    app.apply(event("session.text.delta", at + 120_000, " token"));
    assert_eq!(app.sessions[index].last_activity, Some(at + 60_000));
    app.apply(event("session.execution.succeeded", at + 180_000, ""));
    assert_eq!(app.sessions[index].last_activity, Some(at + 180_000));
    app.apply(event("session.execution.succeeded", at + 240_000, ""));
    assert_eq!(app.sessions[index].last_activity, Some(at + 180_000));
}

#[test]
fn pull_request_refreshes_include_linked_checkouts_coalesce_and_keep_late_results_scoped() {
    let mut app = workspace();
    for path in ["/alpha", "/beta", "/linked"] {
        app.services.checkouts.entry(path.into()).or_default().git =
            Some(caelum_protocol::GitRepositoryStatus {
                project_path: path.into(),
                repository_root: Some(path.into()),
                branch: Some("feature".into()),
                changes: vec![],
            });
    }
    app.sessions[0].draft = "Keep my draft".into();
    app.sessions[0].reading = 240.0;
    app.preferences.session_scope = SessionScope::CurrentProject;
    app.refresh_feed_pull_requests(None);
    let first = app.commands();
    assert_eq!(first.len(), 4);
    assert!(first.iter().all(|(_, command)| matches!(command, Command::Task(task) if ["/alpha", "/linked"].contains(&task.project()))));
    app.refresh_feed_pull_requests(None);
    assert!(
        app.commands().is_empty(),
        "in-flight refreshes should coalesce"
    );

    app.select(1);
    app.refresh_feed_pull_requests(None);
    let second = app.commands();
    assert_eq!(second.len(), 2);
    assert!(second
        .iter()
        .all(|(_, command)| matches!(command, Command::Task(task) if task.project() == "/beta")));
    for (request, command) in second.into_iter().chain(first) {
        let reply = match command {
            Command::Task(Task::Git { project }) => TaskReply::Git(
                app.services.checkouts[&project].git.clone().unwrap(),
                vec![],
            ),
            Command::Task(Task::Discover { project, branch }) => {
                assert_eq!(branch, "feature");
                TaskReply::Discovered(Some(caelum_protocol::GitHubPullRequest {
                    number: 42,
                    title: project.clone(),
                    url: format!("https://github.com/example{project}/pull/42"),
                    is_draft: false,
                    base_ref_name: String::new(),
                    review_decision: None,
                }))
            }
            _ => panic!("unexpected refresh command"),
        };
        app.apply(Event::Completed(request, Ok(Reply::Task(reply))));
    }
    assert_eq!(app.selected, Some(1));
    assert_eq!(app.active_project.as_deref(), Some("/beta"));
    assert_eq!(app.sessions[0].draft, "Keep my draft");
    assert_eq!(app.sessions[0].reading, 240.0);
    assert_eq!(
        app.services.checkouts["/alpha"].pull_requests[0].title,
        "/alpha"
    );
    assert_eq!(
        app.services.checkouts["/beta"].pull_requests[0].title,
        "/beta"
    );
    assert!(!app.feed_includes_project("/alpha", None));
    assert!(app.feed_includes_project("/linked", Some("/alpha")));
    assert!(
        app.commands().is_empty(),
        "unchanged Git branches do not discover by themselves"
    );

    app.preferences.session_scope = SessionScope::AllProjects;
    app.close_project("/beta");
    app.refresh_feed_pull_requests(None);
    let refreshed = app.commands();
    assert_eq!(
        refreshed.len(),
        4,
        "refresh must explicitly re-query unchanged branches"
    );
    assert!(refreshed
        .iter()
        .all(|(_, command)| matches!(command, Command::Task(task) if task.project() != "/beta")));
    assert!(!app.feed_includes_project("/beta", None));
}

#[test]
fn dashboard_buckets_and_activity_histogram_reflect_session_lifecycle() {
    let mut app = workspace();
    let now = 2_000_000_000_000;
    app.sessions[0].last_activity = Some(now - 60_000);
    app.sessions[1].last_activity = Some(now - 2 * 86_400_000);
    app.sessions[2].last_activity = Some(now - 16 * 86_400_000);
    app.sessions[0].conversation.set_working(true);
    app.questions.push(caelum_protocol::QuestionPrompt {
        provider: "codex".into(),
        project_path: "/beta".into(),
        session_id: "shared-id".into(),
        request_id: "q-1".into(),
        title: "Question".into(),
        fields: vec![],
    });
    assert_eq!(
        app.session_bucket(&FeedSession::Local(0)),
        SessionBucket::Running
    );
    assert_eq!(
        app.session_bucket(&FeedSession::Local(1)),
        SessionBucket::Waiting
    );
    assert_eq!(
        app.session_bucket(&FeedSession::Local(2)),
        SessionBucket::Settled
    );
    assert_eq!(
        app.dashboard_sessions("", false),
        [0, 1, 2].map(FeedSession::Local)
    );
    let hist = app.activity_histogram(now);
    assert_eq!(hist.total, 2);
    assert_eq!(hist.days[13], 1);
    assert_eq!(hist.days[11], 1);
    assert_eq!(hist.delta_percent, None);
    for _ in 0..4 {
        let i = app.new_session(Scope {
            project: "/alpha".into(),
            provider: Provider::Codex,
        });
        app.sessions[i].last_activity = Some(now - 16 * 86_400_000);
    }
    let hist = app.activity_histogram(now);
    assert_eq!(hist.delta_percent, Some(-60));
}
