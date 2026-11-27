use crate::application::{
    ForkBoundary, Notification, Options, Scope, SessionKey, Snapshot, Store, Summary, Task,
    TaskReply,
};
use caelum_protocol::{
    AntigravityEvent, ApprovalDecision, ApprovalPrompt, CodexEvent, OpenCodeEvent, ProjectInfo,
    ProviderChoices, QuestionPrompt, SessionHistoryPage,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Command {
    #[serde(skip)]
    RunCommand {
        project: String,
        run_id: u64,
        command: String,
    },
    #[serde(skip)]
    StopCommand {
        project: String,
        run_id: u64,
    },
    #[serde(skip)]
    RecordUsage(crate::telemetry::Record),
    #[serde(skip)]
    Remote(crate::remote::Operation),
    Inbox(SessionKey),
    Enqueue(SessionKey, String, String),
    RemoveQueued(SessionKey, String),
    Generate(SessionKey, String),
    #[serde(skip)]
    SideTurn(SessionKey, crate::side_questions::Turn),
    RegenerateTitle(SessionKey),
    Rename(SessionKey, String),
    CheckpointFiles(SessionKey, String, String),
    #[serde(skip)]
    Export(
        SessionKey,
        String,
        crate::application::ExportFormat,
        std::path::PathBuf,
    ),
    Rewind {
        key: SessionKey,
        before: String,
        tip: String,
        files: bool,
    },
    #[serde(skip)]
    Notify(Notification),
    #[serde(skip)]
    Sound(u8),
    Task(Task),
    #[serde(skip)]
    Load(Store),
    #[serde(skip)]
    Save(Store, std::sync::Arc<Snapshot>),
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
    EditBranch(
        SessionKey,
        ForkBoundary,
        String,
        Vec<crate::application::Attachment>,
    ),
    Resume(SessionKey),
    VerifyOpenCodeMove {
        from: SessionKey,
        destination: String,
    },
    History(SessionKey, Option<String>),
    HandoffChat(SessionKey),
    CodexControlHistory(SessionKey, crate::application::HistoryAction),
    Send(SessionKey, String, Options),
    RunOpenCodeCommand(SessionKey, String, Options),
    Cancel(SessionKey),
    Approve(ApprovalPrompt, ApprovalDecision),
    Answer(QuestionPrompt, Option<crate::questions::Answer>),
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct OpenCodeMove {
    pub from: SessionKey,
    pub project: ProjectInfo,
    pub archived: bool,
    /// Only a Git-verified sibling checkout may be opened automatically.
    pub linked_worktree: bool,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum Reply {
    #[serde(skip)]
    Remote(crate::remote::Outcome),
    CheckpointFiles(Vec<String>),
    Inbox(Vec<serde_json::Value>),
    Generated(String),
    Task(TaskReply),
    #[serde(skip)]
    Snapshot(Option<Box<Snapshot>>),
    Commands(Vec<caelum_protocol::OpenCodeCommandInfo>),
    Shells(Vec<caelum_protocol::OpenCodeBackgroundShell>),
    Details(caelum_protocol::SessionDetails),
    Project(ProjectInfo),
    Sessions(Vec<Summary>),
    Session(Summary),
    Forked {
        source: SessionKey,
        child: Summary,
    },
    EditableBranch {
        source: SessionKey,
        child: Summary,
        text: String,
        attachments: Vec<crate::application::Attachment>,
    },
    Resumed(Summary, SessionHistoryPage),
    SessionMoved(OpenCodeMove),
    Choices(ProviderChoices),
    History(SessionHistoryPage),
    HandoffChat(crate::remote::Chat),
    #[serde(skip)]
    Sent(Option<crate::checkpoints::TurnCheckpoint>),
    Done,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum Event {
    SideAnswer(SessionKey, u64, String),
    #[serde(skip)]
    UpdateProgress(u64, crate::updates::Progress),
    #[serde(skip)]
    Runner(crate::runner::RunnerEvent),
    Checkout(String),
    Completed(u64, Result<Reply, String>),
    Codex(CodexEvent),
    OpenCode(OpenCodeEvent),
    Antigravity(AntigravityEvent),
    Approvals(Vec<ApprovalPrompt>),
    Questions(Vec<QuestionPrompt>),
    Branch(caelum_protocol::SessionBranchUpdate),
}
