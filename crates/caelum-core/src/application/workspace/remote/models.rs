use super::{Lane, Pending, Workspace};
use crate::{
    application::Command,
    remote::{Checkout, ModelCatalogue, Operation, Outcome, Thread, VmProvider},
};

fn reconcile_efforts(checkout: &mut Checkout, catalogue: &ModelCatalogue) {
    for thread in std::iter::once(&mut checkout.thread).chain(&mut checkout.other_threads) {
        if !VmProvider::CHOICES.contains(&thread.provider) {
            continue;
        }
        let Some(level) = &thread.effort else {
            continue;
        };
        if thread
            .model
            .as_deref()
            .is_none_or(|id| !catalogue.supports_effort(thread.provider, id, level))
        {
            thread.effort = None;
        }
    }
}

impl Workspace {
    pub fn refresh_vm_models(&mut self) {
        if !matches!(self.remote.models, ModelCatalogue::Loading(_)) {
            let req = self.request(
                Command::Remote(Operation::Models),
                Pending::Remote(Lane::Models),
            );
            self.remote.models = ModelCatalogue::Loading(req);
        }
    }

    pub(super) fn complete_vm_models(&mut self, req: u64, result: Result<Outcome, String>) {
        if matches!(self.remote.models, ModelCatalogue::Loading(r) if r == req) {
            self.remote.models = match result {
                Ok(Outcome::Models(models)) => {
                    let catalogue = ModelCatalogue::Ready(models);
                    for c in self.remote.saved.checkouts.values_mut() {
                        reconcile_efforts(c, &catalogue);
                    }
                    catalogue
                }
                other => ModelCatalogue::Failed(
                    other
                        .err()
                        .unwrap_or_else(|| "Unexpected Boat model catalogue".into()),
                ),
            };
        }
    }

    pub fn set_vm_model(
        &mut self,
        id: &str,
        owner: &str,
        provider: VmProvider,
        model: Option<&str>,
    ) -> bool {
        if model.is_some_and(|m| self.remote.models.find(provider, m).is_none()) {
            return false;
        }
        let Some(thread) = self.vm_options_owner(id, owner, provider) else {
            return false;
        };
        if thread.model.as_deref() == model {
            return false;
        }
        thread.model = model.map(str::to_owned);
        thread.effort = None;
        self.remote.feeds.remove(id);
        true
    }

    pub fn set_vm_effort(
        &mut self,
        id: &str,
        owner: &str,
        provider: VmProvider,
        model: &str,
        effort: Option<&str>,
    ) -> bool {
        if effort.is_some_and(|e| !self.remote.models.supports_effort(provider, model, e)) {
            return false;
        }
        let Some(thread) = self.vm_options_owner(id, owner, provider) else {
            return false;
        };
        if thread.model.as_deref() != Some(model) || thread.effort.as_deref() == effort {
            return false;
        }
        thread.effort = effort.map(str::to_owned);
        self.remote.feeds.remove(id);
        true
    }

    fn vm_options_owner(
        &mut self,
        id: &str,
        owner: &str,
        provider: VmProvider,
    ) -> Option<&mut Thread> {
        if self.remote.saved.selected.as_deref() != Some(id) || !self.can_switch_vm_thread(id) {
            return None;
        }
        let thread = &mut self.remote.saved.checkouts.get_mut(id)?.thread;
        (thread.editor_key(id) == owner && thread.provider == provider).then_some(thread)
    }
}
