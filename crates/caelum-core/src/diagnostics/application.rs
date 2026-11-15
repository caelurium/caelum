use super::{anonymous, enabled, mark, Mark};
use crate::application::{Command, Event, Session, SessionKey};
use caelum_protocol::OpenCodeEvent;

fn key_owner(key: &SessionKey) -> u64 {
    anonymous((key.scope.provider.as_str(), &key.scope.project, &key.id))
}

pub fn session_owner(session: &Session) -> u64 {
    if !enabled() {
        return 0;
    }
    anonymous((
        session.scope.provider.as_str(),
        &session.scope.project,
        session.remote_id.as_deref().unwrap_or(""),
    ))
}

pub(crate) fn command(phase: &'static str, id: u64, command: &Command) {
    if !enabled() {
        return;
    }
    let (label, key) = match command {
        Command::Resume(key) => ("resume", Some(key)),
        Command::History(key, _) => ("history", Some(key)),
        Command::Send(key, ..) => ("send", Some(key)),
        Command::Enqueue(key, ..) => ("enqueue", Some(key)),
        Command::Cancel(key) => ("cancel", Some(key)),
        Command::Inbox(key) => ("inbox", Some(key)),
        Command::Shells(key) => ("shells", Some(key)),
        Command::Details(key) => ("details", Some(key)),
        Command::Start(..) => ("start", None),
        Command::Save(..) => ("save", None),
        Command::Load(..) => ("load", None),
        Command::List(_) => ("list", None),
        Command::ListArchived(_) => ("archived", None),
        Command::Choices(_) => ("choices", None),
        Command::Commands(_) => ("commands", None),
        Command::Task(_) => ("task", None),
        _ => ("other", None),
    };
    if let Some(mark) = mark("request", label, id, key.map_or(0, key_owner)) {
        mark.record(phase, 0);
    }
}

pub fn event(event: &Event) -> Option<Mark> {
    if !enabled() {
        return None;
    }
    match event {
        Event::Completed(id, _) => mark("request", "completion", *id, 0),
        Event::OpenCode(event) => opencode_event(event),
        _ => None,
    }
}

pub(crate) fn opencode_event(event: &OpenCodeEvent) -> Option<Mark> {
    if !enabled() {
        return None;
    }
    let label = match event.event_type.as_str() {
        "session.text.delta" => "text",
        "session.reasoning.delta" => "reasoning",
        "session.execution.started" => "execution.started",
        "session.execution.succeeded" => "execution.succeeded",
        "session.execution.failed" => "execution.failed",
        "session.execution.interrupted" => "execution.interrupted",
        "session.tool.called" => "tool.called",
        "session.tool.success" => "tool.success",
        "session.tool.failed" => "tool.failed",
        "session.step.started" => "step.started",
        "session.step.first-token" => "step.first-token",
        "session.step.ended" => "step.ended",
        _ => return None,
    };
    let session = event.data["sessionID"].as_str()?;
    let mut mark = mark(
        "provider",
        label,
        anonymous((&event.project_path, event.id.as_deref()?)),
        anonymous(("opencode", &event.project_path, session)),
    )?;
    // Tool IDs are scoped to their session. Neither IDs nor tool names enter the trace.
    mark.parent = event.data["toolCallID"]
        .as_str()
        .or_else(|| event.data["id"].as_str())
        .or_else(|| event.data["assistantMessageID"].as_str())
        .map_or(0, |id| anonymous((mark.owner, id)));
    Some(mark)
}
