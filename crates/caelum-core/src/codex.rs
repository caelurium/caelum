use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc, Condvar, Mutex, MutexGuard,
    },
    time::Duration,
};

use caelum_protocol::CodexEvent;
use serde_json::{json, Value};
use thiserror::Error;

mod requests;
mod sessions;
#[cfg(all(test, unix))]
mod tests;
mod transport;
mod websocket;

use transport::spawn_reader;

use crate::{
    approvals::ApprovalBroker,
    processes::{ProcessId, ProcessRegistry},
    projects::{Project, ProjectError},
    questions::QuestionBroker,
};

const PROVIDER_NAME: &str = "codex";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(5 * 60);

type PendingRequests = Arc<Mutex<HashMap<String, Sender<Result<Value, String>>>>>;
type EventSubscribers = Arc<Mutex<Vec<Sender<CodexEvent>>>>;

/// Manages project-scoped Codex app-server connections and their threads.
/// Each connection uses Codex's local stdio JSONL transport.
#[derive(Clone)]
pub struct CodexProvider {
    inner: Arc<ProviderInner>,
}

struct ProviderInner {
    remote: Option<Arc<crate::remote::native::Access>>,
    executable: Option<PathBuf>,
    processes: Arc<ProcessRegistry>,
    approvals: ApprovalBroker,
    questions: QuestionBroker,
    connections: Mutex<Option<HashMap<PathBuf, Arc<ConnectionSlot>>>>,
    subscribers: EventSubscribers,
}

/// One checkout's connection attempt. The provider map is never locked during I/O.
#[derive(Default)]
struct ConnectionSlot {
    state: Mutex<ConnectionState>,
    ready: Condvar,
}

#[derive(Default)]
enum ConnectionState {
    #[default]
    Empty,
    Connecting(Arc<AtomicBool>),
    Ready(Arc<CodexConnection>),
    Closed,
}

impl ConnectionSlot {
    fn connect(
        &self,
        processes: &ProcessRegistry,
        connect: impl FnOnce(Arc<AtomicBool>) -> Result<Arc<CodexConnection>, CodexError>,
    ) -> Result<Arc<CodexConnection>, CodexError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            match &*state {
                ConnectionState::Ready(connection) if connection.alive.load(Ordering::Acquire) => {
                    return Ok(Arc::clone(connection));
                }
                ConnectionState::Connecting(_) => {
                    state = self.ready.wait(state).unwrap_or_else(|e| e.into_inner());
                }
                ConnectionState::Closed => return Err(CodexError::Closed),
                _ => break,
            }
        }
        let alive = Arc::new(AtomicBool::new(true));
        let previous = std::mem::replace(&mut *state, ConnectionState::Connecting(alive.clone()));
        drop(state);
        if let ConnectionState::Ready(dead) = previous {
            dead.close(processes);
        }
        let result = connect(alive);
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(*state, ConnectionState::Closed) {
            drop(state);
            if let Ok(connection) = result {
                connection.close(processes);
            }
            return Err(CodexError::Closed);
        }
        *state = match &result {
            Ok(connection) => ConnectionState::Ready(Arc::clone(connection)),
            Err(_) => ConnectionState::Empty,
        };
        self.ready.notify_all();
        result
    }

    fn current(&self) -> Option<Arc<CodexConnection>> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        while matches!(*state, ConnectionState::Connecting(_)) {
            state = self.ready.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        match &*state {
            ConnectionState::Ready(connection) if connection.alive.load(Ordering::Acquire) => {
                Some(Arc::clone(connection))
            }
            _ => None,
        }
    }

    #[cfg(test)]
    fn stored(&self) -> Option<Arc<CodexConnection>> {
        match &*self.state.lock().unwrap_or_else(|e| e.into_inner()) {
            ConnectionState::Ready(connection) => Some(Arc::clone(connection)),
            _ => None,
        }
    }

    fn retire(&self) -> Option<Arc<CodexConnection>> {
        let previous = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            std::mem::replace(&mut *state, ConnectionState::Closed)
        };
        let connection = match previous {
            ConnectionState::Connecting(alive) => {
                alive.store(false, Ordering::Release);
                None
            }
            ConnectionState::Ready(connection) => {
                connection.alive.store(false, Ordering::Release);
                Some(connection)
            }
            _ => None,
        };
        self.ready.notify_all();
        connection
    }
}

