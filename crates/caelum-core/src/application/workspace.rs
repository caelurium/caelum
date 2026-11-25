mod approval_policy;
mod archive;
mod completion;
mod composer;
mod controls;
mod events;
mod fork;
mod history;
mod home;
mod notifications;
mod persistence;
mod projects;
mod provider_switch;
mod queue;
mod relocation;
mod remote;
mod review;
mod sessions;
mod subagents;
mod submission;
mod tasks;
mod templates;
mod titles;

pub use fork::FilePreview;
pub use fork::ForkBoundary;
pub use fork::ForkPicker;
pub use fork::ForkPoint;
pub use fork::HistoryAction;
pub use home::Home;
pub use home::HomeDraft;
pub use projects::ProjectMove;
pub use remote::handoff::HandoffPhase;
pub use remote::handoff::HandoffSource;
pub use remote::handoff::VmDestination;
pub use remote::handoff::VmHandoff;

use std::collections::{BTreeSet, HashMap};

use caelum_protocol::{ApprovalPrompt, ProjectInfo, ProviderChoices, QuestionPrompt};

use super::{
    Attachment, Command, Conversation, Options, Preferences, Provider, QueuedPrompt, Scope,
    Services, SessionKey, Summary, Task,
};
#[cfg(test)]
pub(super) use archive::ArchiveFixture;

pub struct Session {
    provider_switch: provider_switch::State,
    vm_link: Option<crate::remote::VmLink>,
    native_handoff: Option<crate::remote::Continuation>,
    pub scope: Scope,
    pub remote_id: Option<String>,
    relocation: relocation::Relocation,
    pub title: String,
    title_state: titles::TitleState,
    controls: controls::Status,
    /// Last meaningful activity in Unix milliseconds; selection and token deltas do not bump it.
    pub last_activity: Option<u64>,
    pub conversation: Conversation,
    pub options: Options,
    history: history::State,
    submission: submission::Submission,
    native_command: Option<submission::NativeCommand>,
    pub error: Option<String>,
    /// A failed or cancelled submission stays available without replacing a
    /// newer editor draft. The UI can explicitly restore/copy it.
    pub failed_prompt: Option<String>,
    pub draft: String,
    pub attachments: Vec<Attachment>,
    pub attaching: usize,
    pub attachment_errors: Vec<(String, String)>,
    pub completion_caret: Option<usize>,
    pub completion_generation: u64,
    pub queue: Vec<QueuedPrompt>,
    pub inbox: super::Inbox,
    pub queue_paused: bool,
    archive_status: archive::Status,
    pub reading: f32,
    pub unread: bool,
    pub origin: Option<caelum_protocol::SessionOrigin>,
    pub pinned: bool,
    pub bookmarked_entries: Vec<String>,
    pub shells: Vec<caelum_protocol::OpenCodeBackgroundShell>,
    shell_revision: u64,
    pub details: Option<caelum_protocol::SessionDetails>,
    pub side_question: Option<(String, String)>,
    pub side_draft: String,
    pub side_attachments: Vec<Attachment>,
    pub side_error: Option<String>,
    pub side_generation: u64,
    side_cancel: Option<tokio::sync::watch::Sender<()>>,
    pub side_completion_caret: Option<usize>,
    pub generating: bool,
    pub disconnected: bool,
    options_dirty: bool,
    revision: u64,
    seen_events: std::collections::VecDeque<String>,
    usage: crate::telemetry::Tracking,
}

