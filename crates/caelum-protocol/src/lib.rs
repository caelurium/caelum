#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

mod github_review;
mod stats;
pub use github_review::GitHubDiffSide;
pub use github_review::GitHubPendingReviewComment;
pub use github_review::GitHubReviewDraft;
pub use github_review::GitHubReviewEvent;
pub use github_review::GitHubReviewWrite;
pub use github_review::GitHubReviewWriteResult;
pub use stats::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateStatus {
    Available,
    UpToDate,
    Ahead,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    pub current_version: String,
    pub latest_version: String,
    pub status: UpdateStatus,
    pub release_url: String,
    pub download_url: Option<String>,
}

/// A project path approved and normalized by the Rust core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub name: String,
    pub path: String,
}

/// A Git checkout and the corresponding directory for this logical project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitWorktree {
    pub path: String,
    pub project: ProjectInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub branch: Option<String>,
    pub main: bool,
    pub locked: Option<String>,
    pub prunable: Option<String>,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitWorktrees {
    pub project: ProjectInfo,
    pub storage_directory: String,
    pub branches: Vec<String>,
    pub default_base: String,
    pub worktrees: Vec<GitWorktree>,
}

/// A changed path reported by Git's porcelain status format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileStatus {
    pub path: String,
    pub index_status: String,
    pub worktree_status: String,
    /// Nanoseconds since the Unix epoch, encoded as text to retain precision in JavaScript.
    #[serde(default)]
    pub modified_at: Option<String>,
    /// Opaque Git/index and working-tree metadata revision used to detect repeated edits.
    #[serde(default)]
    pub revision: String,
}

/// Read-only Git status scoped to the selected project directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRepositoryStatus {
    pub project_path: String,
    pub repository_root: Option<String>,
    pub branch: Option<String>,
    pub changes: Vec<GitFileStatus>,
}

/// An open GitHub pull request for a project's branch (including drafts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPullRequest {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub is_draft: bool,
    #[serde(default)]
    pub base_ref_name: String,
    #[serde(default)]
    pub review_decision: Option<String>,
}