struct CodexConnection {
    process_id: Option<ProcessId>,
    version: std::sync::OnceLock<semver::Version>,
    writer: Arc<transport::Writer>,
    pending: PendingRequests,
    next_id: AtomicU64,
    alive: Arc<AtomicBool>,
    thread_ids: Mutex<HashSet<String>>,
    pending_turns: Arc<Mutex<VecDeque<String>>>,
    turn_threads: Arc<Mutex<HashMap<String, String>>>,
}

struct ReaderContext {
    pending: PendingRequests,
    writer: Arc<transport::Writer>,
    alive: Arc<AtomicBool>,
    subscribers: EventSubscribers,
    approvals: ApprovalBroker,
    questions: QuestionBroker,
    project_path: PathBuf,
    pending_turns: Arc<Mutex<VecDeque<String>>>,
    turn_threads: Arc<Mutex<HashMap<String, String>>>,
}

impl CodexProvider {
    pub(crate) fn forwarded(mut self, access: Arc<crate::remote::native::Access>) -> Self {
        Arc::get_mut(&mut self.inner).expect("new provider").remote = Some(access);
        self
    }
    fn project(&self, path: impl AsRef<Path>) -> Result<Project, ProjectError> {
        Project::for_provider(path, self.inner.remote.as_deref())
    }
    pub fn new(
        processes: Arc<ProcessRegistry>,
        approvals: ApprovalBroker,
        questions: QuestionBroker,
    ) -> Self {
        Self::with_optional_executable(processes, approvals, questions, None)
    }

    /// Construct a provider with an explicit Codex executable. This is useful
    /// for installations outside `PATH` and for protocol-compatible clients.
    pub fn with_executable(
        processes: Arc<ProcessRegistry>,
        approvals: ApprovalBroker,
        questions: QuestionBroker,
        executable: impl Into<PathBuf>,
    ) -> Self {
        Self::with_optional_executable(processes, approvals, questions, Some(executable.into()))
    }

    fn with_optional_executable(
        processes: Arc<ProcessRegistry>,
        approvals: ApprovalBroker,
        questions: QuestionBroker,
        executable: Option<PathBuf>,
    ) -> Self {
        Self {
            inner: Arc::new(ProviderInner {
                remote: None,
                executable,
                processes,
                approvals,
                questions,
                connections: Mutex::new(Some(HashMap::new())),
                subscribers: Arc::new(Mutex::new(Vec::new())),
            }),
        }
    }

    /// Whether the Codex CLI executable is available on this machine.
    pub fn is_installed(&self) -> bool {
        match self.inner.executable.as_deref() {
            Some(path) => crate::executable::executable(path),
            None => find_codex_executable().is_ok(),
        }
    }

    /// Subscribe to Codex thread, turn, item, and process events.
    pub fn subscribe(&self) -> Receiver<CodexEvent> {
        let (sender, receiver) = mpsc::channel();
        let connections = self.inner.lock_connections();
        if connections.is_some() {
            self.inner.lock_subscribers().push(sender);
        }
        receiver
    }

