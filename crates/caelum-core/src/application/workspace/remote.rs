use super::{Pending, Workspace};
use crate::{
    application::{Command, Reply},
    remote::{Connection, Credential, Operation, Outcome},
};

pub(super) enum Lane {
    Connection,
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
                };
            }
            _ => {}
        }
    }
}
