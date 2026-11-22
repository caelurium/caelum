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
    LineChanges, OpenCodeCommandInfo, ProviderChoices, SessionDetails, SessionHistoryPage,
};
use serde_json::{json, Value};
use thiserror::Error;

mod metadata;
mod progress;
pub(crate) mod protocol;
mod questions;
#[cfg(all(test, unix))]
mod tests;
pub(crate) mod tools;
mod transport;

use crate::{
    approvals::ApprovalBroker,
    processes::{ProcessId, ProcessRegistry},
    projects::{Project, ProjectError},
    questions::QuestionBroker,
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
    questions: QuestionBroker,
    connections: Mutex<HashMap<PathBuf, Arc<AcpConnection>>>,
    subscribers: EventSubscribers,
    metadata: Mutex<metadata::SessionMetadataStore>,
    closed: AtomicBool,
}

struct AcpConnection {
    process_id: ProcessId,
    writer: Arc<Mutex<std::process::ChildStdin>>,
    pending: PendingRequests,
    next_id: Arc<AtomicU64>,
    alive: Arc<AtomicBool>,
    loaded_sessions: Mutex<HashSet<String>>,
    progress: Arc<Mutex<progress::SessionProgressTracker>>,
    replays: ReplayBuffers,
    choices: Arc<Mutex<Option<ProviderChoices>>>,
    tier: Arc<Mutex<Option<Value>>>,
    commands: Arc<Mutex<Vec<OpenCodeCommandInfo>>>,
    session_options: Arc<Mutex<HashMap<String, SessionOptionState>>>,
    session_changes: Mutex<HashMap<String, LineChanges>>,
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
    next_id: Arc<AtomicU64>,
    alive: Arc<AtomicBool>,
    subscribers: EventSubscribers,
    approvals: ApprovalBroker,
    questions: QuestionBroker,
    project_path: PathBuf,
    progress: Arc<Mutex<progress::SessionProgressTracker>>,
    replays: ReplayBuffers,
    commands: Arc<Mutex<Vec<OpenCodeCommandInfo>>>,
    session_options: Arc<Mutex<HashMap<String, SessionOptionState>>>,
}

impl AntigravityProvider {
    pub fn new(
        processes: Arc<ProcessRegistry>,
        approvals: ApprovalBroker,
        questions: QuestionBroker,
    ) -> Self {
        Self::with_executable(None, processes, approvals, questions)
    }

    pub fn with_executable(
        executable: Option<PathBuf>,
        processes: Arc<ProcessRegistry>,
        approvals: ApprovalBroker,
        questions: QuestionBroker,
    ) -> Self {
        Self {
            inner: Arc::new(ProviderInner {
                executable,
                processes,
                approvals,
                questions,
                connections: Mutex::new(HashMap::new()),
                subscribers: Arc::new(Mutex::new(Vec::new())),
                metadata: Mutex::new(metadata::SessionMetadataStore::default()),
                closed: AtomicBool::new(false),
            }),
        }
    }

    pub(crate) fn open_metadata(&self, path: PathBuf) -> Result<(), String> {
        lock(&self.inner.metadata).open(path)
    }

    pub fn subscribe(&self) -> Receiver<AntigravityEvent> {
        let (sender, receiver) = mpsc::channel();
        lock(&self.inner.subscribers).push(sender);
        receiver
    }

    pub fn shutdown(&self) {
        self.inner.closed.store(true, Ordering::Release);
        let connections: Vec<_> = lock(&self.inner.connections).drain().collect();
        for (path, connection) in connections {
            connection.alive.store(false, Ordering::Release);
            self.inner
                .questions
                .clear(PROVIDER_NAME, &path.to_string_lossy(), None);
            let _ = self.inner.processes.terminate(connection.process_id);
        }
    }

    pub fn list_sessions(
        &self,
        project_path: impl AsRef<Path>,
    ) -> Result<AntigravitySessionList, AntigravityError> {
        self.list_sessions_filtered(project_path, false)
    }

    pub fn list_archived_sessions(
        &self,
        project_path: impl AsRef<Path>,
    ) -> Result<AntigravitySessionList, AntigravityError> {
        self.list_sessions_filtered(project_path, true)
    }

    fn list_sessions_filtered(
        &self,
        project_path: impl AsRef<Path>,
        archived: bool,
    ) -> Result<AntigravitySessionList, AntigravityError> {
        let project = Project::open(project_path)?;
        let connection = self.connection(&project)?;
        let cwd = project.path().to_string_lossy().into_owned();
        let result = connection.request("session/list", json!({ "cwd": cwd }))?;
        let working = lock(&connection.progress).working_sessions();
        let options = lock(&connection.session_options).clone();
        let metadata = lock(&self.inner.metadata);
        let sessions = protocol::parse_session_list(&result, project.path(), &working, &options)
            .into_iter()
            .filter(|s| {
                !metadata.is_deleted(project.path(), &s.id)
                    && metadata.is_archived(project.path(), &s.id) == archived
            })
            .map(|mut s| {
                if let Some(title) = metadata.title(project.path(), &s.id) {
                    s.title = title;
                }
                s
            })
            .collect();
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
        cache_tier(&connection, &result);
        Ok(choices)
    }

