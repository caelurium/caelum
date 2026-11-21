use super::{
    installation::Prepared,
    worker::{Action, Cancellation},
    Installed, Job, Reply,
};
use caelum_protocol::{UpdateCheck, UpdateStatus};
use std::sync::Arc;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    Downloading { bytes: u64, total: Option<u64> },
    Verifying,
    Preparing,
}

#[derive(Debug, Default)]
pub enum Phase {
    #[default]
    Idle,
    Downloading {
        release: UpdateCheck,
        progress: Progress,
    },
    Cancelling,
    Ready(Arc<Prepared>),
    Installing(Arc<Prepared>),
    Installed(Arc<Installed>),
    Restarting(Arc<Installed>),
    Updated(String),
    Failed {
        message: String,
        retry: Retry,
    },
}

#[derive(Debug, Clone)]
pub enum Retry {
    Download(UpdateCheck),
    Install(Arc<Prepared>),
    Restart(Arc<Installed>),
}

/// One installation lifecycle, with attempt identity independent of UI navigation.
#[derive(Debug, Default)]
pub struct State {
    pub phase: Phase,
    generation: u64,
    cancel: Option<Arc<Cancellation>>,
}

impl State {
    pub fn busy(&self) -> bool {
        matches!(
            self.phase,
            Phase::Downloading { .. }
                | Phase::Cancelling
                | Phase::Installing(_)
                | Phase::Restarting(_)
        )
    }

    pub fn can_check(&self) -> bool {
        matches!(
            self.phase,
            Phase::Idle
                | Phase::Updated(_)
                | Phase::Failed {
                    retry: Retry::Download(_),
                    ..
                }
        )
    }

    pub fn startup(&mut self) -> Job {
        self.job(Action::Finish)
    }

    fn job(&mut self, action: Action) -> Job {
        self.generation += 1;
        let cancel = Arc::new(Cancellation::default());
        self.cancel = Some(cancel.clone());
        Job {
            id: self.generation,
            action,
            cancel,
        }
    }

    pub fn download(&mut self, release: &UpdateCheck) -> Option<Job> {
        if !self.can_check()
            || release.status != UpdateStatus::Available
            || release.download_url.is_none()
        {
            return None;
        }
        self.phase = Phase::Downloading {
            release: release.clone(),
            progress: Progress::Downloading {
                bytes: 0,
                total: None,
            },
        };
        Some(self.job(Action::Download(release.clone())))
    }

    pub fn install(&mut self) -> Option<Job> {
        let prepared = match &self.phase {
            Phase::Ready(prepared) => prepared.clone(),
            Phase::Failed {
                retry: Retry::Install(prepared),
                ..
            } => prepared.clone(),
            _ => return None,
        };
        self.phase = Phase::Installing(prepared.clone());
        Some(self.job(Action::Install(prepared)))
    }

    pub fn retry(&mut self) -> Option<Job> {
        match &self.phase {
            Phase::Failed {
                retry: Retry::Download(release),
                ..
            } => self.download(&release.clone()),
            Phase::Failed {
                retry: Retry::Install(_),
                ..
            } => self.install(),
            Phase::Failed {
                retry: Retry::Restart(installed),
                ..
            } => {
                self.phase = Phase::Restarting(installed.clone());
                None
            }
            _ => None,
        }
    }

    pub fn cancel(&mut self) -> Option<Job> {
        if matches!(
            self.phase,
            Phase::Failed {
                retry: Retry::Download(_),
                ..
            }
        ) {
            self.phase = Phase::Idle;
            return None;
        }
        if matches!(self.phase, Phase::Downloading { .. }) {
            if let Some(cancel) = self.cancel.take() {
                cancel.cancel();
            }
            self.phase = Phase::Cancelling;
            return None;
        }
        let prepared = match &self.phase {
            Phase::Ready(prepared)
            | Phase::Failed {
                retry: Retry::Install(prepared),
                ..
            } => prepared.clone(),
            _ => return None,
        };
        self.phase = Phase::Cancelling;
        Some(self.job(Action::Discard(prepared)))
    }

    pub fn restart(&mut self) {
        if let Phase::Installed(installed) = &self.phase {
            self.phase = Phase::Restarting(installed.clone());
        }
    }

    pub fn restart_failed(&mut self, message: String) {
        if let Phase::Restarting(installed) = &self.phase {
            self.phase = Phase::Failed {
                message,
                retry: Retry::Restart(installed.clone()),
            };
        }
    }

    pub fn progress(&mut self, id: u64, progress: Progress) -> bool {
        if id != self.generation {
            return false;
        }
        if let Phase::Downloading {
            progress: current, ..
        } = &mut self.phase
        {
            if *current != progress {
                *current = progress;
                return true;
            }
        }
        false
    }

    pub fn complete(&mut self, job: &Job, result: Result<Reply, String>) -> Option<Job> {
        if job.id != self.generation {
            return match result {
                Ok(Reply::Prepared(prepared)) => Some(Job {
                    id: job.id,
                    action: Action::Discard(prepared),
                    cancel: Arc::default(),
                }),
                _ => None,
            };
        }
        self.cancel = None;
        if matches!(self.phase, Phase::Cancelling) {
            if let Ok(Reply::Prepared(prepared)) = result {
                return Some(self.job(Action::Discard(prepared)));
            }
            self.phase = Phase::Idle;
            return None;
        }
        match result {
            Ok(Reply::Prepared(prepared)) => self.phase = Phase::Ready(prepared),
            Ok(Reply::Installed(installed)) => self.phase = Phase::Installed(installed),
            Ok(Reply::Finished(Some(version))) => self.phase = Phase::Updated(version),
            Ok(Reply::Finished(None) | Reply::Discarded) => {}
            Err(message) => {
                let retry = match &job.action {
                    Action::Download(release) => Retry::Download(release.clone()),
                    Action::Install(prepared) => Retry::Install(prepared.clone()),
                    Action::Finish | Action::Discard(_) => return None,
                };
                self.phase = Phase::Failed { message, retry };
            }
        }
        None
    }
}
