use super::{Continuation, Target};
use crate::application::Provider;
use serde::{Deserialize, Serialize};

// All managed VM conversations saved before provider selection used OpenCode.
fn opencode() -> Provider {
    Provider::OpenCode
}

/// API keys and native provider sessions never enter this record. The cursor is
/// advanced atomically with its visible messages in the workspace snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    /// A fresh session needs an editor identity before Boat assigns a conversation ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continued: Option<Continuation>,
    #[serde(default = "opencode")]
    pub provider: Provider,
    pub conversation: Option<String>,
    pub cursor: Option<String>,
    pub turn: Turn,
    pub lines: Vec<Line>,
    pub draft: String,
}

impl Default for Thread {
    fn default() -> Self {
        Self {
            local_id: None,
            continued: None,
            provider: opencode(),
            conversation: None,
            cursor: None,
            turn: Turn::Idle,
            lines: Vec::new(),
            draft: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Turn {
    #[default]
    Idle,
    /// The non-idempotent POST may already have succeeded. Never replay on restore.
    Submitting,
    Queued(Run),
    Following(Run),
    Finishing(Run),
    Stopping(Run),
}

impl Turn {
    pub fn run(&self) -> Option<&Run> {
        match self {
            Self::Queued(run)
            | Self::Following(run)
            | Self::Finishing(run)
            | Self::Stopping(run) => Some(run),
            Self::Idle | Self::Submitting => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    #[serde(default = "opencode")]
    pub provider: Provider,
    pub conversation: String,
    pub prompt: String,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Speaker {
    You,
    Codex,
    OpenCode,
    Activity,
    Tool(ToolState),
    Outcome(TurnEnd),
}

impl Speaker {
    pub fn is_assistant(self) -> bool {
        matches!(self, Self::Codex | Self::OpenCode)
    }
}

impl From<Provider> for Speaker {
    fn from(provider: Provider) -> Self {
        match provider {
            Provider::Codex => Self::Codex,
            Provider::OpenCode => Self::OpenCode,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnEnd {
    Finished,
    Interrupted,
    Failed,
    Other,
}

impl TurnEnd {
    pub fn label(self) -> &'static str {
        match self {
            Self::Finished => "Turn finished",
            Self::Interrupted => "Turn stopped",
            Self::Failed => "Turn failed",
            Self::Other => "Turn ended",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolState {
    Running,
    Complete,
    Failed,
    Reported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// A checkout-relative file or executable name, never the command arguments or result body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_detail: Option<String>,
    pub speaker: Speaker,
    pub text: String,
    pub partial: bool,
}

impl Line {
    pub fn new(speaker: Speaker, text: impl Into<String>) -> Self {
        Self {
            key: None,
            tool_detail: None,
            speaker,
            text: text.into(),
            partial: false,
        }
    }
}

impl Thread {
    pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;

    pub fn editor_key<'a>(&'a self, checkout: &'a str) -> &'a str {
        self.local_id
            .as_deref()
            .or_else(|| self.continued.as_ref().map(|c| c.id.as_str()))
            .unwrap_or(checkout)
    }

    pub(crate) fn is_blank(&self) -> bool {
        self.continued.is_none()
            && self.conversation.is_none()
            && self.cursor.is_none()
            && self.turn == Turn::Idle
            && self.lines.is_empty()
            && self.draft.is_empty()
    }

    pub fn title(&self) -> &str {
        self.heading()
            .or_else(|| self.draft.lines().next())
            .unwrap_or("VM conversation")
    }

    pub fn heading(&self) -> Option<&str> {
        self.continued
            .as_ref()
            .map(|c| c.title.as_str())
            .or_else(|| {
                self.lines
                    .iter()
                    .find(|l| l.speaker == Speaker::You)
                    .map(|l| l.text.as_str())
            })
            .and_then(|s| s.lines().next())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| &s[..s.floor_char_boundary(120.min(s.len()))])
    }

    pub(crate) fn prompt(&self, text: &str) -> String {
        if self.conversation.is_none() {
            if let Some(c) = &self.continued {
                return c.chat.prompt(text);
            }
        }
        text.into()
    }

    pub(crate) fn append(&mut self, mut line: Line) {
        line.text
            .truncate(line.text.floor_char_boundary(Self::MAX_MESSAGE_BYTES));
        let start = self
            .lines
            .iter()
            .rposition(|l| l.speaker == Speaker::You)
            .map_or(0, |i| i + 1);
        let current = &mut self.lines[start..];
        // A tool result updates its call even if text arrived between the two.
        // Some harnesses use a fresh event ID for the final snapshot of a response.
        let update = line
            .key
            .as_ref()
            .and_then(|key| current.iter().position(|l| l.key.as_ref() == Some(key)))
            .or_else(|| {
                line.speaker
                    .is_assistant()
                    .then(|| {
                        current
                            .iter()
                            .rposition(|l| l.speaker == line.speaker && l.partial)
                    })
                    .flatten()
            });
        if let Some(i) = update {
            let old = &mut current[i];
            // A repeated snapshot must not reopen an already finished tool.
            if old.speaker.is_assistant() && !old.partial && line.partial
                || matches!(
                    old.speaker,
                    Speaker::Tool(ToolState::Complete | ToolState::Failed)
                ) && line.speaker == Speaker::Tool(ToolState::Running)
            {
                return;
            }
            if line.key.is_none() {
                line.key.clone_from(&old.key);
            }
            if matches!(line.speaker, Speaker::Tool(_)) && line.tool_detail.is_none() {
                line.tool_detail.clone_from(&old.tool_detail);
            }
            *old = line;
        } else {
            line.key
                .get_or_insert_with(|| uuid::Uuid::new_v4().to_string());
            self.lines.push(line);
        }
        let mut bytes = 0;
        let count = self
            .lines
            .iter()
            .rev()
            .take(200)
            .take_while(|l| {
                bytes += l.text.len()
                    + l.key.as_ref().map_or(0, String::len)
                    + l.tool_detail.as_ref().map_or(0, String::len);
                bytes <= 512 * 1024
            })
            .count();
        self.lines.drain(..self.lines.len() - count);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RunStatus {
    Queued,
    Running,
    Finishing,
    Ended(TurnEnd),
}

#[derive(Debug)]
pub struct Page {
    pub lines: Vec<Line>,
    pub cursor: Option<String>,
    pub more: bool,
    pub status: RunStatus,
}

pub(super) trait Harness {
    async fn send(
        &self,
        target: &Target,
        provider: Provider,
        conversation: Option<&str>,
        text: &str,
    ) -> Result<super::Outcome, String>;
    async fn follow(
        &self,
        target: &Target,
        run: &Run,
        cursor: Option<&str>,
    ) -> Result<Page, String>;
    async fn interrupt(&self, target: &Target, run: &Run) -> Result<(), String>;
}
