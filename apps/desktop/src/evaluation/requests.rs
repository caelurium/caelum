use crate::live::Live;
use caelum_core::{
    application::{Command, Event, Reply},
    templates::{Catalog, Target, Template},
};
use caelum_protocol::{ApprovalPrompt, ApprovalRule, QuestionPrompt};
use serde_json::json;

pub(super) fn scene(l: &mut Live, project: &str, name: &str) -> bool {
    if matches!(name, "commands" | "prompt-templates") {
        l.sync_editors();
        let query = if name == "commands" { "/" } else { "/expl" };
        l.drafts[0].replace_text(query);
        l.drafts[0].set_caret(query.len());
        l.drafts[0].focused = true;
        let body = "---\ndescription: Explain a change for a reader\nargument_hint: <change> <audience>\n---\nExplain $1 to $2.";
        l.app.templates = Catalog::Ready {
            target: Target::Checkout(project.into()),
            items: vec![Template::parse("explain".into(), body.into()).unwrap()],
        };
        return true;
    }
    if !matches!(name, "questions" | "approval" | "approval-rules") {
        return false;
    }
    l.app.sessions[0].remote_id = Some("thread-fixture".into());
    l.app.sessions[0].conversation = Default::default();
    if name == "questions" {
        l.app.apply(Event::Questions(vec![QuestionPrompt {provider:"codex".into(),project_path:project.into(),session_id:"thread-fixture".into(),request_id:"fixture".into(),title:"Choose the implementation scope".into(),fields:vec![json!({"key":"target","type":"multiselect","title":"Platforms","required":true,"options":[{"value":"macos","label":"macOS","description":"Native desktop experience"},{"value":"linux","label":"Linux","description":"Shared core and CLI"}],"default":["macos"],"custom":true})] }]));
    } else {
        let command = "cargo test --workspace --locked";
        let mut prompt = ApprovalPrompt {
            provider: "codex".into(),
            project_path: project.into(),
            session_id: Some("thread-fixture".into()),
            request_id: "fixture".into(),
            summary: "Run the workspace checks?".into(),
            description: command.into(),
            attempt: "example-grant".into(),
            repeatable: Some(ApprovalRule::ExactCommand(command.into())),
        };
        l.app.apply(Event::Approvals(vec![prompt.clone()]));
        if name == "approval-rules" {
            l.app.allow_similar(prompt.clone());
            settle(l);
            prompt.attempt = "example-match".into();
            l.app.apply(Event::Approvals(vec![prompt]));
            settle(l);
            l.approval_menu = l.app.sessions[0].key();
        }
    }
    true
}

fn settle(l: &mut Live) {
    let (id, _) = l
        .app
        .commands()
        .into_iter()
        .find(|(_, c)| matches!(c, Command::Approve(..)))
        .unwrap();
    l.app.apply(Event::Approvals(vec![]));
    l.app.apply(Event::Completed(id, Ok(Reply::Done)));
}