impl Session {
    fn new(scope: Scope) -> Self {
        Self {
            provider_switch: Default::default(),
            vm_link: None,
            native_handoff: None,
            scope,
            remote_id: None,
            relocation: Default::default(),
            title: "New session".into(),
            title_state: Default::default(),
            controls: Default::default(),
            last_activity: None,
            conversation: Conversation::default(),
            options: Options::default(),
            history: Default::default(),
            submission: Default::default(),
            native_command: None,
            error: None,
            failed_prompt: None,
            draft: String::new(),
            attachments: Vec::new(),
            attaching: 0,
            attachment_errors: Vec::new(),
            completion_caret: None,
            completion_generation: 0,
            queue: Vec::new(),
            inbox: Default::default(),
            queue_paused: false,
            archive_status: Default::default(),
            reading: -1.0,
            unread: false,
            origin: None,
            pinned: false,
            bookmarked_entries: Vec::new(),
            shells: Vec::new(),
            shell_revision: 0,
            details: None,
            side_question: None,
            side_draft: String::new(),
            side_attachments: Vec::new(),
            side_error: None,
            side_generation: 0,
            side_cancel: None,
            side_completion_caret: None,
            generating: false,
            disconnected: false,
            options_dirty: false,
            revision: 0,
            seen_events: Default::default(),
            usage: Default::default(),
        }
    }

    pub fn key(&self) -> Option<SessionKey> {
        self.remote_id.as_ref().map(|id| SessionKey {
            scope: self.scope.clone(),
            id: id.clone(),
        })
    }

    pub fn working(&self) -> bool {
        self.submitting() || self.conversation.working()
    }

    pub fn is_bookmarked(&self, entry_id: &str) -> bool {
        self.bookmarked_entries.iter().any(|id| id == entry_id)
    }

    fn record_activity(&mut self, at: u64) {
        self.last_activity = Some(self.last_activity.unwrap_or_default().max(at));
    }

    pub fn attachment_error(&mut self, name: String, error: String) {
        self.attachment_errors.retain(|(path, _)| *path != name);
        if self.attachment_errors.len() == 16 {
            self.attachment_errors.remove(0);
        }
        self.attachment_errors.push((name, error));
    }

    fn has_context(&self) -> bool {
        self.remote_id.is_some()
            || !self.conversation.entries.is_empty()
            || !self.draft.trim().is_empty()
            || !self.attachments.is_empty()
            || !self.queue.is_empty()
            || !self.inbox.items.is_empty()
            || self.title != "New session"
    }

    pub fn can_send(&self) -> bool {
        self.scope.provider_available()
            && !self.rewinding()
            && !self.relocation.pending()
            && !self.working()
            && self.loaded()
            && !self.loading()
            && !self.disconnected
            && !self.stopping()
            && !self.archived()
            && !self.forgotten()
            && !self.archiving()
            && self.attaching == 0
    }

    /// Whether a draft may be submitted, including to the queue while working.
    pub fn can_submit(&self) -> bool {
        self.scope.provider_available()
            && !self.rewinding()
            && !self.relocation.pending()
            && !self.archived()
            && !self.forgotten()
            && !self.stopping()
            && !self.archiving()
            && self.attaching == 0
            && !self.commanding()
    }
}

enum Pending {
    ScopedApproval(approval_policy::Attempt),
    Templates,
    PreviousHistory(usize, usize),
    Remote(remote::Lane),
    ConversationControl(usize, SessionKey, controls::Operation),
    Fork(fork::Request),
    Home(home::Request),
    Inbox(usize, u64),
    Enqueue(usize, String),
    RemoveQueued(usize, String),
    Control(usize),
    Generate(usize, String, u64),
    RegenerateTitle(usize),
    Notify,
    Load,
    Save,
    Task(Task),
    Commands(Scope),
    Shells(usize, u64),
    ListArchived(Scope),
    Archive(usize),
    DeleteSession(usize),
    Attachment(usize, String),
    Mention {
        index: usize,
        side: bool,
        name: String,
        generation: u64,
        draft: String,
        token: super::Completion,
        replacement: String,
    },
    Details(usize),
    Open(Option<Provider>, u64),
    List(Scope),
    Choices(Scope),
    Resume(usize),
    VerifyOpenCodeMove(usize),
    History(usize),
    Start(usize),
    CheckHandoff(usize),
    Send(usize),
    Cancel(usize),
    Request {
        provider: String,
        project: String,
        session: Option<String>,
    },
}

