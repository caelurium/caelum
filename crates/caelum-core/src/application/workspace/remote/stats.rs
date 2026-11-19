use super::{Lane, Pending, Workspace};
use crate::{application::Command, remote::*};
use std::time::{Duration, Instant};

impl Workspace {
    /// The desktop supplies visibility; all request and deadline ownership stays in core.
    pub fn monitor_remote(&mut self, visible: bool, now: Instant) -> Option<Instant> {
        let owner = self.remote.saved.selected.as_ref().filter(|id| {
            visible
                && !self.remote.jobs.contains_key(*id)
                && self
                    .remote
                    .saved
                    .checkouts
                    .get(*id)
                    .is_some_and(|c| matches!(c.phase, Phase::Ready { .. }))
        });
        let Some(id) = owner else {
            self.remote.monitor = Monitor::Off;
            return None;
        };
        match &self.remote.monitor {
            Monitor::Loading { owner, .. } | Monitor::Failed { owner, .. } if owner == id => {
                return None
            }
            Monitor::Showing { owner, next, .. } if owner == id && *next > now => {
                return Some(*next)
            }
            _ => {}
        }
        let id = id.clone();
        let key = self.remote.saved.checkouts[&id]
            .machine()
            .unwrap()
            .key
            .clone();
        let previous = match std::mem::take(&mut self.remote.monitor) {
            Monitor::Showing { owner, sample, .. } if owner == id => Some(sample),
            _ => None,
        };
        let request = self.request(
            Command::Remote(Operation::Stats(key)),
            Pending::Remote(Lane::Stats(id.clone())),
        );
        self.remote.monitor = Monitor::Loading {
            owner: id,
            request,
            previous,
        };
        None
    }
    pub(super) fn complete_stats(
        &mut self,
        id: &str,
        request: u64,
        result: Result<Outcome, String>,
    ) {
        if !matches!(&self.remote.monitor,Monitor::Loading{owner,request:r,..} if owner==id && *r==request)
            || self.remote.saved.selected.as_deref() != Some(id)
        {
            return;
        }
        let Some(c) = self.remote.saved.checkouts.get_mut(id) else {
            return;
        };
        let sample = match result {
            Ok(Outcome::Stats(s)) if c.machine().is_some_and(|m| m.key == s.machine.key) => s,
            other => {
                self.remote.monitor = Monitor::Failed {
                    owner: id.into(),
                    error: other
                        .err()
                        .unwrap_or_else(|| "Unexpected machine sample".into()),
                };
                return;
            }
        };
        if !self.remote.jobs.contains_key(id) {
            if matches!(
                sample.machine.state.as_str(),
                "archived" | "archiving" | "stopped" | "error" | "cancelled"
            ) {
                c.phase = Phase::Prepare {
                    machine: sample.machine,
                    new_branch: None,
                };
                self.remote.monitor = Monitor::Off;
                self.refresh_remote(id);
                return;
            }
            if let Phase::Ready { machine, .. } = &mut c.phase {
                machine.clone_from(&sample.machine);
            }
        }
        self.remote.monitor = Monitor::Showing {
            owner: id.into(),
            sample,
            next: Instant::now() + Duration::from_secs(5),
        };
    }
}
