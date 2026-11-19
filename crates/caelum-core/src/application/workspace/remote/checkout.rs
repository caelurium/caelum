use super::{Lane, Pending, Workspace};
use crate::{application::Command, remote::*};
use std::time::{Duration, Instant};

impl Workspace {
    pub fn remote_picker(&mut self, project: String) {
        let request = self.request(
            Command::Remote(Operation::Discover(project.clone())),
            Pending::Remote(Lane::Picker),
        );
        self.remote.picker = Some(Picker::Loading { request, project });
        self.list_machines();
    }

    pub(super) fn complete_remote_picker(&mut self, request: u64, result: Result<Outcome, String>) {
        let Some(Picker::Loading {
            request: active,
            project,
        }) = &self.remote.picker
        else {
            return;
        };
        if request != *active {
            return;
        }
        self.remote.picker = Some(match result {
            Ok(Outcome::Source(s)) if &s.project == project => Picker::Ready(s),
            other => Picker::Failed {
                project: project.clone(),
                error: other
                    .err()
                    .unwrap_or_else(|| "Unexpected project source".into()),
            },
        });
    }

    pub fn create_remote(
        &mut self,
        base: &str,
        branch: &str,
        size: Size,
        minutes: u32,
    ) -> Option<String> {
        let Some(Picker::Ready(mut source)) = self.remote.picker.clone() else {
            return None;
        };
        if !valid_branch(base)
            || !valid_branch(branch)
            || base == branch
            || !matches!(minutes, 30 | 60 | 120)
        {
            return None;
        }
        source.base = base.into();
        let id = uuid::Uuid::new_v4().to_string();
        let plan = Plan {
            token: id.clone(),
            branch: branch.into(),
            size,
            minutes,
        };
        self.remote.saved.checkouts.insert(
            id.clone(),
            Checkout {
                source,
                phase: Phase::Create(plan),
                thread: Thread::default(),
                pinned_stats: false,
            },
        );
        self.remote.picker = None;
        self.select_remote(&id);
        Some(id)
    }

    pub fn attach_remote(&mut self, machine: Machine) -> Option<String> {
        let Some(Picker::Ready(source)) = self.remote.picker.clone() else {
            return None;
        };
        if machine.key.account != source.account {
            return None;
        }
        let existing = self
            .remote
            .saved
            .checkouts
            .iter()
            .find(|(_, c)| c.machine().is_some_and(|m| m.key == machine.key));
        let id = if let Some((id, c)) = existing {
            if !c.source.repository.eq_ignore_ascii_case(&source.repository) {
                return None;
            }
            id.clone()
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            self.remote.saved.checkouts.insert(
                id.clone(),
                Checkout {
                    source,
                    thread: Thread::default(),
                    pinned_stats: false,
                    phase: Phase::Prepare {
                        machine,
                        new_branch: None,
                    },
                },
            );
            id
        };
        self.remote.picker = None;
        self.select_remote(&id);
        Some(id)
    }

    pub fn select_remote(&mut self, id: &str) {
        if !self.remote.saved.checkouts.contains_key(id) {
            return;
        }
        self.leave_conversation();
        self.active_project = None;
        self.remote.saved.selected = Some(id.to_owned());
        self.refresh_remote(id);
    }

    pub fn refresh_remote(&mut self, id: &str) {
        let Some(c) = self.remote.saved.checkouts.get(id) else {
            return;
        };
        let op = match &c.phase {
            Phase::Create(plan) => Operation::Create(c.source.clone(), plan.clone()),
            Phase::Prepare { new_branch, .. } => Operation::Prepare(
                c.target().unwrap(),
                c.source.base.clone(),
                new_branch.clone(),
            ),
            Phase::Ready { .. } => {
                Operation::Prepare(c.target().unwrap(), c.source.base.clone(), None)
            }
        };
        self.checkout_request(id, op);
    }

    pub fn power_remote(&mut self, id: &str, resume: bool) {
        if self.publishing(id) {
            return;
        }
        if let Some(machine) = self
            .remote
            .saved
            .checkouts
            .get(id)
            .and_then(Checkout::machine)
        {
            self.checkout_request(id, Operation::Power(machine.key.clone(), resume));
        }
    }

    fn checkout_request(&mut self, id: &str, op: Operation) {
        if matches!(self.remote.jobs.get(id), Some(Job::Busy(_))) {
            return;
        }
        if self.remote.saved.selected.as_deref() == Some(id) {
            self.remote.monitor = Monitor::Off;
        }
        let request = self.request(
            Command::Remote(op),
            Pending::Remote(Lane::Checkout(id.into())),
        );
        self.remote.jobs.insert(id.into(), Job::Busy(request));
    }

    pub(super) fn complete_checkout(
        &mut self,
        id: &str,
        request: u64,
        result: Result<Outcome, String>,
    ) {
        if !matches!(self.remote.jobs.get(id), Some(Job::Busy(r)) if *r == request) {
            return;
        }
        self.remote.jobs.remove(id);
        let Some(c) = self.remote.saved.checkouts.get_mut(id) else {
            return;
        };
        let valid = |m: &Machine| {
            m.key.account == c.source.account && c.machine().is_none_or(|old| old.key == m.key)
        };
        let new_branch = match &c.phase {
            Phase::Create(p) => Some(p.branch.clone()),
            Phase::Prepare { new_branch, .. } => new_branch.clone(),
            Phase::Ready { .. } => None,
        };
        match result {
            Ok(Outcome::Prepared(machine, branch)) if valid(&machine) => {
                c.phase = Phase::Ready { machine, branch };
                self.follow_remote(id);
            }
            Ok(Outcome::Machine(machine)) if valid(&machine) => {
                let terminal = matches!(
                    machine.state.as_str(),
                    "archived" | "stopped" | "error" | "failed" | "cancelled"
                );
                c.phase = Phase::Prepare {
                    machine,
                    new_branch,
                };
                if !terminal {
                    self.refresh_remote(id);
                }
            }
            Ok(Outcome::Waiting(machine)) if valid(&machine) => {
                c.phase = Phase::Prepare {
                    machine,
                    new_branch,
                };
                self.remote.jobs.insert(
                    id.into(),
                    Job::Wait(Instant::now() + Duration::from_secs(2)),
                );
            }
            other => {
                self.remote.jobs.insert(
                    id.into(),
                    Job::Failed(
                        other
                            .err()
                            .unwrap_or_else(|| "Unexpected machine response".into()),
                    ),
                );
            }
        }
    }

    /// Only an in-progress provisioning/stop requests another wake.
    pub fn remote_deadline(&mut self, now: Instant) -> Option<Instant> {
        let due: Vec<_> = self
            .remote
            .jobs
            .iter()
            .filter(|(_, job)| matches!(job, Job::Wait(at) if *at <= now))
            .map(|(id, _)| id.clone())
            .collect();
        for id in due {
            self.refresh_remote(&id);
        }
        self.remote
            .jobs
            .values()
            .filter_map(|job| {
                if let Job::Wait(at) = job {
                    Some(*at)
                } else {
                    None
                }
            })
            .min()
    }
}