/// View-independent workspace coordinator. Stable local session indices survive
/// first-send creation; every pending request carries its original owner.
#[derive(Default)]
pub struct Workspace {
    policy: approval_policy::State,
    pub templates: crate::templates::Catalog,
    fork: Option<ForkPicker>,
    handoff: Option<VmHandoff>,
    pub home: Home,
    pub remote: crate::remote::State,
    pub window_background: bool,
    pub conversation_hidden: bool,
    pub projects: Vec<ProjectInfo>,
    pub project_roots: std::collections::BTreeMap<String, String>,
    pub sessions: Vec<Session>,
    pub selected: Option<usize>,
    pub active_project: Option<String>,
    pub choices: HashMap<Scope, ProviderChoices>,
    pub notices: HashMap<Scope, String>,
    dirty_checkouts: std::collections::HashSet<String>,
    refresh_after: HashMap<String, Task>,
    pub approvals: Vec<ApprovalPrompt>,
    pub questions: Vec<QuestionPrompt>,
    pub error: Option<String>,
    pub opening: bool,
    pub services: Services,
    pub preferences: Preferences,
    notifications: notifications::State,
    pub provider_commands: HashMap<Scope, Vec<caelum_protocol::OpenCodeCommandInfo>>,
    archive_notice: Option<archive::Notice>,
    archive_revision: u64,
    archive_notice_revision: u64,
    navigation: u64,
    persistence: persistence::State,
    durable_outgoing: Vec<(u64, Command)>,
    ready_outgoing: Vec<(u64, Command)>,
    detail_queue: Vec<usize>,
    pending: HashMap<u64, Pending>,
    outgoing: Vec<(u64, Command)>,
    next_request: u64,
    latest_open: u64,
    launches: HashMap<u64, (Provider, u64)>,
    pending_ci_fixes: HashMap<u64, review::PendingCiFix>,
    remembered: HashMap<String, usize>,
    remembered_checkouts: HashMap<String, usize>,
    managed_worktrees: BTreeSet<String>,
    deferred_lists: HashMap<Scope, (u64, Vec<Summary>)>,
    /// Closing only removes open navigation. Reject earlier discovery/open replies
    /// without invalidating the stable sessions or their in-flight work.
    closed_projects: HashMap<String, u64>,
}

impl Workspace {
    fn record_usage(&mut self, row: crate::telemetry::Record) {
        self.next_request += 1;
        self.outgoing
            .push((self.next_request, Command::RecordUsage(row)));
    }

    fn request(&mut self, mut command: Command, pending: Pending) -> u64 {
        if matches!(command, Command::Notify(..)) && self.preferences.sound_volume > 0 {
            self.request(
                Command::Sound(self.preferences.sound_volume),
                Pending::Notify,
            );
        }
        self.next_request += 1;
        let id = self.next_request;
        self.track_native_request(id, &mut command, &pending);
        crate::diagnostics::command("created", id, &command);
        self.pending.insert(id, pending);
        self.outgoing.push((id, command));
        id
    }
}

fn update_summary(session: &mut Session, summary: Summary, request: u64) {
    if !session.working() || session.last_activity.is_none() || session.revision == 0 {
        if let Some(at) = summary.updated_at {
            session.record_activity(at);
        }
    }
    if session.title_state.allows_snapshot(request) && !summary.title.trim().is_empty() {
        session.title = summary.title;
    }
    if summary.options.model.is_some() && !session.options_dirty {
        session.options.model = summary.options.model;
        session.options.effort = summary.options.effort;
    }
    if summary.options.agent.is_some() && !session.options_dirty {
        session.options.agent = summary.options.agent;
    }
}

fn is_image(path: &str) -> bool {
    matches!(
        std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "bmp" | "ico")
    )
}
