use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc, Mutex, MutexGuard,
    },
    time::Duration,
};

use caelum_protocol::{
    AntigravityEvent, AntigravitySession, AntigravitySessionInfo, AntigravitySessionList,
    OpenCodeCommandInfo, ProviderChoices, SessionDetails, SessionHistoryPage,
};
use serde_json::{json, Value};
use thiserror::Error;

pub(crate) mod protocol;
#[cfg(all(test, unix))]
mod tests;
mod transport;

use crate::{
    approvals::ApprovalBroker,
    processes::{ProcessId, ProcessRegistry},
    projects::{Project, ProjectError},
};

const PROVIDER_NAME: &str = "antigravity";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(5 * 60);

type PendingRequests = Arc<Mutex<HashMap<String, PendingCall>>>;
type EventSubscribers = Arc<Mutex<Vec<Sender<AntigravityEvent>>>>;
type ReplayBuffers = Arc<Mutex<HashMap<String, Vec<Value>>>>;

enum PendingCall {
    Reply(Sender<Result<Value, String>>),
    Prompt { session_id: String },
}

/// Manages project-scoped Antigravity ACP (`agy acp --stdio`) connections.
#[derive(Clone)]
pub struct AntigravityProvider {
    inner: Arc<ProviderInner>,
}

struct ProviderInner {
    executable: Option<PathBuf>,
    processes: Arc<ProcessRegistry>,
    approvals: ApprovalBroker,
    connections: Mutex<HashMap<PathBuf, Arc<AcpConnection>>>,
    subscribers: EventSubscribers,
    closed: AtomicBool,
}

struct AcpConnection {
    process_id: ProcessId,
    writer: Arc<Mutex<std::process::ChildStdin>>,
    pending: PendingRequests,
    next_id: AtomicU64,
    alive: Arc<AtomicBool>,
    loaded_sessions: Mutex<HashSet<String>>,
    working_sessions: Arc<Mutex<HashSet<String>>>,
    replays: ReplayBuffers,
    choices: Arc<Mutex<Option<ProviderChoices>>>,
    commands: Arc<Mutex<Vec<OpenCodeCommandInfo>>>,
    session_options: Arc<Mutex<HashMap<String, SessionOptionState>>>,
}

#[derive(Clone, Default)]
struct SessionOptionState {
    model: Option<String>,
    effort: Option<String>,
    agent: Option<String>,
}

struct ReaderContext {
    pending: PendingRequests,
    writer: Arc<Mutex<std::process::ChildStdin>>,
    alive: Arc<AtomicBool>,
    subscribers: EventSubscribers,
    approvals: ApprovalBroker,
    project_path: PathBuf,
    working_sessions: Arc<Mutex<HashSet<String>>>,
    replays: ReplayBuffers,
    commands: Arc<Mutex<Vec<OpenCodeCommandInfo>>>,
    session_options: Arc<Mutex<HashMap<String, SessionOptionState>>>,
}

impl AntigravityProvider {
    pub fn new(processes: Arc<ProcessRegistry>, approvals: ApprovalBroker) -> Self {
        Self::with_executable(None, processes, approvals)
    }

    pub fn with_executable(
        executable: Option<PathBuf>,
        processes: Arc<ProcessRegistry>,
        approvals: ApprovalBroker,
    ) -> Self {
        Self {
            inner: Arc::new(ProviderInner {
                executable,
                processes,
                approvals,
                connections: Mutex::new(HashMap::new()),
                subscribers: Arc::new(Mutex::new(Vec::new())),
                closed: AtomicBool::new(false),
            }),
        }
    }

    pub fn subscribe(&self) -> Receiver<AntigravityEvent> {
        let (sender, receiver) = mpsc::channel();
        lock(&self.inner.subscribers).push(sender);
        receiver
    }

    pub fn shutdown(&self) {
        self.inner.closed.store(true, Ordering::Release);
        let connections: Vec<_> = lock(&self.inner.connections)
            .drain()
            .map(|(_, connection)| connection)
            .collect();
        for connection in connections {
            connection.alive.store(false, Ordering::Release);
            let _ = self.inner.processes.terminate(connection.process_id);
        }
    }

