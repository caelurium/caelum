use serde::{Deserialize, Serialize};

use crate::GitHubReviewComment;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GitHubReviewEvent {
    #[default]
    Comment,
    Approve,
    RequestChanges,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPendingReviewComment {
    pub path: String,
    pub line: u32,
    pub side: GitHubDiffSide,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum GitHubDiffSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubReviewDraft {
    pub event: GitHubReviewEvent,
    pub body: String,
    pub comments: Vec<GitHubPendingReviewComment>,
}

impl GitHubReviewDraft {
    pub fn can_submit(&self) -> bool {
        self.comments
            .iter()
            .all(|c| c.line > 0 && !c.body.trim().is_empty() && !c.path.is_empty())
            && match self.event {
                GitHubReviewEvent::Approve => true,
                GitHubReviewEvent::Comment => {
                    !self.body.trim().is_empty() || !self.comments.is_empty()
                }
                GitHubReviewEvent::RequestChanges => !self.body.trim().is_empty(),
            }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GitHubReviewWrite {
    Reply { thread_id: String, body: String },
    SetResolved { thread_id: String, resolved: bool },
    Submit(GitHubReviewDraft),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum GitHubReviewWriteResult {
    Replied(GitHubReviewComment),
    Resolution {
        resolved: bool,
        can_resolve: bool,
        can_unresolve: bool,
    },
    Submitted,
}
