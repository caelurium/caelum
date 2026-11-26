//! Framework-independent application boundary. Blocking provider work belongs to
//! `Runtime`; `Workspace` owns scoped requests and conversation reconciliation.

pub mod commands;
mod composer;
mod conversation;
mod export;
mod feed;
pub mod forms;
mod inbox;
mod notifications;
mod persistence;
pub mod presentation;
mod runtime;
mod services;
pub mod subagents;
mod workspace;

pub(crate) use composer::provider_prompt;
pub use composer::{
    split_context, Attachment, AttachmentKind, Completion, CompletionKind, QueuedPrompt,
};
pub use conversation::{Conversation, Entry, EntryKind};
pub use export::ExportFormat;
pub use feed::ActivityHistogram;
pub use feed::FeedSession;
pub use feed::OverviewActivity;
pub use feed::SessionBucket;
pub use feed::SessionScope;
pub use inbox::{Inbox, InboxPrompt, SavedInboxPrompt};
pub use notifications::Notification;
pub use persistence::{Preferences, SavedSession, Snapshot, Store, StoreLock};
pub use runtime::{Command, Event, OpenCodeMove, ProviderRuntime, Reply, Runtime};
pub use services::{Checkout, Review, Services, Task, TaskReply};
#[cfg(test)]
use workspace::ArchiveFixture;
pub use workspace::FilePreview;
pub use workspace::ForkBoundary;
pub use workspace::ForkPicker;
pub use workspace::ForkPoint;
pub use workspace::HandoffPhase;
pub use workspace::HandoffSource;
pub use workspace::HistoryAction;
pub use workspace::Home;
pub use workspace::HomeDraft;
pub use workspace::ProjectMove;
pub use workspace::Session;
pub use workspace::VmDestination;
pub use workspace::VmHandoff;
pub use workspace::Workspace;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Codex,
    OpenCode,
    Antigravity,
}

impl Provider {
    pub const ALL: [Self; 3] = [Self::Codex, Self::OpenCode, Self::Antigravity];

    pub fn for_project(project: Option<&str>) -> &'static [Self] {
        if project
            .and_then(crate::remote::native::checkout_id)
            .is_some()
        {
            &[Self::Codex]
        } else {
            &Self::ALL
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::OpenCode => "OpenCode",
            Self::Antigravity => "Antigravity",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Antigravity => "antigravity",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Scope {
    pub project: String,
    pub provider: Provider,
}

impl Scope {
    pub fn provider_available(&self) -> bool {
        Provider::for_project(Some(&self.project)).contains(&self.provider)
    }

    fn for_new_session(mut self) -> Self {
        if !self.provider_available() {
            self.provider = Provider::Codex;
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionKey {
    pub scope: Scope,
    pub id: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodexPermissions {
    #[default]
    Ask,
    AutoWorkspace,
    FullAccess,
}

impl CodexPermissions {
    pub const ALL: [Self; 3] = [Self::Ask, Self::AutoWorkspace, Self::FullAccess];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::AutoWorkspace => "auto-workspace",
            Self::FullAccess => "full-access",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "ask" => Some(Self::Ask),
            "auto-workspace" => Some(Self::AutoWorkspace),
            "full-access" => Some(Self::FullAccess),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ask => "Ask before running",
            Self::AutoWorkspace => "Auto-approve workspace",
            Self::FullAccess => "Full VM access",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Ask => "Ask",
            Self::AutoWorkspace => "Auto (workspace)",
            Self::FullAccess => "Full access",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Ask => "Require approval for commands; write only in workspace",
            Self::AutoWorkspace => "Run commands without asking; write only in workspace",
            Self::FullAccess => "Run commands without asking or sandbox restrictions",
        }
    }

    pub(crate) fn approval_policy(self) -> &'static str {
        match self {
            Self::Ask => "untrusted",
            Self::AutoWorkspace | Self::FullAccess => "never",
        }
    }

    pub(crate) fn sandbox(self) -> &'static str {
        match self {
            Self::Ask | Self::AutoWorkspace => "workspace-write",
            Self::FullAccess => "danger-full-access",
        }
    }

    pub(crate) fn sandbox_policy(self) -> serde_json::Value {
        match self {
            Self::Ask | Self::AutoWorkspace => serde_json::json!({ "type": "workspaceWrite" }),
            Self::FullAccess => serde_json::json!({ "type": "dangerFullAccess" }),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<CodexPermissions>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    /// Provider activity time in Unix milliseconds.
    pub updated_at: Option<u64>,
    pub key: SessionKey,
    pub title: String,
    pub working: bool,
    pub options: Options,
}

#[cfg(test)]
mod parity_tests;
#[cfg(test)]
mod tests;
