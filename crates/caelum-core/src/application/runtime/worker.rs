use super::{Command, Event, Execute, Task};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
        Arc,
    },
    thread::{self, JoinHandle},
};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinSet,
};

pub(super) type Request = (u64, Command);

const NORMAL_LIMIT: usize = 8;
const HISTORY_LIMIT: usize = 2;
const ACTIONS_LIMIT: usize = 4;
const CONTROL_LIMIT: usize = 2;

pub(super) struct Mailboxes {
    normal: mpsc::Sender<Request>,
    history: mpsc::Sender<Request>,
    actions: mpsc::Sender<Request>,
    control: mpsc::Sender<Request>,
}

impl Mailboxes {
    pub(super) fn try_send(&self, id: u64, command: Command) -> Result<(), String> {
        let queue = if matches!(
            command,
            Command::Cancel(_)
                | Command::StopCommand { .. }
                | Command::Remote(crate::remote::Operation::Interrupt(..))
                | Command::Task(Task::Shell { stop: true, .. })
                | Command::Approve(..)
                | Command::Answer(..)
                | Command::Save(..)
                | Command::Load(_)
        ) {
            &self.control
        } else if matches!(
            command,
            Command::Resume(_)
                | Command::History(..)
                | Command::HandoffChat(_)
                | Command::CodexControlHistory(..)
                | Command::Inbox(_)
        ) {
            &self.history
        } else if matches!(
            command,
            Command::Open(_)
                | Command::Archive(..)
                | Command::DeleteSession(..)
                | Command::Task(
                    Task::RemoveWorktree { .. }
                        | Task::RenameWorktree { .. }
                        | Task::CreateWorktree { .. }
                )
        ) {
            &self.actions
        } else {
            &self.normal
        };
        queue
            .try_send((id, command))
            .map_err(|_| "Too many pending requests; try again shortly".into())
    }
}

pub(super) struct Workers {
    thread: JoinHandle<()>,
    stop: oneshot::Sender<()>,
}

impl Workers {
    pub(super) fn start(
        execute: Execute,
        sender: SyncSender<Event>,
        wake: Arc<dyn Fn() + Send + Sync>,
        closed: Arc<AtomicBool>,
    ) -> (Mailboxes, Self) {
        let (normal_tx, normal) = mpsc::channel(1024);
        let (history_tx, history) = mpsc::channel(128);
        let (actions_tx, actions) = mpsc::channel(128);
        let (control_tx, control) = mpsc::channel(128);
        let (stop, stopped) = oneshot::channel();
        let thread = thread::Builder::new()
            .name("caelum-commands".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .max_blocking_threads(
                        NORMAL_LIMIT + HISTORY_LIMIT + ACTIONS_LIMIT + CONTROL_LIMIT,
                    )
                    .build()
                    .expect("start command runtime");
                let run = move |(id, command): Request| {
                    if closed.load(Ordering::Acquire) {
                        return;
                    }
                    let _context = crate::diagnostics::context(id);
                    crate::diagnostics::command("started", id, &command);
                    let result = execute(&command);
                    crate::diagnostics::command("finished", id, &command);
                    if sender.send(Event::Completed(id, result)).is_ok() {
                        wake();
                    }
                };
                runtime.block_on(async {
                    tokio::select! {
                        biased;
                        _ = stopped => {}
                        _ = async {
                            // History, user actions, and controls keep their own capacity
                            // even when catalogues, diffs, and discovery are blocked.
                            tokio::join!(
                                dispatch(normal, NORMAL_LIMIT, run.clone()),
                                dispatch(history, HISTORY_LIMIT, run.clone()),
                                dispatch(actions, ACTIONS_LIMIT, run.clone()),
                                dispatch(control, CONTROL_LIMIT, run),
                            );
                        } => {}
                    }
                });
                // Dropping the runtime waits for blocking work already running.
            })
            .expect("start command dispatcher");
        (
            Mailboxes {
                normal: normal_tx,
                history: history_tx,
                actions: actions_tx,
                control: control_tx,
            },
            Self { thread, stop },
        )
    }

    pub(super) fn stop(self) -> JoinHandle<()> {
        let _ = self.stop.send(());
        self.thread
    }
}

async fn dispatch(
    mut mailbox: mpsc::Receiver<Request>,
    limit: usize,
    run: impl Fn(Request) + Clone + Send + 'static,
) {
    let mut running = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = running.join_next(), if !running.is_empty() => {}
            request = mailbox.recv(), if running.len() < limit => {
                let Some(request) = request else { break };
                let run = run.clone();
                running.spawn_blocking(move || run(request));
            }
        }
    }
}
