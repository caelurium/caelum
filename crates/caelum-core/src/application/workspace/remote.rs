use super::{Pending, Workspace};
mod checkout;
mod git;
pub(super) mod handoff;
mod harness;
mod lifetime;
mod link;
mod removal;
mod stats;
#[cfg(test)]
mod tests;
use crate::{
    application::{Command, Reply},
    remote::{Connection, Credential, Operation, Outcome},
};

pub(super) enum Lane {
    Connection,
    OpenProject,
    Picker,
    Handoff,
    ConfirmHandoff(Box<(crate::remote::Source, handoff::VmDestination)>),
    Checkout(String),
    Delete(String, crate::remote::MachineId),
    Feed(String),
    Git(String),
    Stats(String),
    Lifetime(String),
    Interrupt(String, crate::remote::Run),
}

impl Workspace {
    pub fn connect_boat(&mut self, key: String) {
        self.connection_request(Operation::Connect(Credential::new(key)));
    }

    pub fn list_machines(&mut self) {
        self.connection_request(Operation::Inventory);
    }

    pub fn disconnect_boat(&mut self) {
        self.connection_request(Operation::Disconnect);
    }

    fn connection_request(&mut self, op: Operation) {
        if matches!(self.remote.connection, Connection::Loading(_)) {
            return;
        }
        let request = self.request(Command::Remote(op), Pending::Remote(Lane::Connection));
        self.remote.deleted_during_inventory.clear();
        self.remote.connection = Connection::Loading(request);
    }

    pub(super) fn complete_remote(
        &mut self,
        request: u64,
        lane: Lane,
        result: Result<Reply, String>,
    ) {
        if let Lane::OpenProject = lane {
            self.complete_remote_project(request, result);
            return;
        }
        if let Lane::Handoff = lane {
            self.complete_handoff_chat(request, result);
            return;
        }
        let result = result.and_then(|reply| match reply {
            Reply::Remote(value) => Ok(value),
            _ => Err("Unexpected remote machine response".into()),
        });
        match lane {
            Lane::Delete(id, key) => self.complete_deletion(&id, request, &key, result),
            Lane::Picker => self.complete_remote_picker(request, result),
            Lane::ConfirmHandoff(plan) => {
                self.complete_handoff_confirmation(request, plan.0, plan.1, result)
            }
            Lane::Feed(id) => self.complete_feed(&id, request, result),
            Lane::Git(id) => self.complete_git(&id, request, result),
            Lane::Stats(id) => self.complete_stats(&id, request, result),
            Lane::Lifetime(id) => self.complete_lifetime(&id, request, result),
            Lane::Interrupt(id, run) => self.complete_interrupt(&id, request, &run, result),
            Lane::Checkout(id) => self.complete_checkout(&id, request, result),
            Lane::Connection if matches!(self.remote.connection, Connection::Loading(id) if id == request) =>
            {
                self.remote.connection = match result {
                    Ok(Outcome::Inventory(mut inventory)) => {
                        inventory
                            .machines
                            .retain(|m| !self.remote.deleted_during_inventory.contains(&m.key));
                        self.remote.inventory = Some(inventory);
                        if self.remote.picker.is_none() {
                            if let Some(h) = self.vm_handoff() {
                                self.discover_remote_source(h.project.clone(), true);
                            }
                        }
                        Connection::Idle
                    }
                    Ok(Outcome::Disconnected) => {
                        self.remote.inventory = None;
                        Connection::Idle
                    }
                    Err(error) => Connection::Failed(error),
                    _ => Connection::Failed("Unexpected connection response".into()),
                };
            }
            _ => {}
        }
    }
}
