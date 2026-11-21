use super::Workspace;
use crate::{
    application::{Options, Provider, Scope},
    remote::{native::checkout_id, ProviderDefaults},
};

impl Workspace {
    fn vm_defaults_mut(&mut self, id: &str) -> Option<&mut ProviderDefaults> {
        let checkout = self.remote.saved.checkouts.get_mut(id)?;
        Some(checkout.defaults.get_or_insert_with(|| ProviderDefaults {
            provider: checkout.thread.provider,
            ..Default::default()
        }))
    }

    pub fn default_provider_for(&self, project: Option<&str>) -> Provider {
        let Some(id) = project.and_then(checkout_id) else {
            return self.preferences.default_provider;
        };
        self.remote
            .saved
            .checkouts
            .get(id)
            .map_or(Provider::OpenCode, |c| {
                c.defaults
                    .as_ref()
                    .map_or(c.thread.provider, |d| d.provider)
            })
    }

    pub fn set_default_provider_for(&mut self, project: Option<&str>, provider: Provider) {
        if let Some(id) = project.and_then(checkout_id) {
            if let Some(d) = self.vm_defaults_mut(id) {
                d.provider = provider;
            }
        } else {
            self.preferences.default_provider = provider;
        }
    }

    pub fn provider_options(&self, scope: &Scope) -> Option<&Options> {
        let options = match checkout_id(&scope.project) {
            Some(id) => {
                &self
                    .remote
                    .saved
                    .checkouts
                    .get(id)?
                    .defaults
                    .as_ref()?
                    .options
            }
            None => &self.preferences.providers,
        };
        options.get(scope.provider.as_str())
    }

    fn options_at(
        &mut self,
        scope: &Scope,
    ) -> Option<&mut std::collections::BTreeMap<String, Options>> {
        Some(match checkout_id(&scope.project) {
            Some(id) => &mut self.vm_defaults_mut(id)?.options,
            None => &mut self.preferences.providers,
        })
    }

    pub fn edit_provider_options(&mut self, scope: &Scope, edit: impl FnOnce(&mut Options)) {
        if let Some(options) = self.options_at(scope) {
            edit(options.entry(scope.provider.as_str().into()).or_default());
        }
    }

    pub fn reset_provider_options(&mut self, scope: &Scope) {
        if let Some(options) = self.options_at(scope) {
            options.remove(scope.provider.as_str());
        }
    }

    pub fn favorite_models(&self, scope: &Scope) -> Option<&[String]> {
        let favorites = match checkout_id(&scope.project) {
            Some(id) => {
                &self
                    .remote
                    .saved
                    .checkouts
                    .get(id)?
                    .defaults
                    .as_ref()?
                    .favorites
            }
            None => &self.preferences.favorite_models,
        };
        favorites.get(scope.provider.as_str()).map(Vec::as_slice)
    }

    pub fn toggle_favorite_model(&mut self, scope: &Scope, model: &str) {
        let favorites = match checkout_id(&scope.project) {
            Some(id) => {
                let Some(d) = self.vm_defaults_mut(id) else {
                    return;
                };
                &mut d.favorites
            }
            None => &mut self.preferences.favorite_models,
        };
        let list = favorites.entry(scope.provider.as_str().into()).or_default();
        if list.iter().any(|s| s == model) {
            list.retain(|s| s != model);
        } else {
            list.push(model.into());
        }
    }
}