    pub fn usage_limits(&self, project_path: impl AsRef<Path>) -> Result<Value, AntigravityError> {
        let project = Project::open(project_path)?;
        let executable = self.executable()?;
        let output = std::process::Command::new(&executable)
            .args(["--print", "/usage", "--output-format", "json"])
            .current_dir(project.path())
            .env("PATH", crate::executable::path(&executable))
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|error| {
                AntigravityError::Launch(format!(
                    "Could not run `{}` --print /usage: {error}",
                    executable.display()
                ))
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(AntigravityError::Protocol(if stderr.is_empty() {
                format!("Antigravity /usage exited with status {}", output.status)
            } else {
                stderr
            }));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let parsed: Value = serde_json::from_str(stdout.trim()).map_err(|error| {
            AntigravityError::Protocol(format!("Invalid Antigravity /usage JSON: {error}"))
        })?;
        let data = parsed
            .pointer("/command/data")
            .filter(|value| value.is_object())
            .ok_or_else(|| {
                AntigravityError::Protocol("Antigravity /usage response has no usage data".into())
            })?;
        let tier = lock(&self.inner.connections)
            .get(project.path())
            .and_then(|conn| lock(&conn.tier).clone());
        let mut usage = data.clone();
        if let Some(obj) = usage.as_object_mut() {
            obj.insert("provider".into(), Value::String(PROVIDER_NAME.into()));
            if let Some(tier) = tier {
                obj.insert("tier".into(), tier);
            }
        }
        Ok(usage)
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
        cache_tier(&connection, &result);
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
        if lock(&self.inner.metadata).is_deleted(project.path(), session_id) {
            return Err(AntigravityError::Protocol(
                "Antigravity session was deleted".into(),
            ));
        }
        let connection = self.connection(&project)?;
        let (result, replayed) = self.load_session(&connection, project.path(), session_id)?;
        let choices = protocol::parse_choices(&result);
        if !choices.models.is_empty() || !choices.agents.is_empty() {
            *lock(&connection.choices) = Some(choices);
        }
        cache_tier(&connection, &result);
        let opts = protocol::parse_current_options(&result);
        lock(&connection.loaded_sessions).insert(session_id.to_owned());
        lock(&connection.session_options).insert(session_id.to_owned(), opts.clone());
        let mut page =
            protocol::history_from_updates(&replayed, false, &opts, Some(project.path()));
        let active_tools: Vec<String> = page
            .data
            .iter()
            .filter(|item| {
                item["type"] == "tool"
                    && item["name"] != "ask_question"
                    && progress::is_active_tool_status(item["status"].as_str())
            })
            .filter_map(|item| item["toolCallId"].as_str().map(str::to_owned))
            .collect();
        let is_working = {
            let mut tracker = lock(&connection.progress);
            tracker.on_replayed_history(session_id, active_tools);
            tracker.is_working(session_id)
        };
        page.is_working = Some(is_working);
        let project_str = project.path().to_string_lossy().into_owned();
        for item in &page.data {
            if item["type"] == "tool"
                && item["name"] == "ask_question"
                && item["status"] == "pending"
            {
                if let Some(raw) = item.pointer("/metadata/acp") {
                    questions::register_from_update(
                        &self.inner.questions,
                        questions::QuestionReplyContext {
                            project_path: project_str.clone(),
                            session_id: session_id.to_owned(),
                            writer: connection.writer.clone(),
                            pending: connection.pending.clone(),
                            next_id: connection.next_id.clone(),
                            alive: connection.alive.clone(),
                            progress: connection.progress.clone(),
                            subscribers: self.inner.subscribers.clone(),
                        },
                        raw,
                    );
                }
            }
        }
        if let Some(changes) = tools::session_line_changes(&page.data) {
            lock(&connection.session_changes).insert(session_id.to_owned(), changes);
        } else {
            lock(&connection.session_changes).remove(session_id);
        }
        let title = lock(&self.inner.metadata)
            .title(project.path(), session_id)
            .or_else(|| {
                page.data
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
        session_id: &str,
    ) -> Result<SessionDetails, AntigravityError> {
        let project = Project::open(project_path)?;
        let changes = lock(&self.inner.connections)
            .get(project.path())
            .and_then(|conn| lock(&conn.session_changes).get(session_id).cloned());
        Ok(SessionDetails {
            branch: None,
            branches: Vec::new(),
            branch_is_current: true,
            changes,
        })
    }

    pub fn set_session_archived(
        &self,
        project_path: impl AsRef<Path>,
        session_id: &str,
        archived: bool,
    ) -> Result<(), AntigravityError> {
        if session_id.trim().is_empty() {
            return Err(AntigravityError::Protocol(
                "Antigravity session ID cannot be empty".into(),
            ));
        }
        let project = Project::open(project_path)?;
        let connection = self.connection(&project)?;
        self.verify_session_owned(&connection, &project, session_id)?;
        if archived && lock(&connection.progress).is_working(session_id) {
            return Err(AntigravityError::Protocol(
                "stop the running turn before archiving this session".into(),
            ));
        }
        lock(&self.inner.metadata).set_archived(project.path(), session_id, archived)
    }

    pub fn delete_session(
        &self,
        project_path: impl AsRef<Path>,
        session_id: &str,
    ) -> Result<(), AntigravityError> {
        if session_id.trim().is_empty() {
            return Err(AntigravityError::Protocol(
                "Antigravity session ID cannot be empty".into(),
            ));
        }
        let project = Project::open(project_path)?;
        let connection = self.connection(&project)?;
        self.verify_session_owned(&connection, &project, session_id)?;
        if lock(&connection.progress).is_working(session_id) {
            return Err(AntigravityError::Protocol(
                "stop the running turn before deleting this session".into(),
            ));
        }
        if !lock(&self.inner.metadata).is_archived(project.path(), session_id) {
            return Err(AntigravityError::Protocol(
                "only archived Antigravity sessions can be deleted".into(),
            ));
        }
        let _ = connection.request("session/close", json!({ "sessionId": session_id }));
        lock(&connection.loaded_sessions).remove(session_id);
        lock(&connection.session_options).remove(session_id);
        lock(&connection.session_changes).remove(session_id);
        self.inner.questions.clear(
            PROVIDER_NAME,
            &project.path().to_string_lossy(),
            Some(session_id),
        );
        lock(&self.inner.metadata).mark_deleted(project.path(), session_id)
    }

    pub fn rename(
        &self,
        project_path: impl AsRef<Path>,
        session_id: &str,
        title: &str,
    ) -> Result<(), AntigravityError> {
        if session_id.trim().is_empty() {
            return Err(AntigravityError::Protocol(
                "Antigravity session ID cannot be empty".into(),
            ));
        }
        let project = Project::open(project_path)?;
        let connection = self.connection(&project)?;
        self.verify_session_owned(&connection, &project, session_id)?;
        lock(&self.inner.metadata).set_title(project.path(), session_id, title)
    }

    fn verify_session_owned(
        &self,
        connection: &AcpConnection,
        project: &Project,
        session_id: &str,
    ) -> Result<(), AntigravityError> {
        if lock(&self.inner.metadata).is_deleted(project.path(), session_id) {
            return Err(AntigravityError::Protocol(
                "Antigravity session does not exist in this project".into(),
            ));
        }
        if lock(&connection.loaded_sessions).contains(session_id)
            || lock(&connection.session_options).contains_key(session_id)
        {
            return Ok(());
        }
        let cwd = project.path().to_string_lossy().into_owned();
        let result = connection.request("session/list", json!({ "cwd": cwd }))?;
        let exists = result["sessions"].as_array().is_some_and(|items| {
            items.iter().any(|item| {
                item["sessionId"].as_str().map(str::trim) == Some(session_id)
                    && item["cwd"].as_str().is_none_or(|c| {
                        Path::new(c) == project.path()
                            || c == project.path().to_string_lossy().as_ref()
                    })
            })
        });
        if exists {
            Ok(())
        } else {
            Err(AntigravityError::Protocol(
                "Antigravity session does not belong to this project".into(),
            ))
        }
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
        self.inner.questions.clear(
            PROVIDER_NAME,
            &project.path().to_string_lossy(),
            Some(session_id),
        );
        lock(&connection.progress).on_prompt_started(session_id);
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
            lock(&connection.progress).on_prompt_send_failed(session_id);
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
        let result = connection.notify("session/cancel", json!({ "sessionId": session_id }));
        if result.is_ok() {
            let effect = lock(&connection.progress).on_cancel(session_id);
            if let Some(effect) = effect {
                transport::dispatch_progress_effect(
                    &project.path().to_string_lossy(),
                    effect,
                    &self.inner.subscribers,
                    &connection.progress,
                );
            }
        }
        result
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
            self.inner.questions.clone(),
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

fn cache_tier(connection: &AcpConnection, result: &Value) {
    if let Some(tier) = result
        .pointer("/_meta/antigravity/tier")
        .filter(|value| value.is_object())
    {
        *lock(&connection.tier) = Some(tier.clone());
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
