use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
};

use caelum_protocol::{AntigravityEvent, ApprovalDecision, PermissionRequest};
use serde_json::{json, Value};

use super::{
    lock,
    progress::{ProgressEffect, SessionProgressTracker, REACTIVE_SETTLE_DELAY},
    protocol, questions, AcpConnection, AntigravityError, AntigravityProvider, Connection,
    EventSubscribers, PendingCall, PendingRequests, ReaderContext, APPROVAL_TIMEOUT, PROVIDER_NAME,
    REQUEST_TIMEOUT,
};
use crate::{approvals::ApprovalBroker, processes::ProcessRegistry, questions::QuestionBroker};

impl AntigravityProvider {
    /// Whether the Antigravity CLI executable is available on this machine.
    pub fn is_installed(&self) -> bool {
        self.executable().is_ok_and(|path| {
            crate::executable::executable(&path)
                || (path.components().count() == 1
                    && path
                        .to_str()
                        .is_some_and(|name| crate::executable::find(name).is_some()))
        })
    }

    pub(super) fn connection(
        &self,
        project: &crate::projects::Project,
    ) -> Result<Arc<AcpConnection>, AntigravityError> {
        if let Some(existing) = self.live_connection()? {
            return Ok(existing);
        }
        // One ACP startup at a time protects the shared OAuth credential store.
        let _guard = lock(&self.inner.connect_lock);
        if let Some(existing) = self.live_connection()? {
            return Ok(existing);
        }
        let connection = spawn_connection(
            self.executable(),
            project.path(),
            &self.inner.processes,
            self.inner.approvals.clone(),
            self.inner.questions.clone(),
            self.inner.subscribers.clone(),
        )?;
        let mut slot = lock(&self.inner.connection);
        if matches!(*slot, Connection::Closed) {
            drop(slot);
            connection.alive.store(false, Ordering::Release);
            let _ = self.inner.processes.terminate(connection.process_id);
            return Err(AntigravityError::ConnectionClosed);
        }
        *slot = Connection::Connected(connection.clone());
        Ok(connection)
    }

    fn live_connection(&self) -> Result<Option<Arc<AcpConnection>>, AntigravityError> {
        match &*lock(&self.inner.connection) {
            Connection::Closed => Err(AntigravityError::ConnectionClosed),
            Connection::Connected(connection) if connection.alive.load(Ordering::Acquire) => {
                Ok(Some(connection.clone()))
            }
            Connection::Vacant | Connection::Connected(_) => Ok(None),
        }
    }

