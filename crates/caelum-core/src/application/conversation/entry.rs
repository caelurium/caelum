use std::borrow::Cow;
use std::collections::BTreeMap;

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    User,
    Assistant,
    Reasoning,
    Compaction,
    Tool,
    Status,
}

#[derive(Debug, Clone, Copy)]
enum User {
    Optimistic,
    Confirmed,
}

#[derive(Debug, Clone, Copy)]
enum Placement {
    Foreground,
    Background,
}

#[derive(Debug, Clone)]
enum State {
    User(User),
    Assistant(Progress),
    Reasoning(Progress),
    Compaction(Progress),
    Tool {
        placement: Placement,
        progress: Progress,
    },
    Status,
}

#[derive(Debug, Clone)]
enum Progress {
    Running(Cow<'static, str>),
    Finished(Cow<'static, str>),
}

impl Progress {
    fn new(complete: bool, label: Cow<'static, str>) -> Self {
        if complete {
            Self::Finished(label)
        } else {
            Self::Running(label)
        }
    }

    fn complete(&self) -> bool {
        matches!(self, Self::Finished(_))
    }

    fn label(&self) -> &str {
        match self {
            Self::Running(label) | Self::Finished(label) => label,
        }
    }
}

impl State {
    fn new(kind: EntryKind, complete: bool) -> Self {
        let progress = || {
            if complete {
                Progress::Finished("completed".into())
            } else {
                Progress::Running("running".into())
            }
        };
        match kind {
            EntryKind::User => Self::User(User::Confirmed),
            EntryKind::Assistant => Self::Assistant(progress()),
            EntryKind::Reasoning => Self::Reasoning(progress()),
            EntryKind::Compaction => Self::Compaction(progress()),
            EntryKind::Tool => Self::Tool {
                placement: Placement::Foreground,
                progress: progress(),
            },
            EntryKind::Status => Self::Status,
        }
    }

    fn progress(&self) -> Option<&Progress> {
        match self {
            Self::Assistant(progress)
            | Self::Reasoning(progress)
            | Self::Compaction(progress)
            | Self::Tool { progress, .. } => Some(progress),
            Self::User(_) | Self::Status => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: String,
    state: State,
    pub text: String,
    pub name: String,
    pub input: String,
    pub output: String,
    pub started_at: Option<u64>,
    pub metadata: Value,
    pub error: String,
    pub media: Vec<crate::application::presentation::Media>,
    pub(super) history: String,
    pub(super) stream: String,
    pub(super) history_output: String,
    pub(super) stream_output: String,
    pub(super) reasoning: BTreeMap<(bool, u64), String>,
}

impl Entry {
    pub fn kind(&self) -> EntryKind {
        match self.state {
            State::User(_) => EntryKind::User,
            State::Assistant(_) => EntryKind::Assistant,
            State::Reasoning(_) => EntryKind::Reasoning,
            State::Compaction(_) => EntryKind::Compaction,
            State::Tool { .. } => EntryKind::Tool,
            State::Status => EntryKind::Status,
        }
    }

    pub fn complete(&self) -> bool {
        self.state.progress().is_none_or(Progress::complete)
    }

    pub fn optimistic(&self) -> bool {
        matches!(self.state, State::User(User::Optimistic))
    }

    pub fn background(&self) -> bool {
        matches!(
            self.state,
            State::Tool {
                placement: Placement::Background,
                ..
            }
        )
    }

    pub fn status(&self) -> &str {
        self.state.progress().map_or("completed", Progress::label)
    }

    pub(super) fn pending_user(id: String, text: String) -> Self {
        let mut entry = Self::new(id, EntryKind::User, text, true);
        entry.state = State::User(User::Optimistic);
        entry
    }

    pub(super) fn set_background(&mut self) {
        if let State::Tool { placement, .. } = &mut self.state {
            *placement = Placement::Background;
        }
    }

    pub(super) fn set_progress(&mut self, complete: bool, label: impl Into<Cow<'static, str>>) {
        match &mut self.state {
            State::Assistant(progress)
            | State::Reasoning(progress)
            | State::Compaction(progress)
            | State::Tool { progress, .. } => *progress = Progress::new(complete, label.into()),
            State::User(_) | State::Status => {}
        }
    }

    pub(super) fn settle(&mut self) {
        if self.background() || self.state.progress().is_none() {
            return;
        }
        let label = match (self.kind(), self.status()) {
            (EntryKind::Compaction, "running" | "pending" | "in_progress") => "interrupted".into(),
            (_, "running") => "completed".into(),
            _ if self.complete() => return,
            (_, label) => Cow::Owned(label.to_owned()),
        };
        self.set_progress(true, label);
    }

    pub fn new(id: String, kind: EntryKind, text: String, complete: bool) -> Self {
        let media = crate::application::presentation::markdown_media(&text);
        Self {
            media,
            id,
            state: State::new(kind, complete),
            text,
            name: String::new(),
            input: String::new(),
            output: String::new(),
            started_at: None,
            metadata: Value::Null,
            error: String::new(),
            history: String::new(),
            stream: String::new(),
            history_output: String::new(),
            stream_output: String::new(),
            reasoning: BTreeMap::new(),
        }
    }
}