    pub fn list_sessions(
        &self,
        project_path: impl AsRef<Path>,
    ) -> Result<AntigravitySessionList, AntigravityError> {
        let project = Project::open(project_path)?;
        let connection = self.connection(&project)?;
        let cwd = project.path().to_string_lossy().into_owned();
        let result = connection.request("session/list", json!({ "cwd": cwd }))?;
        let working = lock(&connection.working_sessions).clone();
        let options = lock(&connection.session_options).clone();
        let sessions = protocol::parse_session_list(&result, project.path(), &working, &options);
        Ok(AntigravitySessionList {
            project: project.info(),
            sessions,
        })
    }

    pub fn choices(
        &self,
        project_path: impl AsRef<Path>,
    ) -> Result<ProviderChoices, AntigravityError> {
        let project = Project::open(project_path)?;
        let connection = self.connection(&project)?;
        if let Some(cached) = lock(&connection.choices).clone() {
            return Ok(cached);
        }
        let cwd = project.path().to_string_lossy().into_owned();
        let result = connection.request("session/new", json!({ "cwd": cwd, "mcpServers": [] }))?;
        if let Some(session_id) = result["sessionId"].as_str() {
            let _ = connection.request("session/close", json!({ "sessionId": session_id }));
        }
        let choices = protocol::parse_choices(&result);
        *lock(&connection.choices) = Some(choices.clone());
        Ok(choices)
    }

    pub fn commands(
        &self,
        project_path: impl AsRef<Path>,
    ) -> Result<Vec<OpenCodeCommandInfo>, AntigravityError> {
        let project = Project::open(project_path)?;
        let connection = self.connection(&project)?;
        let commands = lock(&connection.commands).clone();
        if !commands.is_empty() {
            return Ok(commands);
        }
        let _ = self.choices(project.path());
        let commands = lock(&connection.commands).clone();
        Ok(commands)
    }

