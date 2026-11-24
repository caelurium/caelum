use super::*;
use caelum_protocol::{ApprovalDecision, ApprovalPrompt, ApprovalRule};

fn pending(owner: &SessionKey, token: &str, rule: &ApprovalRule) -> ApprovalPrompt {
    ApprovalPrompt {
        attempt: token.into(),
        repeatable: Some(rule.clone()),
        provider: owner.scope.provider.as_str().into(),
        project_path: owner.scope.project.clone(),
        session_id: Some(owner.id.clone()),
        request_id: "reused".into(),
        summary: "Request".into(),
        description: String::new(),
    }
}
fn decisions(app: &mut Workspace) -> Vec<(u64, ApprovalPrompt, ApprovalDecision)> {
    app.commands()
        .into_iter()
        .filter_map(|(r, c)| match c {
            Command::Approve(p, d) => Some((r, p, d)),
            _ => None,
        })
        .collect()
}

#[test]
fn grants_are_session_scoped_one_shot_audited_and_revocable() {
    let mut app = Workspace::default();
    let keys = [
        (Provider::Codex, "boat://checkout/first", "one"),
        (Provider::Codex, "boat://checkout/second", "one"),
        (Provider::Codex, "boat://checkout/first", "two"),
        (Provider::OpenCode, "/local", "one"),
        (Provider::Codex, "/local", "one"),
    ]
    .map(|(p, path, id)| {
        let i = app.new_session(scope(p, path));
        app.sessions[i].remote_id = Some(id.into());
        app.sessions[i].key().unwrap()
    });
    let owner = &keys[0];
    let rule = ApprovalRule::ExactCommand("git status".into());
    let first = pending(owner, "1", &rule);
    app.apply(Event::Approvals(vec![first.clone()]));
    app.allow_similar(first.clone());
    let [(r, _, ApprovalDecision::AllowOnce)] = decisions(&mut app)[..] else {
        panic!("expected one-shot grant")
    };
    assert!(app.approval_rules(owner).is_empty());
    app.apply(Event::Completed(r, Ok(Reply::Done)));
    assert_eq!(app.approval_rules(owner), std::slice::from_ref(&rule));
    assert!(decisions(&mut app).is_empty());
    let second = pending(owner, "2", &rule);
    let mut all = vec![second.clone()];
    all.extend(
        keys[1..]
            .iter()
            .enumerate()
            .map(|(i, k)| pending(k, &format!("other-{i}"), &rule)),
    );
    let mut altered = pending(owner, "unrelated", &ApprovalRule::ExactCommand("ls".into()));
    altered.request_id = "different".into();
    all.push(altered.clone());
    app.apply(Event::Approvals(all.clone()));
    let auto = decisions(&mut app);
    assert_eq!(auto.len(), 1);
    assert_eq!(
        (&auto[0].1, auto[0].2),
        (&second, ApprovalDecision::AllowOnce)
    );
    app.apply(Event::Approvals(all));
    assert!(decisions(&mut app).is_empty());
    app.apply(Event::Completed(auto[0].0, Ok(Reply::Done)));
    assert_eq!(
        app.sessions[0].conversation.entries.last().unwrap().text,
        rule.to_string()
    );
    assert!(app.sessions[1..]
        .iter()
        .all(|s| s.conversation.entries.is_empty()));

    app.allow_similar(altered.clone());
    let (r, _, _) = decisions(&mut app).pop().unwrap();
    app.apply(Event::Completed(r, Ok(Reply::Done)));
    assert_eq!(app.approval_rules(owner).len(), 2);
    app.revoke_approval_rules(owner, Some(&rule));
    assert_eq!(app.approval_rules(owner), [altered.repeatable.unwrap()]);
    app.apply(Event::Approvals(vec![pending(owner, "3", &rule)]));
    assert!(decisions(&mut app).is_empty());
    assert!(!serde_json::to_string(&app.snapshot())
        .unwrap()
        .contains("ExactCommand"));
    app.revoke_approval_rules(owner, None);
    assert!(app.approval_rules(owner).is_empty());
}

#[test]
fn failed_grants_revocation_and_manual_decisions_win_over_late_replies() {
    let mut app = Workspace::default();
    let slot = app.new_session(scope(Provider::OpenCode, "/a"));
    app.sessions[slot].remote_id = Some("session".into());
    let owner = app.sessions[slot].key().unwrap();
    let rule = ApprovalRule::ReadFile("README.md".into());
    for success in [false, true] {
        let p = pending(&owner, "original", &rule);
        app.apply(Event::Approvals(vec![p.clone()]));
        app.allow_similar(p);
        let (r, _, _) = decisions(&mut app).pop().unwrap();
        app.revoke_approval_rules(&owner, None);
        app.apply(Event::Completed(
            r,
            if success {
                Ok(Reply::Done)
            } else {
                Err("expired".into())
            },
        ));
        assert!(app.approval_rules(&owner).is_empty());
    }
    let grant = pending(&owner, "grant", &rule);
    let mut deny = pending(&owner, "deny", &rule);
    deny.request_id = "second".into();
    app.apply(Event::Approvals(vec![grant.clone(), deny.clone()]));
    app.allow_similar(grant);
    app.approve(deny, ApprovalDecision::Deny);
    let replies = decisions(&mut app);
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[1].2, ApprovalDecision::Deny);
    app.apply(Event::Completed(replies[0].0, Ok(Reply::Done)));
    assert!(decisions(&mut app).is_empty());
}