    pub(super) fn executable(&self) -> Result<PathBuf, AntigravityError> {
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

impl AcpConnection {
    pub(super) fn request(&self, method: &str, params: Value) -> Result<Value, AntigravityError> {
        if !self.alive.load(Ordering::Acquire) {
            return Err(AntigravityError::ConnectionClosed);
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id_key = id.to_string();
        let (sender, receiver) = mpsc::channel();
        lock(&self.pending).insert(id_key.clone(), PendingCall::Reply(sender));
        if let Err(error) = write_message(
            &self.writer,
            &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        ) {
            lock(&self.pending).remove(&id_key);
            return Err(AntigravityError::Transport(error));
        }

        match receiver.recv_timeout(REQUEST_TIMEOUT) {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(message)) => Err(AntigravityError::Rpc(message)),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                lock(&self.pending).remove(&id_key);
                Err(AntigravityError::RequestTimedOut(method.to_owned()))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(AntigravityError::ConnectionClosed),
        }
    }

    pub(super) fn send_prompt(
        &self,
        request: u64,
        session_id: &str,
        params: Value,
    ) -> Result<(), AntigravityError> {
        send_prompt_parts(
            &self.writer,
            &self.pending,
            request,
            &self.alive,
            session_id,
            params,
        )
    }

    pub(super) fn notify(&self, method: &str, params: Value) -> Result<(), AntigravityError> {
        if !self.alive.load(Ordering::Acquire) {
            return Err(AntigravityError::ConnectionClosed);
        }
        write_message(
            &self.writer,
            &json!({ "jsonrpc": "2.0", "method": method, "params": params }),
        )
        .map_err(AntigravityError::Transport)
    }
}

pub(super) fn send_prompt_parts(
    writer: &Arc<Mutex<std::process::ChildStdin>>,
    pending: &PendingRequests,
    request: u64,
    alive: &Arc<AtomicBool>,
    session_id: &str,
    params: Value,
) -> Result<(), AntigravityError> {
    if !alive.load(Ordering::Acquire) {
        return Err(AntigravityError::ConnectionClosed);
    }
    let id_key = request.to_string();
    lock(pending).insert(
        id_key.clone(),
        PendingCall::Prompt {
            session_id: session_id.to_owned(),
            request,
        },
    );
    if let Err(error) = write_message(
        writer,
        &json!({ "jsonrpc": "2.0", "id": request, "method": "session/prompt", "params": params }),
    ) {
        lock(pending).remove(&id_key);
        return Err(AntigravityError::Transport(error));
    }
    Ok(())
}

pub(super) fn spawn_connection(
    executable: Result<PathBuf, AntigravityError>,
    project_path: &Path,
    processes: &Arc<ProcessRegistry>,
    approvals: ApprovalBroker,
    questions: QuestionBroker,
    subscribers: EventSubscribers,
) -> Result<Arc<AcpConnection>, AntigravityError> {
    let executable = executable?;
    let mut command = Command::new(&executable);
    command
        .args(["acp", "--stdio"])
        .current_dir(project_path)
        .env("PATH", crate::executable::path(&executable))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = command.spawn().map_err(|error| {
        AntigravityError::Launch(format!(
            "Could not launch `{}` acp --stdio: {error}",
            executable.display()
        ))
    })?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| AntigravityError::Launch("Antigravity ACP stdin unavailable".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AntigravityError::Launch("Antigravity ACP stdout unavailable".into()))?;
    let process_id = processes
        .register(child)
        .map_err(|e| AntigravityError::Launch(e.to_string()))?;

    let writer = Arc::new(Mutex::new(stdin));
    let pending = Arc::new(Mutex::new(HashMap::new()));
    let next_id = Arc::new(AtomicU64::new(1));
    let alive = Arc::new(AtomicBool::new(true));
    let progress = Arc::new(Mutex::new(SessionProgressTracker::default()));
    let replays = Arc::new(Mutex::new(HashMap::new()));
    let commands = Arc::new(Mutex::new(Vec::new()));
    let session_options = Arc::new(Mutex::new(HashMap::new()));
    let session_titles = Arc::new(Mutex::new(HashMap::new()));
    let session_projects = Arc::new(Mutex::new(HashMap::new()));
    let transcripts = Arc::new(Mutex::new(super::titles::TranscriptStore::default()));

    let connection = Arc::new(AcpConnection {
        process_id,
        writer: writer.clone(),
        pending: pending.clone(),
        next_id: next_id.clone(),
        alive: alive.clone(),
        loaded_sessions: Mutex::new(HashSet::new()),
        progress: progress.clone(),
        replays: replays.clone(),
        choices: Arc::new(Mutex::new(None)),
        tier: Arc::new(Mutex::new(None)),
        commands: commands.clone(),
        session_options: session_options.clone(),
        session_titles: session_titles.clone(),
        session_projects: session_projects.clone(),
        session_changes: Mutex::new(HashMap::new()),
        transcripts: transcripts.clone(),
    });

    spawn_reader(
        stdout,
        ReaderContext {
            pending,
            writer,
            next_id,
            alive,
            subscribers,
            approvals,
            questions,
            default_project: project_path.to_path_buf(),
            progress,
            replays,
            commands,
            session_options,
            session_titles,
            session_projects,
            transcripts,
        },
    );

    connection.request(
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientCapabilities": {},
            "clientInfo": {
                "name": "caelum",
                "version": env!("CARGO_PKG_VERSION"),
            }
        }),
    )?;

    Ok(connection)
}

fn resolve_project(
    session_projects: &Arc<Mutex<HashMap<String, String>>>,
    session_id: Option<&str>,
    default_project: &str,
) -> String {
    session_id
        .and_then(|sid| lock(session_projects).get(sid).cloned())
        .unwrap_or_else(|| default_project.to_owned())
}