    pub fn ensure_checkout_idle(&self, checkout: &Path) -> Result<(), CodexError> {
        let slots: Vec<_> = self
            .inner
            .lock_connections()
            .as_ref()
            .into_iter()
            .flatten()
            .filter(|(path, _)| path.starts_with(checkout))
            .map(|(path, slot)| (path.clone(), Arc::clone(slot)))
            .collect();
        for (path, slot) in slots {
            let Some(connection) = slot.current() else {
                continue;
            };
            let starting = !connection
                .pending_turns
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_empty();
            if starting
                || Self::list_sessions_on(&self.project(&path)?, &connection, false)?
                    .threads
                    .iter()
                    .any(|thread| thread.is_working)
            {
                return Err(CodexError::Protocol(
                    "Stop running Codex sessions before removing this worktree.".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn release_checkout(&self, checkout: &Path) {
        let removed = {
            let mut registry = self.inner.lock_connections();
            let Some(connections) = registry.as_mut() else {
                return;
            };
            let paths: Vec<_> = connections
                .keys()
                .filter(|path| path.starts_with(checkout))
                .cloned()
                .collect();
            paths
                .into_iter()
                .filter_map(|path| {
                    let connection = connections.remove(&path)?.retire();
                    self.inner
                        .questions
                        .clear(PROVIDER_NAME, &path.to_string_lossy(), None);
                    connection
                })
                .collect::<Vec<_>>()
        };
        for connection in removed {
            connection.close(&self.inner.processes);
        }
    }

    /// Terminate every app-server process owned by this provider.
    pub fn shutdown(&self) {
        let Some(connections) = self.inner.lock_connections().take() else {
            return;
        };
        for (path, slot) in connections {
            self.inner
                .questions
                .clear(PROVIDER_NAME, &path.to_string_lossy(), None);
            if let Some(connection) = slot.retire() {
                connection.close(&self.inner.processes);
            }
        }
        self.inner.lock_subscribers().clear();
    }

    fn connection(&self, project: &Project) -> Result<Arc<CodexConnection>, CodexError> {
        let slot = {
            let mut registry = self.inner.lock_connections();
            registry
                .as_mut()
                .ok_or(CodexError::Closed)?
                .entry(project.path().to_path_buf())
                .or_default()
                .clone()
        };
        slot.connect(&self.inner.processes, |alive| self.connect(project, alive))
    }

    fn connect(
        &self,
        project: &Project,
        alive: Arc<AtomicBool>,
    ) -> Result<Arc<CodexConnection>, CodexError> {
        if !alive.load(Ordering::Acquire) {
            return Err(CodexError::Closed);
        }

        let (process_id, writer, reader) = if let Some(access) = &self.inner.remote {
            let url = access.codex().map_err(|error| {
                if error.contains("CAELUM_CODEX_AUTH_REQUIRED") {
                    CodexError::NotAuthenticated
                } else {
                    CodexError::Transport(error)
                }
            })?;
            let (writer, reader) = websocket::open(access, &url)?;
            (None, writer, reader)
        } else {
            let executable = match self.inner.executable.as_deref() {
                Some(path) if crate::executable::executable(path) => path.to_path_buf(),
                Some(path) => return Err(CodexError::ExecutableNotFound(path.to_path_buf())),
                None => find_codex_executable()?,
            };
            check_authentication(&executable, project.path())?;

            let mut child = Command::new(&executable)
                .env("PATH", crate::executable::path(&executable))
                .args(["app-server", "--listen", "stdio://"])
                .current_dir(project.path())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|source| CodexError::StartServer {
                    executable: executable.clone(),
                    source,
                })?;
            let stdin = child.stdin.take().ok_or_else(|| {
                CodexError::Protocol("Codex app-server stdin was not piped".into())
            })?;
            let stdout = child.stdout.take().ok_or_else(|| {
                CodexError::Protocol("Codex app-server stdout was not piped".into())
            })?;
            let process_id = self.inner.processes.register(child)?;
            use std::io::{BufRead, BufReader};
            (
                Some(process_id),
                transport::Writer::Stdio(crate::json_lines::JsonLines::new(stdin)),
                Box::new(BufReader::new(stdout).lines()) as transport::Messages,
            )
        };
        let connection = Arc::new(CodexConnection {
            process_id,
            version: Default::default(),
            writer: Arc::new(writer),
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(1),
            alive,
            thread_ids: Mutex::new(HashSet::new()),
            pending_turns: Arc::new(Mutex::new(VecDeque::new())),
            turn_threads: Arc::new(Mutex::new(HashMap::new())),
        });

        spawn_reader(
            reader,
            ReaderContext {
                pending: Arc::clone(&connection.pending),
                writer: Arc::clone(&connection.writer),
                alive: Arc::clone(&connection.alive),
                subscribers: Arc::clone(&self.inner.subscribers),
                approvals: self.inner.approvals.clone(),
                questions: self.inner.questions.clone(),
                project_path: project.path().to_path_buf(),
                pending_turns: Arc::clone(&connection.pending_turns),
                turn_threads: Arc::clone(&connection.turn_threads),
            },
        );

        let init_result = connection
            .request(
                "initialize",
                json!({
                    "capabilities": { "experimentalApi": true },
                    "clientInfo": {
                        "name": "caelum",
                        "title": "Caelum",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            )
            .and_then(|info| {
                if let Some(version) = info["userAgent"]
                    .as_str()
                    .and_then(|s| s.split_whitespace().next())
                    .and_then(|s| s.rsplit_once('/'))
                    .and_then(|(_, v)| semver::Version::parse(v).ok())
                {
                    let _ = connection.version.set(version);
                }
                connection.notify("initialized", json!({}))?;
                Ok(Value::Null)
            });
        if let Err(error) = init_result {
            connection.close(&self.inner.processes);
            return Err(error);
        }

        if let Some(access) = &self.inner.remote {
            // Resubscribing active threads lets Codex reissue its own pending
            // requests, even for a background session. Never restart a turn.
            let subscribed = Self::list_sessions_on(project, &connection, false).and_then(|list| {
                for thread in list.threads.into_iter().filter(|t| t.is_working) {
                    match connection.request(
                        "thread/resume",
                        json!({"threadId":thread.id,"excludeTurns":true}),
                    ) {
                        Ok(_) | Err(CodexError::Rpc(_)) => {}
                        Err(error) => return Err(error),
                    }
                }
                Ok(())
            });
            if let Err(e) = subscribed {
                connection.close(&self.inner.processes);
                return Err(e);
            }
            access.codex_used.store(true, Ordering::Release);
        }
        Ok(connection)
    }

    fn connection_for_thread(
        &self,
        project: &Project,
        thread_id: &str,
    ) -> Result<Arc<CodexConnection>, CodexError> {
        let mut connection = self.connection(project)?;
        if !connection.owns_thread(thread_id) {
            self.list_sessions(project.path())?;
            connection = self.connection(project)?;
        }
        if !connection.owns_thread(thread_id) {
            return Err(CodexError::ThreadNotInProject);
        }
        Ok(connection)
    }
}

impl Drop for ProviderInner {
    fn drop(&mut self) {
        let Some(connections) = self
            .connections
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        else {
            return;
        };
        for (path, slot) in connections {
            if let Some(connection) = slot.retire() {
                connection.close(&self.processes);
            }
            self.questions
                .clear(PROVIDER_NAME, &path.to_string_lossy(), None);
        }
    }
}

impl CodexConnection {
    fn close(&self, processes: &ProcessRegistry) {
        self.alive.store(false, Ordering::Release);
        if let Some(id) = self.process_id {
            let _ = processes.terminate(id);
        }
        if let transport::Writer::WebSocket(_, stop, _) = self.writer.as_ref() {
            stop.send_replace(true);
        }
    }
}

fn id_key(id: &Value) -> String {
    match id {
        Value::String(value) => value.clone(),
        _ => id.to_string(),
    }
}

fn request_id_string(id: &Value) -> Result<String, ()> {
    match id {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        _ => Err(()),
    }
}

fn project_key(path: &Path) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

fn find_codex_executable() -> Result<PathBuf, CodexError> {
    crate::executable::find("codex").ok_or(CodexError::NotInstalled)
}

fn check_authentication(executable: &Path, project_path: &Path) -> Result<(), CodexError> {
    let output = Command::new(executable)
        .env("PATH", crate::executable::path(executable))
        .args(["login", "status"])
        .current_dir(project_path)
        .output()
        .map_err(|source| CodexError::CheckAuthentication {
            executable: executable.to_path_buf(),
            source,
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(CodexError::NotAuthenticated)
    }
}

#[derive(Debug, Error)]
pub enum CodexError {
    #[error(transparent)]
    Project(#[from] ProjectError),
    #[error("Codex CLI was not found. Install Codex CLI, then refresh provider status.")]
    NotInstalled,
    #[error("Codex executable was not found at '{0}'. Check the configured Codex CLI path.")]
    ExecutableNotFound(PathBuf),
    #[error("Codex is not signed in. Run `codex login` in a terminal on the machine running Codex, then try again.")]
    NotAuthenticated,
    #[error("could not check Codex authentication using '{executable}': {source}")]
    CheckAuthentication {
        executable: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not start Codex app-server using '{executable}': {source}")]
    StartServer {
        executable: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Codex app-server transport failed: {0}")]
    Transport(String),
    #[error("Codex app-server request failed: {0}")]
    Rpc(String),
    #[error("Codex app-server protocol error: {0}")]
    Protocol(String),
    #[error("Codex app-server request '{0}' timed out")]
    RequestTimedOut(String),
    #[error("Codex app-server connection closed unexpectedly")]
    ConnectionClosed,
    #[error("Codex session ID cannot be empty")]
    InvalidThreadId,
    #[error("Codex session does not belong to this project")]
    ThreadNotInProject,
    #[error("Prompt cannot be empty")]
    EmptyPrompt,
    #[error("Codex provider is shutting down")]
    Closed,
    #[error(transparent)]
    Process(#[from] crate::processes::ProcessError),
}

impl ProviderInner {
    fn lock_connections(&self) -> MutexGuard<'_, Option<HashMap<PathBuf, Arc<ConnectionSlot>>>> {
        self.connections.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn lock_subscribers(&self) -> MutexGuard<'_, Vec<Sender<CodexEvent>>> {
        self.subscribers.lock().unwrap_or_else(|p| p.into_inner())
    }
}
