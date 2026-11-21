use super::Tool;
use crate::application::{EntryKind, Session};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityKind {
    Working,
    Thinking,
    Responding,
    Tool,
    Retrying,
    Input,
    Background,
    Stopping,
}

pub struct Activity {
    pub kind: ActivityKind,
    pub label: String,
}

/// Activity belongs to a session, not to the most recently painted output chunk.
pub fn activity(session: &Session, needs_input: bool) -> Option<Activity> {
    if session.disconnected {
        return None;
    }
    let (kind, label) = if session.stopping() {
        (ActivityKind::Stopping, "Stopping…".into())
    } else if needs_input {
        (ActivityKind::Input, "Waiting for input".into())
    } else if session.submitting() {
        (ActivityKind::Working, "Working…".into())
    } else if session.conversation.working() {
        if session.conversation.retrying() {
            (ActivityKind::Retrying, "Retrying…".into())
        } else if let Some(entry) = session
            .conversation
            .entries
            .iter()
            .rev()
            .take_while(|e| e.kind() != EntryKind::User)
            .find(|e| !e.complete() && !e.background())
        {
            match entry.kind() {
                EntryKind::Reasoning => (ActivityKind::Thinking, "Thinking…".into()),
                EntryKind::Assistant => (ActivityKind::Responding, "Responding…".into()),
                EntryKind::Tool => {
                    let tool = Tool::from_entry(entry, 0);
                    (
                        ActivityKind::Tool,
                        if tool.name == "Run" {
                            "Running a command…".into()
                        } else {
                            format!("{}…", tool.name)
                        },
                    )
                }
                EntryKind::Compaction => (ActivityKind::Working, "Compacting context…".into()),
                _ => (ActivityKind::Working, "Working…".into()),
            }
        } else if !session.shells.is_empty() {
            (
                ActivityKind::Background,
                "Waiting for background commands".into(),
            )
        } else {
            (ActivityKind::Working, "Working…".into())
        }
    } else if !session.shells.is_empty() {
        (
            ActivityKind::Background,
            "Waiting for background commands".into(),
        )
    } else {
        return None;
    };
    Some(Activity { kind, label })
}