fn spawn_reader(stdout: std::process::ChildStdout, context: ReaderContext) {
    thread::spawn(move || {
        let ReaderContext {
            pending,
            writer,
            next_id,
            alive,
            subscribers,
            approvals,
            questions,
            default_project,
            progress,
            replays,
            commands,
            session_options,
            session_titles,
            session_projects,
            transcripts,
        } = context;
        let default_project = default_project.to_string_lossy().into_owned();
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = match line {
                Ok(line) => line,
                Err(_) => break,
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(message) = serde_json::from_str::<Value>(trimmed) else {
                continue;
            };
            let has_method = message.get("method").and_then(Value::as_str).is_some();
            let has_id = message.get("id").is_some_and(|id| !id.is_null());

            if has_id && !has_method {
                let id_key = id_to_key(&message["id"]);
                let call = lock(&pending).remove(&id_key);
                let result = if let Some(error) = message.get("error").filter(|e| !e.is_null()) {
                    Err(error["message"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| error.to_string()))
                } else {
                    Ok(message.get("result").cloned().unwrap_or(Value::Null))
                };
                match call {
                    Some(PendingCall::Reply(sender)) => {
                        let _ = sender.send(result);
                    }
                    Some(PendingCall::Prompt {
                        session_id,
                        request,
                    }) => {
                        let usage = result.as_ref().ok().and_then(|r| r.get("usage")).cloned();
                        let mut current = lock(&progress);
                        let Some(effect) = current.on_prompt_replied(&session_id, request, result)
                        else {
                            continue;
                        };
                        let project =
                            resolve_project(&session_projects, Some(&session_id), &default_project);
                        if let Some(usage) = usage {
                            publish_event(
                                &subscribers,
                                AntigravityEvent {
                                    project_path: project.clone(),
                                    session_id: Some(session_id.clone()),
                                    method: "antigravity/turnUsage".into(),
                                    params: json!({"usage":usage}),
                                },
                            );
                        }
                        dispatch_progress_effect(&project, effect, &subscribers, &progress);
                    }
                    None => {}
                }
                continue;
            }

            if has_method && has_id {
                let method = message["method"].as_str().unwrap_or_default().to_owned();
                let id = message["id"].clone();
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                let writer = writer.clone();
                let approvals = approvals.clone();
                let project = resolve_project(
                    &session_projects,
                    params["sessionId"].as_str(),
                    &default_project,
                );
                thread::spawn(move || {
                    handle_server_request(&writer, &approvals, &project, id, &method, &params);
                });
                continue;
            }

            if has_method {
                let method = message["method"].as_str().unwrap_or_default();
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                if method == "session/update" {
                    let project = resolve_project(
                        &session_projects,
                        params["sessionId"].as_str(),
                        &default_project,
                    );
                    handle_session_update(
                        &project,
                        &params,
                        &writer,
                        &pending,
                        &next_id,
                        &alive,
                        &questions,
                        &subscribers,
                        &progress,
                        &replays,
                        &commands,
                        &session_options,
                        &session_titles,
                        &transcripts,
                    );
                }
            }
        }

        alive.store(false, Ordering::Release);
        let mut affected_projects: HashSet<String> =
            lock(&session_projects).values().cloned().collect();
        affected_projects.insert(default_project.clone());
        for project in &affected_projects {
            questions.clear(PROVIDER_NAME, project, None);
        }
        let remaining: Vec<_> = lock(&pending).drain().map(|(_, call)| call).collect();
        for call in remaining {
            match call {
                PendingCall::Reply(sender) => {
                    let _ = sender.send(Err("Antigravity ACP connection closed".into()));
                }
                PendingCall::Prompt {
                    session_id,
                    request,
                } => {
                    let Some(effect) = lock(&progress).on_prompt_replied(
                        &session_id,
                        request,
                        Err("Antigravity ACP connection closed".into()),
                    ) else {
                        continue;
                    };
                    debug_assert!(matches!(effect, ProgressEffect::TurnCompleted { .. }));
                    let project =
                        resolve_project(&session_projects, Some(&session_id), &default_project);
                    publish_event(
                        &subscribers,
                        AntigravityEvent {
                            project_path: project,
                            session_id: Some(session_id),
                            method: "antigravity/turnCompleted".into(),
                            params: json!({
                                "stopReason": "disconnected",
                                "error": "Antigravity ACP connection closed",
                            }),
                        },
                    );
                }
            }
        }
        let disconnected_effects = lock(&progress).on_disconnected();
        for effect in disconnected_effects {
            let sid = match &effect {
                ProgressEffect::TurnYielded { session_id }
                | ProgressEffect::ScheduleSettle { session_id, .. }
                | ProgressEffect::TurnCompleted { session_id, .. } => Some(session_id.as_str()),
            };
            let project = resolve_project(&session_projects, sid, &default_project);
            dispatch_progress_effect(&project, effect, &subscribers, &progress);
        }
        for project in affected_projects {
            publish_event(
                &subscribers,
                AntigravityEvent {
                    project_path: project,
                    session_id: None,
                    method: "antigravity/processExited".into(),
                    params: Value::Null,
                },
            );
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn handle_session_update(
    project: &str,
    params: &Value,
    writer: &Arc<Mutex<std::process::ChildStdin>>,
    pending: &PendingRequests,
    next_id: &Arc<AtomicU64>,
    alive: &Arc<AtomicBool>,
    questions: &QuestionBroker,
    subscribers: &EventSubscribers,
    progress: &Arc<Mutex<SessionProgressTracker>>,
    replays: &super::ReplayBuffers,
    commands: &Arc<Mutex<Vec<caelum_protocol::OpenCodeCommandInfo>>>,
    session_options: &Arc<Mutex<HashMap<String, super::SessionOptionState>>>,
    session_titles: &Arc<Mutex<HashMap<String, String>>>,
    transcripts: &Arc<Mutex<super::titles::TranscriptStore>>,
) {
    let session_id = params["sessionId"].as_str().map(str::to_owned);
    let update = &params["update"];
    let kind = update["sessionUpdate"].as_str().unwrap_or_default();

    if kind == "available_commands_update" {
        let parsed = protocol::parse_available_commands(update);
        if !parsed.is_empty() {
            *lock(commands) = parsed;
        }
    }

    if kind == "current_mode_update" {
        if let (Some(sid), Some(mode)) = (session_id.as_deref(), update["currentModeId"].as_str()) {
            lock(session_options)
                .entry(sid.to_owned())
                .or_default()
                .agent = Some(mode.to_owned());
        }
    }

    if kind == "session_info_update" {
        if let (Some(sid), Some(title)) = (
            session_id.as_deref(),
            update["title"]
                .as_str()
                .map(str::trim)
                .filter(|t| !t.is_empty()),
        ) {
            lock(session_titles).insert(sid.to_owned(), title.to_owned());
        }
    }

    if let Some(sid) = session_id.as_deref() {
        let mut buffers = lock(replays);
        if let Some(buffer) = buffers.get_mut(sid) {
            buffer.push(update.clone());
            return;
        }
    }

    if matches!(kind, "user_message_chunk" | "agent_message_chunk") {
        if let Some(sid) = session_id.as_deref() {
            let text = protocol::chunk_text(update);
            if !text.is_empty() {
                lock(transcripts).record_chunk(sid, kind == "user_message_chunk", &text);
            }
        }
    }

    if matches!(kind, "tool_call" | "tool_call_update") && questions::is_ask_question(update) {
        if let (Some(sid), Some(tool_id)) = (
            session_id.as_deref(),
            update["toolCallId"].as_str().filter(|s| !s.is_empty()),
        ) {
            match update["status"].as_str() {
                Some("pending") => {
                    questions::register_from_update(
                        questions,
                        questions::QuestionReplyContext {
                            project_path: project.to_owned(),
                            session_id: sid.to_owned(),
                            writer: writer.clone(),
                            pending: pending.clone(),
                            next_id: next_id.clone(),
                            alive: alive.clone(),
                            progress: progress.clone(),
                            subscribers: subscribers.clone(),
                        },
                        update,
                    );
                }
                Some("completed" | "failed" | "cancelled" | "interrupted" | "error") => {
                    questions.dismiss(PROVIDER_NAME, project, sid, tool_id);
                }
                _ => {}
            }
        }
    }

    let effect = session_id
        .as_deref()
        .and_then(|sid| lock(progress).on_session_update(sid, kind, update));

    if !kind.is_empty() {
        publish_event(
            subscribers,
            AntigravityEvent {
                project_path: project.to_owned(),
                session_id,
                method: kind.to_owned(),
                params: update.clone(),
            },
        );
    }

    if let Some(effect) = effect {
        dispatch_progress_effect(project, effect, subscribers, progress);
    }
}

pub(super) fn dispatch_progress_effect(
    project: &str,
    effect: ProgressEffect,
    subscribers: &EventSubscribers,
    progress: &Arc<Mutex<SessionProgressTracker>>,
) {
    match effect {
        ProgressEffect::TurnYielded { session_id } => {
            publish_event(
                subscribers,
                AntigravityEvent {
                    project_path: project.to_owned(),
                    session_id: Some(session_id.clone()),
                    method: "antigravity/turnYielded".into(),
                    params: json!({ "sessionId": session_id }),
                },
            );
        }
        ProgressEffect::TurnCompleted {
            session_id,
            stop_reason,
            error,
        } => {
            let params = match error {
                Some(error) => json!({
                    "sessionId": session_id,
                    "stopReason": stop_reason,
                    "error": error,
                }),
                None => json!({
                    "sessionId": session_id,
                    "stopReason": stop_reason,
                }),
            };
            publish_event(
                subscribers,
                AntigravityEvent {
                    project_path: project.to_owned(),
                    session_id: Some(session_id),
                    method: "antigravity/turnCompleted".into(),
                    params,
                },
            );
        }
        ProgressEffect::ScheduleSettle {
            session_id,
            generation,
        } => {
            let project = project.to_owned();
            let subscribers = subscribers.clone();
            let progress = progress.clone();
            thread::spawn(move || {
                thread::sleep(REACTIVE_SETTLE_DELAY);
                let completed = lock(&progress).on_settle_timer(&session_id, generation);
                if let Some(effect) = completed {
                    dispatch_progress_effect(&project, effect, &subscribers, &progress);
                }
            });
        }
    }
}

fn handle_server_request(
    writer: &Arc<Mutex<std::process::ChildStdin>>,
    approvals: &ApprovalBroker,
    project: &str,
    id: Value,
    method: &str,
    params: &Value,
) {
    if method != "session/request_permission" {
        let _ = write_message(
            writer,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("Method not supported: {method}") }
            }),
        );
        return;
    }

    let request_id = id_to_key(&id);
    let session_id = params["sessionId"].as_str().map(str::to_owned);
    let tool = &params["toolCall"];
    let summary = tool["title"]
        .as_str()
        .or_else(|| tool["name"].as_str())
        .unwrap_or("Approve Antigravity action")
        .to_owned();
    let description = if !tool["rawInput"].is_null() {
        serde_json::to_string_pretty(&tool["rawInput"]).unwrap_or_else(|_| summary.clone())
    } else {
        summary.clone()
    };
    let decision = approvals
        .request(PermissionRequest {
            provider: PROVIDER_NAME.into(),
            project_path: project.to_owned(),
            session_id,
            request_id,
            summary,
            description,
            provider_details: params.clone(),
        })
        .map(|waiter| waiter.wait(APPROVAL_TIMEOUT))
        .unwrap_or(ApprovalDecision::Deny);

    let outcome = super::protocol::permission_option(params, decision)
        .map(|option_id| json!({ "outcome": "selected", "optionId": option_id }))
        .unwrap_or_else(|| json!({ "outcome": "cancelled" }));

    let _ = write_message(
        writer,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "outcome": outcome }
        }),
    );
}

