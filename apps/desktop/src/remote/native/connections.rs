use caelum_core::{
    application::{Command, Event, Runtime, Store, Task, Workspace},
    remote::native::{Binding, Client, NativeTarget, Notice, Status, checkout_id},
};
use rune::Waker;
use std::collections::BTreeMap;

struct Connection {
    binding: Binding,
    phase: Phase,
}

enum Phase {
    Live(LiveConnection),
    StartFailed {
        status: Status,
        announced: bool,
    },
    Fixture(Status),
    #[cfg(test)]
    TestConnecting {
        reads: Vec<(u64, Command)>,
    },
}

struct LiveConnection {
    client: Client,
    state: LiveState,
}

enum LiveState {
    Connecting { reads: Vec<(u64, Command)> },
    Connected,
    Disconnected(Status),
}

impl Connection {
    fn status(&self) -> &Status {
        match &self.phase {
            Phase::Live(live) => match &live.state {
                LiveState::Connecting { .. } => &Status::Connecting,
                LiveState::Connected => &Status::Connected,
                LiveState::Disconnected(status) => status,
            },
            Phase::StartFailed { status, .. } | Phase::Fixture(status) => status,
            #[cfg(test)]
            Phase::TestConnecting { .. } => &Status::Connecting,
        }
    }
}

#[derive(Default)]
pub(crate) struct Connections(BTreeMap<String, Connection>);

impl Connections {
    pub(super) fn ensure(&mut self, id: &str, target: NativeTarget, wake: Waker) {
        if self.0.contains_key(id) {
            return;
        }
        let Ok(binding) = Binding::new(id, &target) else {
            return;
        };
        let phase = match Client::start(target, Store::default_path(), move || wake()) {
            Ok(client) => Phase::Live(LiveConnection {
                client,
                state: LiveState::Connecting { reads: Vec::new() },
            }),
            Err(error) => Phase::StartFailed {
                status: Status::Disconnected(error),
                announced: false,
            },
        };
        self.0.insert(id.into(), Connection { binding, phase });
    }
    pub(crate) fn status(&self, id: &str) -> Option<&Status> {
        Some(self.0.get(id)?.status())
    }

    pub(super) fn retry(&mut self, id: &str) {
        if let Some(Connection {
            phase: Phase::Live(live),
            ..
        }) = self.0.get(id)
        {
            live.client.reconnect();
        } else {
            self.0.remove(id);
        }
    }

    pub(crate) fn submit(
        &mut self,
        local: &mut Runtime,
        app: &Workspace,
        wake: Option<Waker>,
        request: u64,
        mut command: Command,
    ) -> Result<(), String> {
        let Some(id) = Binding::owner(&mut command).and_then(|p| checkout_id(p)) else {
            return local.submit(request, command);
        };
        if !self.0.contains_key(id)
            && app.can_open_vm_session(id)
            && super::unavailable(app, id).is_none()
            && let (Some(target), Some(wake)) = (
                app.remote
                    .saved
                    .checkouts
                    .get(id)
                    .and_then(|checkout| checkout.native_target()),
                wake,
            )
        {
            self.ensure(id, target, wake);
        }
        let lane = self
            .0
            .get_mut(id)
            .ok_or("Could not connect to the ready VM checkout.")?;
        let command = lane.binding.outgoing(command)?;
        let queueable = matches!(
            &command,
            Command::Choices(_)
                | Command::Start(_, _)
                | Command::Details(_)
                | Command::Resume(_)
                | Command::List(_)
                | Command::ListArchived(_)
                | Command::History(..)
                | Command::CodexControlHistory(..)
                | Command::Export(..)
                | Command::Send(..)
                | Command::Task(
                    Task::AttachPullRequest { .. }
                        | Task::Details { .. }
                        | Task::EncodeImage { .. }
                        | Task::Feedback { .. }
                        | Task::Image { .. }
                        | Task::Merge { .. }
                        | Task::ReviewCache { .. }
                        | Task::ReviewFiles { .. }
                        | Task::SetAutoMerge { .. }
                        | Task::UpdateBranch { .. }
                        | Task::Worktrees { .. }
                        | Task::Directory { .. }
                        | Task::Git { .. }
                        | Task::Diff { .. }
                        | Task::File { .. }
                        | Task::Search { .. }
                        | Task::Discover { .. }
                        | Task::Completions { .. }
                )
        );
        match &mut lane.phase {
            Phase::Live(LiveConnection {
                state: LiveState::Connecting { reads },
                ..
            }) if queueable && reads.len() < 128 => {
                reads.push((request, command));
                Ok(())
            }
            #[cfg(test)]
            Phase::TestConnecting { reads } if queueable && reads.len() < 128 => {
                reads.push((request, command));
                Ok(())
            }
            Phase::Live(LiveConnection {
                client,
                state: LiveState::Connected,
            }) => client.submit(request, command),
            Phase::Live(_) | Phase::StartFailed { .. } | Phase::Fixture(_) => {
                Err("Reconnect to the VM before trying again.".into())
            }
            #[cfg(test)]
            Phase::TestConnecting { .. } => Err("Reconnect to the VM before trying again.".into()),
        }
    }

