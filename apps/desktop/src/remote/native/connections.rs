use caelum_core::{
    application::{Command, Event, Runtime, Store, Task, Workspace},
    remote::native::{Binding, Client, NativeTarget, Notice, Status, checkout_id},
};
use rune::Waker;
use std::collections::BTreeMap;

struct Connection {
    binding: Binding,
    status: Status,
    client: Option<Client>,
    reads: Vec<(u64, Command)>,
    initial: Option<Status>,
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
        let (client, status) = match Client::start(target, Store::default_path(), move || wake()) {
            Ok(c) => (Some(c), Status::Connecting),
            Err(e) => (None, Status::Disconnected(e)),
        };
        let initial = client.is_none().then(|| status.clone());
        self.0.insert(
            id.into(),
            Connection {
                binding,
                status,
                client,
                reads: vec![],
                initial,
            },
        );
    }
    pub(crate) fn status(&self, id: &str) -> Option<&Status> {
        Some(&self.0.get(id)?.status)
    }

    pub(super) fn retry(&mut self, id: &str) {
        if let Some(c) = self.0.get(id).and_then(|c| c.client.as_ref()) {
            c.reconnect();
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
        if lane.status == Status::Connecting
            && lane.reads.len() < 128
            && matches!(
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
            )
        {
            lane.reads.push((request, command));
            return Ok(());
        }
        if lane.status != Status::Connected {
            return Err("Reconnect to the VM before trying again.".into());
        }
        lane.client
            .as_ref()
            .ok_or("Reconnect to the VM before trying again.")?
            .submit(request, command)
    }

    pub(super) fn drain(&mut self, app: &Workspace) -> Vec<(String, Notice)> {
        let mut batch = Vec::new();
        self.0.retain(|id, lane| {
            if let Some(status) = lane.initial.take() {
                batch.push((id.clone(), Notice::Status(status)));
            }
            if let Some(reason) = super::unavailable(app, id) {
                batch.push((
                    id.clone(),
                    Notice::Status(Status::Disconnected(reason.reason().into())),
                ));
                return false;
            }
            if let Some(client) = &mut lane.client {
                for notice in client.drain() {
                    let notice = match notice {
                        Notice::Event(e) => {
                            let Some(e) = lane.binding.incoming(*e) else {
                                continue;
                            };
                            Notice::Event(Box::new(e))
                        }
                        Notice::Status(s) => {
                            lane.status = s.clone();
                            if s == Status::Connected {
                                for (request, command) in lane.reads.drain(..) {
                                    if let Err(e) = client.submit(request, command) {
                                        batch.push((
                                            id.clone(),
                                            Notice::Event(Box::new(Event::Completed(
                                                request,
                                                Err(e),
                                            ))),
                                        ));
                                    }
                                }
                            } else if matches!(s, Status::Disconnected(_)) {
                                for (request, _) in lane.reads.drain(..) {
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
                            Notice::Status(s)
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
        self.0.insert(
            id.into(),
            Connection {
                binding: Binding::new(id, &target).unwrap(),
                client: None,
                reads: vec![],
                initial: None,
                status: if offline {
                    Status::Disconnected("VM disconnected. Your draft is saved.".into())
                } else {
                    Status::Connected
                },
            },
        );
    }
}

#[cfg(test)]
#[path = "connections_tests.rs"]
mod tests;