pub(super) fn publish_event(subscribers: &EventSubscribers, event: AntigravityEvent) {
    lock(subscribers).retain(|subscriber| subscriber.send(event.clone()).is_ok());
}

fn write_message(
    writer: &Arc<Mutex<std::process::ChildStdin>>,
    value: &Value,
) -> Result<(), String> {
    let mut encoded = serde_json::to_string(value).map_err(|e| e.to_string())?;
    encoded.push('\n');
    let mut stdin = lock(writer);
    stdin
        .write_all(encoded.as_bytes())
        .and_then(|()| stdin.flush())
        .map_err(|e| e.to_string())
}

fn id_to_key(id: &Value) -> String {
    match id {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

impl super::AntigravityProvider {
    pub(crate) fn side_command(
        &self,
        path: &str,
        id: &str,
    ) -> Result<std::process::Command, String> {
        let project = crate::projects::Project::open(path).map_err(|e| e.to_string())?;
        let c = self.connection(&project).map_err(|e| e.to_string())?;
        self.verify_session_owned(&c, &project, id)
            .map_err(|e| e.to_string())?;
        let exe = self.executable().map_err(|e| e.to_string())?;
        let mut cmd = std::process::Command::new(&exe);
        cmd.args([
            "acp",
            "--stdio",
            "--sandbox",
            "--include-default-customizations=false",
        ])
        .env("PATH", crate::executable::path(&exe));
        Ok(cmd)
    }
}