    pub(super) fn drain(&mut self, app: &Workspace) -> Vec<(String, Notice)> {
        let mut batch = Vec::new();
        self.0.retain(|id, lane| {
            if let Phase::StartFailed { status, announced } = &mut lane.phase
                && !*announced
            {
                *announced = true;
                batch.push((id.clone(), Notice::Status(status.clone())));
            }
            if let Some(reason) = super::unavailable(app, id) {
                batch.push((
                    id.clone(),
                    Notice::Status(Status::Disconnected(reason.reason().into())),
                ));
                return false;
            }
            if let Phase::Live(live) = &mut lane.phase {
                for notice in live.client.drain() {
                    let notice = match notice {
                        Notice::Event(e) => {
                            let Some(e) = lane.binding.incoming(*e) else {
                                continue;
                            };
                            Notice::Event(Box::new(e))
                        }
                        Notice::Status(status) => {
                            let queued = match std::mem::replace(
                                &mut live.state,
                                match &status {
                                    Status::Connecting => LiveState::Connecting {
                                        reads: Vec::new(),
                                    },
                                    Status::Connected => LiveState::Connected,
                                    Status::Disconnected(_) => {
                                        LiveState::Disconnected(status.clone())
                                    }
                                },
                            ) {
                                LiveState::Connecting { reads } => reads,
                                LiveState::Connected | LiveState::Disconnected(_) => Vec::new(),
                            };
                            match &status {
                                Status::Connected => {
                                    for (request, command) in queued {
                                        if let Err(e) = live.client.submit(request, command) {
                                            batch.push((
                                                id.clone(),
                                                Notice::Event(Box::new(Event::Completed(
                                                    request,
                                                    Err(e),
                                                ))),
                                            ));
                                        }
                                    }
                                }
                                Status::Disconnected(_) => {
                                    for (request, _) in queued {
                                        batch.push((
                                            id.clone(),
                                            Notice::Event(Box::new(Event::Completed(
                                                request,
                                                Err("The VM disconnected before this action was sent."
                                                    .into()),
                                            ))),
                                        ));
                                    }
                                }
                                Status::Connecting => {
                                    if let LiveState::Connecting { reads } = &mut live.state {
                                        *reads = queued;
                                    }
                                }
                            }
                            Notice::Status(status)
                        }
                    };
                    batch.push((id.clone(), notice));
                }
            }
            true
        });
        batch
    }

    pub(crate) fn fixture(&mut self, id: &str, target: NativeTarget, offline: bool) {
        let status = if offline {
            Status::Disconnected("VM disconnected. Your draft is saved.".into())
        } else {
            Status::Connected
        };
        self.0.insert(
            id.into(),
            Connection {
                binding: Binding::new(id, &target).unwrap(),
                phase: Phase::Fixture(status),
            },
        );
    }
}

#[cfg(test)]
#[path = "connections_tests.rs"]
mod tests;