    pub fn start_session_with_options(
        &self,
        project_path: impl AsRef<Path>,
        model: Option<&str>,
        effort: Option<&str>,
        agent: Option<&str>,
    ) -> Result<AntigravitySessionInfo, AntigravityError> {
        let project = Project::open(project_path)?;
        let connection = self.connection(&project)?;
        let cwd = project.path().to_string_lossy().into_owned();
        let result = connection.request("session/new", json!({ "cwd": cwd, "mcpServers": [] }))?;
        let session_id = result["sessionId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                AntigravityError::Protocol("Antigravity did not return a session ID".into())
            })?
            .to_owned();
        let choices = protocol::parse_choices(&result);
        if !choices.models.is_empty() || !choices.agents.is_empty() {
            *lock(&connection.choices) = Some(choices);
        }
        let mut opts = protocol::parse_current_options(&result);
        self.apply_options(&connection, &session_id, &mut opts, model, effort, agent)?;
        lock(&connection.loaded_sessions).insert(session_id.clone());
        lock(&connection.session_options).insert(session_id.clone(), opts.clone());
        Ok(AntigravitySessionInfo {
            project: project.info(),
            session: AntigravitySession {
                id: session_id,
                title: String::new(),
                updated_at: None,
                is_working: false,
                model: opts.model,
                effort: opts.effort,
                agent: opts.agent,
            },
        })
    }

    pub fn resume_session(
        &self,
        project_path: impl AsRef<Path>,
        session_id: &str,
    ) -> Result<(AntigravitySessionInfo, SessionHistoryPage), AntigravityError> {
        let project = Project::open(project_path)?;
        let connection = self.connection(&project)?;
        let (result, replayed) = self.load_session(&connection, project.path(), session_id)?;
        let choices = protocol::parse_choices(&result);
        if !choices.models.is_empty() || !choices.agents.is_empty() {
            *lock(&connection.choices) = Some(choices);
        }
        let opts = protocol::parse_current_options(&result);
        lock(&connection.loaded_sessions).insert(session_id.to_owned());
        lock(&connection.session_options).insert(session_id.to_owned(), opts.clone());
        let is_working = lock(&connection.working_sessions).contains(session_id);
        let mut page = protocol::history_from_updates(&replayed, is_working, &opts);
        let title = page
            .data
            .iter()
            .find(|item| item["type"] == "user")
            .and_then(|item| item["text"].as_str())
            .map(|text| {
                text.lines()
                    .next()
                    .unwrap_or(text)
                    .chars()
                    .take(60)
                    .collect()
            })
            .unwrap_or_default();
        if page.model.is_none() {
            page.model = opts.model.clone();
        }
        if page.effort.is_none() {
            page.effort = opts.effort.clone();
        }
        if page.agent.is_none() {
            page.agent = opts.agent.clone();
        }
        Ok((
            AntigravitySessionInfo {
                project: project.info(),
                session: AntigravitySession {
                    id: session_id.to_owned(),
                    title,
                    updated_at: None,
                    is_working,
                    model: opts.model,
                    effort: opts.effort,
                    agent: opts.agent,
                },
            },
            page,
        ))
    }

    pub fn session_history(
        &self,
        project_path: impl AsRef<Path>,
        session_id: &str,
    ) -> Result<SessionHistoryPage, AntigravityError> {
        let (_, page) = self.resume_session(project_path, session_id)?;
        Ok(page)
    }

    pub fn session_details(
        &self,
        project_path: impl AsRef<Path>,
        _session_id: &str,
    ) -> Result<SessionDetails, AntigravityError> {
        let _ = Project::open(project_path)?;
        Ok(SessionDetails {
            branch: None,
            branches: Vec::new(),
            branch_is_current: true,
            changes: None,
        })
    }

    pub fn send_prompt_with_options(
        &self,
        project_path: impl AsRef<Path>,
        session_id: &str,
        prompt: &str,
        model: Option<&str>,
        effort: Option<&str>,
        agent: Option<&str>,
    ) -> Result<(), AntigravityError> {
        let project = Project::open(project_path)?;
        let connection = self.connection(&project)?;
        if !lock(&connection.loaded_sessions).contains(session_id) {
            let (result, _) = self.load_session(&connection, project.path(), session_id)?;
            let opts = protocol::parse_current_options(&result);
            lock(&connection.loaded_sessions).insert(session_id.to_owned());
            lock(&connection.session_options).insert(session_id.to_owned(), opts);
        }
        let mut opts = lock(&connection.session_options)
            .get(session_id)
            .cloned()
            .unwrap_or_default();
        self.apply_options(&connection, session_id, &mut opts, model, effort, agent)?;
        lock(&connection.session_options).insert(session_id.to_owned(), opts);

        let blocks = protocol::prompt_blocks(prompt);
        lock(&connection.working_sessions).insert(session_id.to_owned());
        transport::publish_event(
            &self.inner.subscribers,
            AntigravityEvent {
                project_path: project.path().to_string_lossy().into_owned(),
                session_id: Some(session_id.to_owned()),
                method: "antigravity/turnStarted".into(),
                params: json!({ "sessionId": session_id }),
            },
        );
        if let Err(error) = connection.send_prompt(
            session_id,
            json!({
                "sessionId": session_id,
                "prompt": blocks,
            }),
        ) {
            lock(&connection.working_sessions).remove(session_id);
            return Err(error);
        }
        Ok(())
    }

    pub fn cancel_turn(
        &self,
        project_path: impl AsRef<Path>,
        session_id: &str,
    ) -> Result<(), AntigravityError> {
        let project = Project::open(project_path)?;
        let connection = self.connection(&project)?;
        connection.notify("session/cancel", json!({ "sessionId": session_id }))
    }

    fn load_session(
        &self,
        connection: &AcpConnection,
        project_path: &Path,
        session_id: &str,
    ) -> Result<(Value, Vec<Value>), AntigravityError> {
        lock(&connection.replays).insert(session_id.to_owned(), Vec::new());
        let cwd = project_path.to_string_lossy().into_owned();
        let result = connection.request(
            "session/load",
            json!({
                "sessionId": session_id,
                "cwd": cwd,
                "mcpServers": [],
            }),
        );
        let replayed = lock(&connection.replays)
            .remove(session_id)
            .unwrap_or_default();
        Ok((result?, replayed))
    }

    fn apply_options(
        &self,
        connection: &AcpConnection,
        session_id: &str,
        current: &mut SessionOptionState,
        model: Option<&str>,
        effort: Option<&str>,
        agent: Option<&str>,
    ) -> Result<(), AntigravityError> {
        if let Some(model) = model.filter(|m| !m.is_empty()) {
            if current.model.as_deref() != Some(model) {
                let _ = connection.request(
                    "session/set_config_option",
                    json!({
                        "sessionId": session_id,
                        "configId": "model",
                        "value": model,
                    }),
                )?;
                current.model = Some(model.to_owned());
            }
        }
        if let Some(effort) = effort.filter(|e| !e.is_empty()) {
            if current.effort.as_deref() != Some(effort) {
                let _ = connection.request(
                    "session/set_config_option",
                    json!({
                        "sessionId": session_id,
                        "configId": "thought_level",
                        "value": effort,
                    }),
                )?;
                current.effort = Some(effort.to_owned());
            }
        }
        if let Some(agent) = agent.filter(|a| !a.is_empty()) {
            if current.agent.as_deref() != Some(agent) {
                let _ = connection.request(
                    "session/set_mode",
                    json!({
                        "sessionId": session_id,
                        "modeId": agent,
                    }),
                )?;
                current.agent = Some(agent.to_owned());
            }
        }
        Ok(())
    }

    fn connection(&self, project: &Project) -> Result<Arc<AcpConnection>, AntigravityError> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(AntigravityError::ConnectionClosed);
        }
        let path = project.path().to_path_buf();
        if let Some(existing) = lock(&self.inner.connections).get(&path).cloned() {
            if existing.alive.load(Ordering::Acquire) {
                return Ok(existing);
            }
        }
        let connection = transport::spawn_connection(
            self.executable(),
            &path,
            &self.inner.processes,
            self.inner.approvals.clone(),
            self.inner.subscribers.clone(),
        )?;
        let mut connections = lock(&self.inner.connections);
        if self.inner.closed.load(Ordering::Acquire) {
            connection.alive.store(false, Ordering::Release);
            let _ = self.inner.processes.terminate(connection.process_id);
            return Err(AntigravityError::ConnectionClosed);
        }
        connections.insert(path, connection.clone());
        Ok(connection)
    }

    fn executable(&self) -> Result<PathBuf, AntigravityError> {
        if let Some(path) = &self.inner.executable {
            return Ok(path.clone());
        }
        if let Some(env_path) = std::env::var_os("CAELUM_ANTIGRAVITY_BIN") {
            let path = PathBuf::from(env_path);
            if !path.as_os_str().is_empty() {
                return Ok(path);
            }
        }
        crate::executable::find("agy").ok_or_else(|| {
            AntigravityError::Launch(
                "Could not find `agy` on PATH. Install Antigravity CLI or set CAELUM_ANTIGRAVITY_BIN."
                    .into(),
            )
        })
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, Error)]
pub enum AntigravityError {
    #[error(transparent)]
    Project(#[from] ProjectError),
    #[error("{0}")]
    Launch(String),
    #[error("Antigravity ACP transport error: {0}")]
    Transport(String),
    #[error("Antigravity ACP protocol error: {0}")]
    Protocol(String),
    #[error("Antigravity ACP RPC error: {0}")]
    Rpc(String),
    #[error("Antigravity ACP request `{0}` timed out")]
    RequestTimedOut(String),
    #[error("Antigravity ACP connection closed")]
    ConnectionClosed,
}
