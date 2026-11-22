use super::{Review, Services, Task};

#[cfg(test)]
mod tests;

pub(super) enum Slot {
    Running(Request),
    Failed(String),
}

pub(super) struct Request {
    id: u64,
    owner: Owner,
}

enum Owner {
    Independent,
    Mutation,
    Review {
        key: (String, String),
        generation: u64,
        pin: Pin,
    },
}

enum Pin {
    Details,
    Head(String),
    Diff { head: String, base: String },
}

impl Pin {
    fn matches(&self, review: &Review) -> bool {
        match self {
            Self::Details => true,
            Self::Head(head) => review.head() == Some(head),
            Self::Diff { head, base } => {
                review.head() == Some(head) && review.expected_base() == Some(base)
            }
        }
    }
}

impl Services {
    pub(in crate::application) fn current(&self, id: u64, task: &Task) -> bool {
        let Some(Slot::Running(request)) = self.requests.get(&task.slot()) else {
            return false;
        };
        if request.id != id {
            return false;
        }
        match &request.owner {
            Owner::Independent | Owner::Mutation => true,
            Owner::Review {
                key,
                generation,
                pin,
            } => self
                .reviews
                .get(key)
                .is_some_and(|review| review.generation == *generation && pin.matches(review)),
        }
    }

    pub fn busy(&self, task: &Task) -> bool {
        matches!(self.requests.get(&task.slot()), Some(Slot::Running(_)))
    }

    pub fn error(&self, task: &Task) -> Option<&str> {
        match self.requests.get(&task.slot()) {
            Some(Slot::Failed(error)) => Some(error),
            _ => None,
        }
    }

    pub(super) fn start_slot(&mut self, task: &Task, id: u64) {
        let owner = if let Some((url, head)) = task.review() {
            let key = (task.project().into(), url.into());
            let review = self.reviews.entry(key.clone()).or_default();
            review.error = None;
            if task.mutates_review() {
                Owner::Mutation
            } else {
                let pin = match task {
                    Task::ReviewFiles { head, base, .. } => Pin::Diff {
                        head: head.clone(),
                        base: base.clone(),
                    },
                    _ => head.map_or(Pin::Details, |head| Pin::Head(head.into())),
                };
                Owner::Review {
                    key,
                    generation: review.generation,
                    pin,
                }
            }
        } else {
            Owner::Independent
        };
        self.requests
            .insert(task.slot(), Slot::Running(Request { id, owner }));
    }

    pub(super) fn finish_slot(&mut self, task: &Task, id: u64) -> bool {
        let current = self.current(id, task);
        let slot = task.slot();
        if matches!(self.requests.get(&slot), Some(Slot::Running(request)) if request.id == id) {
            self.requests.remove(&slot);
        }
        current
    }

    pub(super) fn invalidate_review_reads(&mut self, key: &(String, String)) {
        self.requests.retain(|_, phase| !matches!(phase,
            Slot::Running(Request { owner: Owner::Review { key: owner, .. }, .. }) if owner == key
        ));
    }
}