/// On-demand GitHub metadata for a pull request, independent of the checkout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPullRequestDetails {
    #[serde(flatten)]
    pub pull_request: GitHubPullRequest,
    pub body: String,
    pub state: String,
    pub author: Option<GitHubAuthor>,
    pub head_ref_name: String,
    pub head_ref_oid: String,
    #[serde(default)]
    pub base_ref_oid: String,
    pub mergeable: String,
    pub merge_state_status: String,
    pub additions: u64,
    pub deletions: u64,
    pub changed_files: u64,
    pub status_check_rollup: Option<Vec<GitHubStatusCheck>>,
    pub auto_merge_request: Option<GitHubAutoMergeRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubAuthor {
    pub login: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPullRequestFile {
    #[serde(alias = "filename")]
    pub path: String,
    pub status: String,
    #[serde(alias = "previous_filename")]
    pub previous_path: Option<String>,
    pub additions: u64,
    pub deletions: u64,
    pub patch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPullRequestFiles {
    pub files: Vec<GitHubPullRequestFile>,
    pub base_oid: String,
    pub next_page: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GitHubFeedbackKind {
    Discussion,
    Reviews,
    Threads,
    Replies,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubReviewComment {
    pub id: String,
    pub body: String,
    pub url: String,
    pub author: Option<GitHubAuthor>,
    pub created_at: String,
    pub state: Option<String>,
    pub diff_hunk: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubReviewThread {
    pub id: String,
    pub path: String,
    pub line: Option<u64>,
    pub start_line: Option<u64>,
    pub original_line: Option<u64>,
    pub diff_side: String,
    pub is_resolved: bool,
    pub is_outdated: bool,
    #[serde(default)]
    pub viewer_can_reply: bool,
    #[serde(default)]
    pub viewer_can_resolve: bool,
    #[serde(default)]
    pub viewer_can_unresolve: bool,
    pub comments: Vec<GitHubReviewComment>,
    pub next_cursor: Option<String>,
}

impl GitHubReviewThread {
    pub fn can_toggle_resolution(&self) -> bool {
        if self.is_resolved {
            self.viewer_can_unresolve
        } else {
            self.viewer_can_resolve
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPullRequestFeedback {
    pub comments: Vec<GitHubReviewComment>,
    pub threads: Vec<GitHubReviewThread>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubCheckLogs {
    pub run_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<u64>,
    pub check_name: String,
    pub excerpt: String,
    pub truncated: bool,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionOrigin {
    Subagent {
        parent_remote_id: String,
        entry_id: String,
    },
    PullRequestCheck {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_session_id: Option<String>,
        project: String,
        pr_url: String,
        pr_number: u64,
        head_oid: String,
        check_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        run_id: Option<u64>,
    },
    Orchestration {
        orchestration_id: String,
        task_id: String,
    },
}

/// GitHub exposes both Actions check runs and legacy commit statuses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "__typename")]
pub enum GitHubStatusCheck {
    CheckRun {
        name: String,
        status: String,
        conclusion: Option<String>,
        #[serde(rename = "detailsUrl")]
        details_url: Option<String>,
        #[serde(rename = "startedAt")]
        started_at: Option<String>,
        #[serde(rename = "completedAt")]
        completed_at: Option<String>,
    },
    StatusContext {
        context: String,
        state: String,
        #[serde(rename = "targetUrl")]
        target_url: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GitHubMergeMethod {
    #[serde(alias = "MERGE")]
    Merge,
    #[serde(alias = "SQUASH")]
    Squash,
    #[serde(alias = "REBASE")]
    Rebase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubAutoMergeRequest {
    pub merge_method: GitHubMergeMethod,
}

/// Confirmed remote state after changing a pull request's auto-merge setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubAutoMergeResult {
    pub head_ref_oid: String,
    pub state: String,
    pub auto_merge_request: Option<GitHubAutoMergeRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubMergeResult {
    pub merged: bool,
    pub message: String,
}

/// Separate staged, unstaged, and untracked diffs for one changed path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileDiff {
    pub path: String,
    pub staged: String,
    pub unstaged: String,
    pub untracked: String,
}

/// Kind of an entry in the selected project's filesystem tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProjectEntryKind {
    File,
    Directory,
    Symlink,
}

/// A single, non-recursive entry in a project directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFileEntry {
    pub name: String,
    pub path: String,
    pub kind: ProjectEntryKind,
    pub size_bytes: Option<u64>,
}

/// One paginated directory listing; child directories are fetched only when expanded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDirectoryListing {
    pub path: String,
    pub entries: Vec<ProjectFileEntry>,
    pub total: usize,
    pub next_offset: Option<usize>,
}

/// A bounded UTF-8 preview of a file inside the selected project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFileContent {
    pub path: String,
    pub content: String,
    pub size_bytes: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProjectSearchMode {
    Files,
    Content,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSearchMatch {
    pub path: String,
    pub line: Option<usize>,
    /// One-based UTF-16 column, matching the code viewer's coordinate system.
    pub column: Option<usize>,
    pub preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSearchResults {
    pub matches: Vec<ProjectSearchMatch>,
    pub truncated: bool,
    pub skipped: usize,
}

/// Project and session metadata restored from Caelum's local workspace database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedProject {
    pub project: ProjectInfo,
    #[serde(default)]
    pub root_path: Option<String>,
    #[serde(default)]
    pub selected_worktree_path: Option<String>,
    pub provider: String,
    pub codex_session_id: Option<String>,
    pub opencode_session_id: Option<String>,
    pub codex_session_ids: Vec<String>,
    pub opencode_session_ids: Vec<String>,
}

/// App-owned workspace metadata. Provider conversation history is not included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub projects: Vec<PersistedProject>,
    pub active_project_path: Option<String>,
}

/// A provider-specific request for explicit user approval.
///
/// `provider_details` is intentionally opaque to Caelum so adapters can retain
/// provider-specific permission semantics instead of coercing them into a
/// generic permission mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    pub provider: String,
    pub project_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub request_id: String,
    pub summary: String,
    pub description: String,
    pub provider_details: serde_json::Value,
}

/// The selected fields shown to the user for an approval request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalPrompt {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub attempt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeatable: Option<ApprovalRule>,
    pub provider: String,
    pub project_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub request_id: String,
    pub summary: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalRule {
    ExactCommand(String),
    ReadFile(String),
}

impl std::fmt::Display for ApprovalRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExactCommand(cmd) => write!(f, "Run `{cmd}` in the checkout root"),
            Self::ReadFile(path) => write!(f, "Read {path}"),
        }
    }
}

/// Decisions are one-shot: there is deliberately no persistent "allow all".
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalDecision {
    AllowOnce,
    #[default]
    Deny,
}

/// An interactive provider request. Fields retain the OpenCode V2 form shape;
/// Codex question options are adapted to the same presentation contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionPrompt {
    pub provider: String,
    pub project_path: String,
    pub session_id: String,
    pub request_id: String,
    pub title: String,
    pub fields: Vec<serde_json::Value>,
}

/// A Codex thread as returned by the local app-server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexThreadInfo {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub preview: String,
    #[serde(default)]
    pub updated_at: Option<u64>,
    #[serde(default)]
    pub is_working: bool,
}

/// The project and thread selected in the desktop Codex workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionInfo {
    pub project: ProjectInfo,
    pub thread: CodexThreadInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

/// Recent Codex-owned threads scoped to one project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionList {
    pub project: ProjectInfo,
    pub threads: Vec<CodexThreadInfo>,
}

/// A notification or server-initiated request from the Codex app-server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexEvent {
    pub project_path: String,
    pub method: String,
    pub params: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<serde_json::Value>,
}

/// A session owned by the OpenCode HTTP API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeSession {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub updated_at: Option<u64>,
    #[serde(default)]
    pub is_working: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub archived: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !value
}

