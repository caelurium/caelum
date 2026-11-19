use super::{Lane, Pending, Workspace};
use crate::{
    application::{Command, Provider},
    remote::{ModelCatalogue, Operation, Outcome},
};

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
                Ok(Outcome::Models(models)) => ModelCatalogue::Ready(models),
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
        provider: Provider,
        model: Option<&str>,
    ) -> bool {
        if self.remote.saved.selected.as_deref() != Some(id)
            || !self.can_switch_vm_thread(id)
            || model.is_some_and(|m| self.remote.models.find(provider, m).is_none())
        {
            return false;
        }
        let thread = &mut self.remote.saved.checkouts.get_mut(id).unwrap().thread;
        if thread.editor_key(id) != owner
            || thread.provider != provider
            || thread.model.as_deref() == model
        {
            return false;
        }
        thread.model = model.map(str::to_owned);
        self.remote.feeds.remove(id);
        true
    }
}
