use super::{Pending, Workspace};
use crate::application::{Command, Reply, Task, TaskReply};
use crate::remote::{Operation, Outcome};
use crate::templates::{Catalog, Target};

impl Workspace {
    pub fn show_templates(&mut self, owner: Option<Target>) {
        if self.templates.target() == owner.as_ref() {
            return;
        }
        self.templates = Catalog::Closed;
        if let Some(owner) = owner {
            self.load_templates(owner);
        }
    }

    fn load_templates(&mut self, target: Target) {
        let command = match &target {
            Target::Checkout(project) => Command::Task(Task::Templates {
                project: project.clone(),
            }),
            Target::Boat(id) => {
                let Some(checkout) = self.remote.saved.checkouts.get(id).and_then(|c| c.target())
                else {
                    return;
                };
                Command::Remote(Operation::Templates(checkout))
            }
        };
        let request = self.request(command, Pending::Templates);
        self.templates = Catalog::Loading {
            target,
            request,
            invalidated: false,
        };
    }

    pub(super) fn templates_changed(&mut self, owner: Target) {
        if self.templates.target() != Some(&owner) {
            return;
        }
        if let Catalog::Loading { invalidated, .. } = &mut self.templates {
            *invalidated = true;
        } else {
            self.load_templates(owner)
        }
    }

    pub(super) fn complete_templates(&mut self, id: u64, result: Result<Reply, String>) {
        let Catalog::Loading {
            target,
            request,
            invalidated,
        } = &self.templates
        else {
            return;
        };
        if *request != id {
            return;
        }
        let target = target.clone();
        if *invalidated {
            self.load_templates(target);
            return;
        }
        self.templates = match result {
            Ok(Reply::Task(TaskReply::Templates(items)))
            | Ok(Reply::Remote(Outcome::Templates(items))) => Catalog::Ready { target, items },
            other => Catalog::Failed {
                target,
                error: other
                    .err()
                    .unwrap_or_else(|| "Unexpected template response".into()),
            },
        };
    }
}