/// The selected project and OpenCode session shown in the desktop workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeSessionInfo {
    pub project: ProjectInfo,
    pub session: OpenCodeSession,
}

/// Recent OpenCode sessions scoped to one project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeSessionList {
    pub project: ProjectInfo,
    pub sessions: Vec<OpenCodeSession>,
}

/// A running command owned by an OpenCode session, which can outlive its model turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeBackgroundShell {
    pub id: String,
    pub command: String,
    pub started_at: u64,
}

/// One chronological page of provider-owned history, retaining native tool details.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHistoryPage {
    pub data: Vec<serde_json::Value>,
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub is_working: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

/// Compact provider-owned context for a session's sidebar entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetails {
    pub branch: Option<String>,
    /// All branches attributed to this session, including earlier PR branches.
    #[serde(default)]
    pub branches: Vec<String>,
    pub branch_is_current: bool,
    pub changes: Option<LineChanges>,
}

/// A checkout branch change attributed to its currently executing session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBranchUpdate {
    pub project_path: String,
    pub provider: String,
    pub session_id: String,
    pub branch: Option<String>,
    pub branches: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineChanges {
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderChoice {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelChoice {
    pub id: String,
    pub name: String,
    /// The service supplying this model, which may differ from its original vendor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    pub efforts: Vec<ProviderChoice>,
    pub default_effort: Option<String>,
}

/// Choices reported by the installed provider, rather than hard-coded UI options.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderChoices {
    pub models: Vec<ProviderModelChoice>,
    pub agents: Vec<ProviderChoice>,
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenCodeCommandInfo {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenCodeCommandFile {
    pub uri: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenCodeCommandInput {
    pub name: String,
    pub text: String,
    #[serde(default)]
    pub files: Vec<OpenCodeCommandFile>,
}

/// An event received from OpenCode's `/api/event` server-sent event stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeEvent {
    pub project_path: String,
    pub event_type: String,
    pub data: serde_json::Value,
    /// Native event identity and time are needed to reconcile projected compaction messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<serde_json::Value>,
}

/// Summary of an Antigravity ACP session scoped to one project directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravitySession {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub updated_at: Option<u64>,
    #[serde(default)]
    pub is_working: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

/// The selected project and Antigravity ACP session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravitySessionInfo {
    pub project: ProjectInfo,
    pub session: AntigravitySession,
}

/// Recent Antigravity ACP sessions scoped to one project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravitySessionList {
    pub project: ProjectInfo,
    pub sessions: Vec<AntigravitySession>,
}

/// A live notification or lifecycle event emitted by an Antigravity ACP server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AntigravityEvent {
    pub project_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub method: String,
    pub params: serde_json::Value,
}
