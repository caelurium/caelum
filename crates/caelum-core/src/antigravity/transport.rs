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
    lock, protocol, AcpConnection, AntigravityError, EventSubscribers, PendingCall, ReaderContext,
    APPROVAL_TIMEOUT, PROVIDER_NAME, REQUEST_TIMEOUT,
};
use crate::{approvals::ApprovalBroker, processes::ProcessRegistry};

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
        session_id: &str,
        params: Value,
    ) -> Result<(), AntigravityError> {
        if !self.alive.load(Ordering::Acquire) {
            return Err(AntigravityError::ConnectionClosed);
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id_key = id.to_string();
        lock(&self.pending).insert(
            id_key.clone(),
            PendingCall::Prompt {
                session_id: session_id.to_owned(),
            },
        );
        if let Err(error) = write_message(
            &self.writer,
            &json!({ "jsonrpc": "2.0", "id": id, "method": "session/prompt", "params": params }),
        ) {
            lock(&self.pending).remove(&id_key);
            return Err(AntigravityError::Transport(error));
        }
        Ok(())
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

pub(super) fn spawn_connection(
    executable: Result<PathBuf, AntigravityError>,
    project_path: &Path,
    processes: &Arc<ProcessRegistry>,
    approvals: ApprovalBroker,
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
    let alive = Arc::new(AtomicBool::new(true));
    let working_sessions = Arc::new(Mutex::new(HashSet::new()));
    let replays = Arc::new(Mutex::new(HashMap::new()));
    let commands = Arc::new(Mutex::new(Vec::new()));
    let session_options = Arc::new(Mutex::new(HashMap::new()));

    let connection = Arc::new(AcpConnection {
        process_id,
        writer: writer.clone(),
        pending: pending.clone(),
        next_id: AtomicU64::new(1),
        alive: alive.clone(),
        loaded_sessions: Mutex::new(HashSet::new()),
        working_sessions: working_sessions.clone(),
        replays: replays.clone(),
        choices: Arc::new(Mutex::new(None)),
        commands: commands.clone(),
        session_options: session_options.clone(),
        session_changes: Mutex::new(HashMap::new()),
    });

    spawn_reader(
        stdout,
        ReaderContext {
            pending,
            writer,
            alive,
            subscribers,
            approvals,
            project_path: project_path.to_path_buf(),
            working_sessions,
            replays,
            commands,
            session_options,
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

fn spawn_reader(stdout: std::process::ChildStdout, context: ReaderContext) {
    thread::spawn(move || {
        let ReaderContext {
            pending,
            writer,
            alive,
            subscribers,
            approvals,
            project_path,
            working_sessions,
            replays,
            commands,
            session_options,
        } = context;
        let project = project_path.to_string_lossy().into_owned();
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
                    Some(PendingCall::Prompt { session_id }) => {
                        lock(&working_sessions).remove(&session_id);
                        let params = match result {
                            Ok(res) => json!({
                                "sessionId": session_id,
                                "stopReason": res["stopReason"].as_str().unwrap_or("end_turn"),
                            }),
                            Err(error) => json!({
                                "sessionId": session_id,
                                "stopReason": "error",
                                "error": error,
                            }),
                        };
                        publish_event(
                            &subscribers,
                            AntigravityEvent {
                                project_path: project.clone(),
                                session_id: Some(session_id),
                                method: "antigravity/turnCompleted".into(),
                                params,
                            },
                        );
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
                let project = project.clone();
                thread::spawn(move || {
                    handle_server_request(&writer, &approvals, &project, id, &method, &params);
                });
                continue;
            }

            if has_method {
                let method = message["method"].as_str().unwrap_or_default();
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                if method == "session/update" {
                    handle_session_update(
                        &project,
                        &params,
                        &subscribers,
                        &replays,
                        &commands,
                        &session_options,
                    );
                }
            }
        }

        alive.store(false, Ordering::Release);
        let remaining: Vec<_> = lock(&pending).drain().map(|(_, call)| call).collect();
        for call in remaining {
            match call {
                PendingCall::Reply(sender) => {
                    let _ = sender.send(Err("Antigravity ACP connection closed".into()));
                }
                PendingCall::Prompt { session_id } => {
                    lock(&working_sessions).remove(&session_id);
                    publish_event(
                        &subscribers,
                        AntigravityEvent {
                            project_path: project.clone(),
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
        lock(&working_sessions).clear();
        publish_event(
            &subscribers,
            AntigravityEvent {
                project_path: project,
                session_id: None,
                method: "antigravity/processExited".into(),
                params: Value::Null,
            },
        );
    });
}

fn handle_session_update(
    project: &str,
    params: &Value,
    subscribers: &EventSubscribers,
    replays: &super::ReplayBuffers,
    commands: &Arc<Mutex<Vec<caelum_protocol::OpenCodeCommandInfo>>>,
    session_options: &Arc<Mutex<HashMap<String, super::SessionOptionState>>>,
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

    if let Some(sid) = session_id.as_deref() {
        let mut buffers = lock(replays);
        if let Some(buffer) = buffers.get_mut(sid) {
            buffer.push(update.clone());
            return;
        }
    }

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
    let options = params["options"].as_array().cloned().unwrap_or_default();
    let allow_option = options
        .iter()
        .find(|o| matches!(o["kind"].as_str(), Some("allow_once" | "allow_always")))
        .and_then(|o| o["optionId"].as_str())
        .map(str::to_owned);
    let reject_option = options
        .iter()
        .find(|o| matches!(o["kind"].as_str(), Some("reject_once" | "reject_always")))
        .and_then(|o| o["optionId"].as_str())
        .map(str::to_owned);

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

    let outcome = match decision {
        ApprovalDecision::AllowOnce => allow_option
            .map(|option_id| json!({ "outcome": "selected", "optionId": option_id }))
            .unwrap_or_else(|| json!({ "outcome": "cancelled" })),
        ApprovalDecision::Deny => reject_option
            .map(|option_id| json!({ "outcome": "selected", "optionId": option_id }))
            .unwrap_or_else(|| json!({ "outcome": "cancelled" })),
    };

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
