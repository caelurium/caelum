//! Per-checkout managed command runner with bounded output and clean process cleanup.

use std::{
    collections::HashMap,
    io::Read,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
        Arc, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use tokio::sync::mpsc;

use crate::{
    application::{presentation::now, Event},
    blocking_actor::BlockingActor,
    processes::{ProcessId, ProcessRegistry},
    projects::Project,
};

#[cfg(test)]
mod tests;

pub const OUTPUT_LIMIT: usize = 64 * 1024;
const COMMAND_LIMIT: usize = 8 * 1024;
const HISTORY_LIMIT: usize = 20;
const STREAM_THROTTLE: Duration = Duration::from_millis(60);

/// Bounded UTF-8 tail buffer retaining the newest `OUTPUT_LIMIT` bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RingBuffer {
    bytes: Vec<u8>,
    truncated: bool,
}

impl RingBuffer {
    pub fn push(&mut self, chunk: &[u8]) {
        if chunk.len() >= OUTPUT_LIMIT {
            self.bytes.clear();
            self.bytes
                .extend_from_slice(&chunk[chunk.len() - OUTPUT_LIMIT..]);
            self.truncated = true;
            return;
        }
        let overflow = self
            .bytes
            .len()
            .saturating_add(chunk.len())
            .saturating_sub(OUTPUT_LIMIT);
        if overflow > 0 {
            self.bytes.drain(..overflow);
            self.truncated = true;
        }
        self.bytes.extend_from_slice(chunk);
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }

