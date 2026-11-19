use super::{Pending, Workspace};
mod checkout;
mod git;
mod harness;
#[cfg(test)]
mod tests;
use crate::{
    application::{Command, Reply},
    remote::{Connection, Credential, Operation, Outcome},
};

pub(super) enum Lane {
    Connection,
    Picker,
    Checkout(String),
    Feed(String),
    Git(String),
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
        self.remote.connection = Connection::Loading(request);
    }

    pub(super) fn complete_remote(
        &mut self,
        request: u64,
        lane: Lane,
        result: Result<Reply, String>,
    ) {
        let result = result.and_then(|reply| match reply {
            Reply::Remote(value) => Ok(value),
            _ => Err("Unexpected remote machine response".into()),
        });
        match lane {
            Lane::Picker => self.complete_remote_picker(request, result),
            Lane::Feed(id) => self.complete_feed(&id, request, result),
            Lane::Git(id) => self.complete_git(&id, request, result),
            Lane::Interrupt(id, run) => self.complete_interrupt(&id, request, &run, result),
            Lane::Checkout(id) => self.complete_checkout(&id, request, result),
            Lane::Connection if matches!(self.remote.connection, Connection::Loading(id) if id == request) =>
            {
                self.remote.connection = match result {
                    Ok(Outcome::Inventory(inventory)) => {
                        self.remote.inventory = Some(inventory);
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
