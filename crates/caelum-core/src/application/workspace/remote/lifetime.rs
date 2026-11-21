use super::{Lane, Pending, Workspace};
use crate::{
    application::Command,
    remote::{Job, Lifetime, Monitor, Operation, Outcome, Phase},
};

impl Workspace {
    pub fn can_set_vm_lifetime(&self, id: &str) -> bool {
        self.remote
            .saved
            .checkouts
            .get(id)
            .is_some_and(|c| matches!(c.phase, Phase::Ready { .. }))
            && !self.remote.jobs.contains_key(id)
            && !matches!(self.remote.lifetimes.get(id), Some(Job::Busy(_)))
    }

    pub fn set_vm_lifetime(&mut self, id: &str, lifetime: Lifetime) {
        if !self.can_set_vm_lifetime(id) {
            return;
        }
        let machine = self.remote.saved.checkouts[id]
            .machine()
            .unwrap()
            .key
            .clone();
        if self.remote.monitor.owns(id) {
            self.remote.monitor = Monitor::Off;
        }
        let request = self.request(
            Command::Remote(Operation::Lifetime(machine, lifetime)),
            Pending::Remote(Lane::Lifetime(id.into(), lifetime)),
        );
        self.remote.lifetimes.insert(id.into(), Job::Busy(request));
    }

    pub(super) fn complete_lifetime(
        &mut self,
        id: &str,
        request: u64,
        lifetime: Lifetime,
        result: Result<Outcome, String>,
    ) {
        if !matches!(self.remote.lifetimes.get(id), Some(Job::Busy(r)) if *r == request) {
            return;
        }
        self.remote.lifetimes.remove(id);
        let Some(c) = self.remote.saved.checkouts.get_mut(id) else {
            return;
        };
        match result {
            Ok(Outcome::Lifetime(m)) if c.machine().is_some_and(|v| v.key == m.key) => {
                if let Phase::Ready { machine, .. } = &mut c.phase {
                    machine.auto_stop_unix = m.auto_stop_unix;
                }
                c.lifetime = lifetime;
            }
            other => {
                self.remote.lifetimes.insert(
                    id.into(),
                    Job::Failed(
                        other
                            .err()
                            .unwrap_or_else(|| "Unexpected auto-stop response".into()),
                    ),
                );
            }
        }
    }
}
