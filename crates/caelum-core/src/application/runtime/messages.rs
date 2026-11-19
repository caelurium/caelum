use crate::application::{
    ForkBoundary, Notification, Options, Scope, SessionKey, Snapshot, Store, Summary, Task,
    TaskReply,
};
use caelum_protocol::{
    ApprovalDecision, ApprovalPrompt, CodexEvent, OpenCodeEvent, ProjectInfo, ProviderChoices,
    QuestionPrompt, SessionHistoryPage,
};

#[derive(Debug, Clone)]
pub enum Command {
    Remote(crate::remote::Operation),
    Inbox(SessionKey),
    Enqueue(SessionKey, String, String),
    RemoveQueued(SessionKey, String),
    Generate(SessionKey, String),
    RegenerateTitle(SessionKey),
    Notify(Notification),
    Sound(u8),
    Task(Task),
    Load(Store),
    Save(Store, Box<Snapshot>),
    Commands(Scope),
    Shells(SessionKey),
    Archive(SessionKey, bool),
    DeleteSession(SessionKey),
    Details(SessionKey),
    Open(String),
    List(Scope),
    ListArchived(Scope),
    Choices(Scope),
    Start(Scope, Options),
    Fork(SessionKey, ForkBoundary),
    Resume(SessionKey),
    VerifyOpenCodeMove {
        from: SessionKey,
        destination: String,
    },
    History(SessionKey, Option<String>),
    HandoffChat(SessionKey),
    Send(SessionKey, String, Options),
    Cancel(SessionKey),
    Approve(ApprovalPrompt, ApprovalDecision),
    Answer(QuestionPrompt, Option<crate::questions::Answer>),
}

#[derive(Debug)]
pub struct OpenCodeMove {
    pub from: SessionKey,
    pub project: ProjectInfo,
    pub archived: bool,
    /// Only a Git-verified sibling checkout may be opened automatically.
    pub linked_worktree: bool,
}

#[derive(Debug)]
pub enum Reply {
    Remote(crate::remote::Outcome),
    Inbox(Vec<serde_json::Value>),
    Generated(String),
    Task(TaskReply),
    Snapshot(Option<Box<Snapshot>>),
    Commands(Vec<caelum_protocol::OpenCodeCommandInfo>),
    Shells(Vec<caelum_protocol::OpenCodeBackgroundShell>),
    Details(caelum_protocol::SessionDetails),
    Project(ProjectInfo),
    Sessions(Vec<Summary>),
    Session(Summary),
    Forked { source: SessionKey, child: Summary },
    Resumed(Summary, SessionHistoryPage),
    SessionMoved(OpenCodeMove),
    Choices(ProviderChoices),
    History(SessionHistoryPage),
    HandoffChat(crate::remote::Chat),
    Done,
}

#[derive(Debug)]
pub enum Event {
    UpdateProgress(u64, crate::updates::Progress),
    Checkout(String),
    Completed(u64, Result<Reply, String>),
    Codex(CodexEvent),
    OpenCode(OpenCodeEvent),
    Approvals(Vec<ApprovalPrompt>),
    Questions(Vec<QuestionPrompt>),
    Branch(caelum_protocol::SessionBranchUpdate),
}