    pub fn text(&self) -> String {
        if !self.truncated {
            return String::from_utf8_lossy(&self.bytes).into_owned();
        }
        // Skip up to 3 leading UTF-8 continuation bytes left after front-truncation.
        let mut start = 0;
        while start < self.bytes.len().min(3) && (self.bytes[start] & 0b1100_0000) == 0b1000_0000 {
            start += 1;
        }
        String::from_utf8_lossy(&self.bytes[start..]).into_owned()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RunState {
    #[default]
    Idle,
    Running {
        run_id: u64,
        command: String,
        started_at: u64,
        output: String,
        truncated: bool,
    },
    Exited {
        run_id: u64,
        command: String,
        code: Option<i32>,
        duration_ms: u64,
        output: String,
        truncated: bool,
    },
    Cancelled {
        run_id: u64,
        command: String,
        duration_ms: u64,
        output: String,
        truncated: bool,
    },
    Failed {
        run_id: u64,
        command: String,
        error: String,
    },
}

impl RunState {
    pub fn run_id(&self) -> Option<u64> {
        match self {
            Self::Idle => None,
            Self::Running { run_id, .. }
            | Self::Exited { run_id, .. }
            | Self::Cancelled { run_id, .. }
            | Self::Failed { run_id, .. } => Some(*run_id),
        }
    }

    pub fn running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }

    pub fn command(&self) -> Option<&str> {
        match self {
            Self::Idle => None,
            Self::Running { command, .. }
            | Self::Exited { command, .. }
            | Self::Cancelled { command, .. }
            | Self::Failed { command, .. } => Some(command),
        }
    }

    pub fn output(&self) -> Option<(&str, bool)> {
        match self {
            Self::Running {
                output, truncated, ..
            }
            | Self::Exited {
                output, truncated, ..
            }
            | Self::Cancelled {
                output, truncated, ..
            } => Some((output, *truncated)),
            Self::Idle | Self::Failed { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerEvent {
    Output {
        project: String,
        run_id: u64,
        output: String,
        truncated: bool,
    },
    Finished {
        project: String,
        run_id: u64,
        code: Option<i32>,
        duration_ms: u64,
        output: String,
        truncated: bool,
        cancelled: bool,
    },
    Failed {
        project: String,
        run_id: u64,
        error: String,
    },
}

#[derive(Debug, Clone, Default)]
pub struct CheckoutRun {
    pub state: RunState,
    pub history: Vec<String>,
}

#[derive(Debug, Default)]
pub struct WorkspaceRunner {
    checkouts: HashMap<String, CheckoutRun>,
    next_id: u64,
}

impl WorkspaceRunner {
    pub fn get(&self, project: &str) -> Option<&CheckoutRun> {
        self.checkouts.get(project)
    }

    pub fn start(&mut self, project: &str, command: String) -> Option<(u64, String)> {
        let trimmed = command.trim().to_owned();
        if trimmed.is_empty() || trimmed.len() > COMMAND_LIMIT {
            return None;
        }
        self.next_id += 1;
        let run_id = self.next_id;
        let entry = self.checkouts.entry(project.to_owned()).or_default();
        entry.history.retain(|item| item != &trimmed);
        entry.history.insert(0, trimmed.clone());
        entry.history.truncate(HISTORY_LIMIT);
        entry.state = RunState::Running {
            run_id,
            command: trimmed.clone(),
            started_at: now(),
            output: String::new(),
            truncated: false,
        };
        Some((run_id, trimmed))
    }

    pub fn running_id(&self, project: &str) -> Option<u64> {
        match self.checkouts.get(project).map(|c| &c.state) {
            Some(RunState::Running { run_id, .. }) => Some(*run_id),
            _ => None,
        }
    }

    pub fn apply(&mut self, event: RunnerEvent) -> bool {
        match event {
            RunnerEvent::Output {
                project,
                run_id,
                output,
                truncated,
            } => {
                let Some(entry) = self.checkouts.get_mut(&project) else {
                    return false;
                };
                if let RunState::Running {
                    run_id: current,
                    output: current_out,
                    truncated: current_trunc,
                    ..
                } = &mut entry.state
                {
                    if *current == run_id {
                        *current_out = output;
                        *current_trunc = truncated;
                        return true;
                    }
                }
                false
            }
            RunnerEvent::Finished {
                project,
                run_id,
                code,
                duration_ms,
                output,
                truncated,
                cancelled,
            } => {
                let Some(entry) = self.checkouts.get_mut(&project) else {
                    return false;
                };
                let command = match &entry.state {
                    RunState::Running {
                        run_id: current,
                        command,
                        ..
                    } if *current == run_id => command.clone(),
                    _ => return false,
                };
                entry.state = if cancelled {
                    RunState::Cancelled {
                        run_id,
                        command,
                        duration_ms,
                        output,
                        truncated,
                    }
                } else {
                    RunState::Exited {
                        run_id,
                        command,
                        code,
                        duration_ms,
                        output,
                        truncated,
                    }
                };
                true
            }
            RunnerEvent::Failed {
                project,
                run_id,
                error,
            } => {
                let Some(entry) = self.checkouts.get_mut(&project) else {
                    return false;
                };
                let command = match &entry.state {
                    RunState::Running {
                        run_id: current,
                        command,
                        ..
                    } if *current == run_id => command.clone(),
                    _ => return false,
                };
                entry.state = RunState::Failed {
                    run_id,
                    command,
                    error,
                };
                true
            }
        }
    }
}

type Observer = (SyncSender<Event>, Arc<dyn Fn() + Send + Sync>);

struct ActiveRun {
    run_id: u64,
    process: ProcessId,
    #[cfg(unix)]
    process_group: i32,
    cancelled: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct Backend {
    processes: Arc<ProcessRegistry>,
    active: BlockingActor<Option<HashMap<String, ActiveRun>>>,
    observer: Arc<OnceLock<Observer>>,
}

impl Backend {
    pub fn new(processes: Arc<ProcessRegistry>) -> Self {
        Self {
            processes,
            active: BlockingActor::spawn("caelum-runner", Some(HashMap::new())),
            observer: Arc::default(),
        }
    }

    pub(crate) fn observe(&self, sender: SyncSender<Event>, wake: Arc<dyn Fn() + Send + Sync>) {
        let _ = self.observer.set((sender, wake));
    }

    fn emit(&self, event: RunnerEvent) {
        if let Some((sender, wake)) = self.observer.get() {
            if sender.try_send(Event::Runner(event)).is_ok() {
                wake();
            }
        }
    }

    pub fn start(&self, project: &str, run_id: u64, command: &str) -> Result<(), String> {
        let trimmed = command.trim();
        if trimmed.is_empty() || trimmed.len() > COMMAND_LIMIT {
            return Err("Command must be between 1 and 8192 bytes.".into());
        }
        let opened = Project::open(project).map_err(|e| e.to_string())?;
        let backend = self.clone();
        let run_project = project.to_owned();
        let command = trimmed.to_owned();
        let result = self
            .active
            .call(move |active| {
                let active = active.as_mut().ok_or("Command runner is shutting down.")?;
                backend.start_run(active, run_project, run_id, command, opened)
            })
            .ok_or("Command runner has stopped.")?;
        if let Err(error) = &result {
            self.emit(RunnerEvent::Failed {
                project: project.to_owned(),
                run_id,
                error: error.clone(),
            });
        }
        result
    }

    fn start_run(
        &self,
        active: &mut HashMap<String, ActiveRun>,
        project: String,
        run_id: u64,
        command: String,
        opened: Project,
    ) -> Result<(), String> {
        if let Some(run) = active.remove(&project) {
            self.terminate(run);
        }

        let mut cmd = shell_command(&command);
        #[cfg(unix)]
        cmd.process_group(0);
        cmd.current_dir(opened.path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|error| error.to_string())?;
        #[cfg(unix)]
        let process_group = child.id() as i32;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let process = self
            .processes
            .register(child)
            .map_err(|error| error.to_string())?;
        let cancelled = Arc::new(AtomicBool::new(false));
        active.insert(
            project.clone(),
            ActiveRun {
                run_id,
                process,
                #[cfg(unix)]
                process_group,
                cancelled: cancelled.clone(),
            },
        );

        let backend = self.clone();
        let run_project = project.clone();
        let started = Instant::now();
        let worker = thread::Builder::new()
            .name(format!("caelum-runner-{run_id}"))
            .spawn(move || {
                let project = run_project;
                let mut output = RingBuffer::default();
                let mut last_update: Option<Instant> = None;
                let (chunks, mut mailbox) = mpsc::channel::<Vec<u8>>(32);
                let t_out = stdout.map(|pipe| pump_output(pipe, chunks.clone()));
                let t_err = stderr.map(|pipe| pump_output(pipe, chunks));
                while let Some(chunk) = mailbox.blocking_recv() {
                    output.push(&chunk);
                    if last_update.is_none_or(|last| last.elapsed() >= STREAM_THROTTLE) {
                        last_update = Some(Instant::now());
                        backend.emit(RunnerEvent::Output {
                            project: project.clone(),
                            run_id,
                            output: output.text(),
                            truncated: output.truncated(),
                        });
                    }
                }
                for pump in t_out.into_iter().chain(t_err) {
                    let _ = pump.join();
                }

                let mut code = None;
                for _ in 0..40 {
                    match backend.processes.try_wait_one(process) {
                        Ok(Some(status)) => {
                            code = status.code();
                            break;
                        }
                        Ok(None) => thread::sleep(Duration::from_millis(10)),
                        Err(_) => break,
                    }
                }
                if code.is_none() {
                    if let Ok(status) = backend.processes.terminate(process) {
                        code = status.code();
                    }
                }
                backend.take_run(&project, run_id);
                let duration_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
                backend.emit(RunnerEvent::Finished {
                    project,
                    run_id,
                    code,
                    duration_ms,
                    output: output.text(),
                    truncated: output.truncated(),
                    cancelled: cancelled.load(Ordering::Acquire),
                });
            });
        if let Err(error) = worker {
            if let Some(run) = active.remove(&project) {
                self.terminate(run);
            }
            return Err(error.to_string());
        }
        Ok(())
    }

    pub fn stop(&self, project: &str, run_id: u64) {
        if let Some(run) = self.take_run(project, run_id) {
            self.terminate(run);
        }
    }

    fn take_run(&self, project: &str, run_id: u64) -> Option<ActiveRun> {
        let project = project.to_owned();
        self.active
            .call(move |active| {
                let active = active.as_mut()?;
                if active.get(&project).is_some_and(|run| run.run_id == run_id) {
                    active.remove(&project)
                } else {
                    None
                }
            })
            .flatten()
    }

    pub fn shutdown(&self) {
        if let Some(active) = self.active.call(Option::take).flatten() {
            for run in active.into_values() {
                self.terminate(run);
            }
        }
    }

    fn terminate(&self, run: ActiveRun) {
        run.cancelled.store(true, Ordering::Release);
        #[cfg(unix)]
        if let Some(process_group) = rustix::process::Pid::from_raw(run.process_group) {
            let _ =
                rustix::process::kill_process_group(process_group, rustix::process::Signal::KILL);
        }
        let _ = self.processes.terminate(run.process);
    }
}

fn pump_output(
    mut pipe: impl Read + Send + 'static,
    chunks: mpsc::Sender<Vec<u8>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0; 4096];
        while let Ok(n) = pipe.read(&mut buffer) {
            if n == 0 || chunks.blocking_send(buffer[..n].to_vec()).is_err() {
                break;
            }
        }
    })
}

fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd
    }
}
