use super::*;
use crate::templates::{Catalog, Target, Template};

fn templates(body: &str) -> Vec<Template> {
    vec![Template::parse("explain".into(), body.into()).unwrap()]
}
fn discovery(app: &mut Workspace, path: &str) -> u64 {
    app.commands()
        .into_iter()
        .find_map(|(r, c)| {
            matches!(c,Command::Task(Task::Templates {project}) if project==path).then_some(r)
        })
        .unwrap()
}

#[test]
fn invalidated_and_out_of_order_catalogues_never_replace_the_active_checkout() {
    let mut app = Workspace::default();
    let owner = Target::Checkout("/a".into());
    let other = Target::Checkout("/b".into());
    app.show_templates(Some(other.clone()));
    let old = discovery(&mut app, "/b");
    app.show_templates(Some(owner.clone()));
    let first = discovery(&mut app, "/a");
    app.apply(Event::Checkout("/a".into()));
    app.apply(Event::Checkout("/a".into()));
    let complete =
        |r, body| Event::Completed(r, Ok(Reply::Task(TaskReply::Templates(templates(body)))));
    app.apply(complete(first, "superseded"));
    let latest = discovery(&mut app, "/a");
    app.apply(complete(latest, "current $1"));
    app.apply(complete(old, "wrong checkout"));
    assert_eq!(
        app.templates
            .expand(&owner, "/explain one")
            .unwrap()
            .as_deref(),
        Some("current one")
    );
    assert!(app.templates.items(&other).is_empty());
    assert!(app.commands().is_empty());
}

#[test]
fn templates_are_ordinary_messages_across_providers_including_leading_slashes() {
    for provider in Provider::ALL {
        let mut app = Workspace::default();
        let scope = scope(provider, "/project");
        let i = app.new_session(scope.clone());
        app.templates = Catalog::Ready {
            target: Target::Checkout(scope.project.clone()),
            items: templates("/new $1: $ARGUMENTS"),
        };
        app.sessions[i].options.model = Some("example/model".into());
        assert!(app.send(i, "/explain '$1'".into()));
        let (r, Command::Start(_, _)) = app.commands().pop().unwrap() else {
            panic!("expected session creation")
        };
        app.show_templates(None);
        app.apply(Event::Completed(
            r,
            Ok(Reply::Session(summary(scope, "thread"))),
        ));
        let (send, c) = app.commands().pop().unwrap();
        let Command::Send(_, text, options) = c else {
            panic!("unexpected send: {c:?}")
        };
        assert_eq!(text, "/new $1: '$1'");
        assert_eq!(options.model.as_deref(), Some("example/model"));
        assert_eq!(app.sessions[i].conversation.entries[0].text, text);
        app.apply(Event::Completed(send, Err("offline".into())));
        assert_eq!(
            app.sessions[i].failed_prompt.as_deref(),
            Some("/explain '$1'")
        );
    }
}

#[test]
fn queues_capture_the_rendered_text_with_attachments_and_preserve_literal_dispatch() {
    let mut app = Workspace::default();
    let owner = Target::Checkout("/p".into());
    let i = started(&mut app, scope(Provider::OpenCode, "/p"), "session");
    app.sessions[i].queue_paused = true;
    app.attach(
        i,
        Attachment::new("note.md".into(), "snapshot".into(), None, false),
    );
    app.templates = Catalog::Ready {
        target: owner.clone(),
        items: templates("/models $1"),
    };
    assert!(app.send(i, "/explain café".into()));
    app.templates = Catalog::Ready {
        target: owner,
        items: templates("changed"),
    };
    let queued = &app.snapshot().sessions[i].queue[0];
    assert!(!queued.provider_command);
    let (text, files) = split_context(&queued.text);
    assert_eq!(text, "/models café");
    assert_eq!(files[0].content, "snapshot");
}
