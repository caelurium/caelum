use super::{Lane, Pending, Workspace};
use crate::{application::Command, remote::*};

impl Workspace {
    pub fn review_remote(&mut self, id: &str) {
        self.git_request(id, GitAction::Read);
    }

    pub fn commit_remote(&mut self, id: &str, message: &str) {
        if message.trim().is_empty() {
            return;
        }
        if let Some(Changes::Ready(review)) = self.remote.changes.get(id) {
            if !review.patch.is_empty() {
                self.git_request(
                    id,
                    GitAction::Commit(review.revision.clone(), message.into()),
                );
            }
        }
    }

    pub fn push_remote(&mut self, id: &str) {
        if let Some(Changes::Ready(review)) = self.remote.changes.get(id) {
            self.git_request(id, GitAction::Push(review.revision.clone()));
        }
    }

    pub(super) fn publishing(&self, id: &str) -> bool {
        matches!(
            self.remote.changes.get(id),
            Some(Changes::Loading { mutation: true, .. })
        )
    }

    fn git_request(&mut self, id: &str, action: GitAction) {
        let Some(c) = self.remote.saved.checkouts.get(id) else {
            return;
        };
        let mutation = !matches!(action, GitAction::Read);
        if !matches!(c.phase, Phase::Ready { .. })
            || self.remote.jobs.contains_key(id)
            || self.publishing(id)
            || (mutation && c.thread.turn != Turn::Idle)
        {
            return;
        }
        let target = c.target().unwrap();
        // Keep only the selected review's patch; in-flight operations retain their owner.
        self.remote
            .changes
            .retain(|key, state| key == id || matches!(state, Changes::Loading { .. }));
        let request = self.request(
            Command::Remote(Operation::Git(target, action)),
            Pending::Remote(Lane::Git(id.into())),
        );
        self.remote
            .changes
            .insert(id.into(), Changes::Loading { request, mutation });
    }

    pub(super) fn complete_git(&mut self, id: &str, request: u64, result: Result<Outcome, String>) {
        if !matches!(self.remote.changes.get(id),Some(Changes::Loading{request:r,..}) if *r==request)
        {
            return;
        }
        if self.remote.saved.selected.as_deref() != Some(id) {
            self.remote.changes.remove(id);
            return;
        }
        let state = match result {
            Ok(Outcome::Review(review)) => {
                if let Some(Checkout {
                    phase: Phase::Ready { branch, .. },
                    ..
                }) = self.remote.saved.checkouts.get_mut(id)
                {
                    if branch != &review.revision.branch {
                        branch.clone_from(&review.revision.branch);
                        self.templates_changed(crate::templates::Target::Boat(id.into()));
                    }
                }
                Changes::Ready(review)
            }
            other => Changes::Failed(
                other
                    .err()
                    .unwrap_or_else(|| "Unexpected remote Git response".into()),
            ),
        };
        self.remote.changes.insert(id.into(), state);
    }
}
