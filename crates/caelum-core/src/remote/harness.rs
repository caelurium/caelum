use super::{Continuation, Target};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VmProvider {
    #[default]
    Codex,
    Pi,
    OpenCode,
    Antigravity,
}

impl VmProvider {
    pub const CHOICES: [Self; 2] = [Self::Codex, Self::Pi];

    pub fn name(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Pi => "Pi",
            Self::OpenCode => "OpenCode",
            Self::Antigravity => "Antigravity",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Pi => "pi",
            Self::OpenCode => "opencode",
            Self::Antigravity => "antigravity",
        }
    }

    pub(crate) fn native(self) -> Option<crate::application::Provider> {
        use crate::application::Provider;
        match self {
            Self::Codex => Some(Provider::Codex),
            Self::OpenCode => Some(Provider::OpenCode),
            Self::Antigravity => Some(Provider::Antigravity),
            Self::Pi => None,
        }
    }
}

// All managed VM conversations saved before provider selection used OpenCode.
fn opencode() -> VmProvider {
    VmProvider::OpenCode
}

/// API keys and native provider sessions never enter this record. The cursor is
/// advanced atomically with its visible messages in the workspace snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub awaiting_native: bool,
    /// A fresh session needs an editor identity before Boat assigns a conversation ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continued: Option<Continuation>,
    #[serde(default = "opencode")]
    pub provider: VmProvider,
    /// Explicit Boat model ID; `None` delegates to this harness's Boat settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// An advertised reasoning level for this exact provider/model pair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Feed recency, in Unix milliseconds. Older snapshots have no timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_activity: Option<u64>,
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
            awaiting_native: false,
            continued: None,
            provider: VmProvider::Codex,
            model: None,
            effort: None,
            last_activity: None,
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
    pub provider: VmProvider,
    pub conversation: String,
    pub prompt: String,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Speaker {
    You,
    Codex,
    OpenCode,
    Pi,
    Antigravity,
    Activity,
    ReasoningSummary,
    Tool(ToolState),
    Outcome(TurnEnd),
}

impl Speaker {
    pub fn is_assistant(self) -> bool {
        matches!(
            self,
            Self::Codex | Self::OpenCode | Self::Pi | Self::Antigravity
        )
    }
}

impl From<VmProvider> for Speaker {
    fn from(provider: VmProvider) -> Self {
        match provider {
            VmProvider::Codex => Self::Codex,
            VmProvider::OpenCode => Self::OpenCode,
            VmProvider::Pi => Self::Pi,
            VmProvider::Antigravity => Self::Antigravity,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_source: Option<Box<super::ToolSource>>,
    pub speaker: Speaker,
    pub text: String,
    pub partial: bool,
}

impl Line {
    pub fn new(speaker: Speaker, text: impl Into<String>) -> Self {
        Self {
            key: None,
            tool_detail: None,
            tool_source: None,
            speaker,
            text: text.into(),
            partial: false,
        }
    }
}

impl Thread {
    pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;

    pub fn can_prompt_provider(&self) -> bool {
        VmProvider::CHOICES.contains(&self.provider) || self.conversation.is_some()
    }

    pub(crate) fn for_checkout() -> Self {
        Self {
            last_activity: Some(crate::application::presentation::now()),
            ..Self::default()
        }
    }

    pub(crate) fn record_activity(&mut self) {
        let now = crate::application::presentation::now();
        self.last_activity = Some(now.max(self.last_activity.unwrap_or_default()));
    }

    pub fn editor_key<'a>(&'a self, checkout: &'a str) -> &'a str {
        self.local_id
            .as_deref()
            .or_else(|| self.continued.as_ref().map(|c| c.id.as_str()))
            .unwrap_or(checkout)
    }

    pub fn is_blank(&self) -> bool {
        self.continued.is_none()
            && self.model.is_none()
            && self.effort.is_none()
            && self.conversation.is_none()
            && self.cursor.is_none()
            && self.turn == Turn::Idle
            && self.lines.is_empty()
            && self.draft.is_empty()
    }

    /// A handoff replaces the checkout's initial empty editor without parking a phantom row.
    pub(super) fn is_placeholder(&self) -> bool {
        self.local_id.is_none() && self.is_blank()
    }

    pub fn title(&self) -> &str {
        self.heading()
            .or_else(|| Self::title_line(&self.draft))
            .unwrap_or(if self.conversation.is_none() {
                "New VM session"
            } else {
                "VM conversation"
            })
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
            .and_then(Self::title_line)
    }

    fn title_line(s: &str) -> Option<&str> {
        s.lines()
            .next()
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
                (line.speaker.is_assistant() && (!line.partial || line.key.is_none()))
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
            if (old.speaker.is_assistant() || old.speaker == Speaker::ReasoningSummary)
                && !old.partial
                && line.partial
                || matches!(
                    old.speaker,
                    Speaker::Tool(ToolState::Complete | ToolState::Failed)
                ) && matches!(
                    line.speaker,
                    Speaker::Tool(ToolState::Running | ToolState::Reported)
                )
            {
                return;
            }
            if line.key.is_none() {
                line.key.clone_from(&old.key);
            }
            if matches!(line.speaker, Speaker::Tool(_)) && line.tool_detail.is_none() {
                line.tool_detail.clone_from(&old.tool_detail);
            }
            if matches!(line.speaker, Speaker::Tool(_) | Speaker::ReasoningSummary)
                && line.text.is_empty()
            {
                line.text.clone_from(&old.text);
            }
            *old = line;
        } else {
            if matches!(line.speaker, Speaker::Tool(_)) && line.text.is_empty() {
                line.text = "Tool".into();
            }
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
                    + l.tool_detail.as_ref().map_or(0, String::len)
                    + l.tool_source.as_deref().map_or(0, super::ToolSource::bytes);
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

/// Captured once, on explicit Send. Neither a model choice nor a conversation
/// ID needs to be resolved by the worker.
#[derive(Clone, Debug)]
pub struct Prompt {
    pub provider: VmProvider,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub conversation: Option<String>,
    pub text: String,
}

pub(super) trait Harness {
    async fn send(&self, target: &Target, prompt: &Prompt) -> Result<super::Outcome, String>;
    async fn follow(
        &self,
        target: &Target,
        run: &Run,
        cursor: Option<&str>,
    ) -> Result<Page, String>;
    async fn interrupt(&self, target: &Target, run: &Run) -> Result<(), String>;
}
