use crate::application::Provider;
use boat_sdk::agents::ProviderCatalogue;
use caelum_protocol::ProviderChoice;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub type ModelLists = HashMap<Provider, Arc<[ProviderChoice]>>;

const MAX_MODELS_PER_PROVIDER: usize = 512;
const MAX_FIELD_BYTES: usize = 512;

fn usable(value: &str) -> bool {
    value.len() <= MAX_FIELD_BYTES
        && !value.trim().is_empty()
        && !value.chars().any(char::is_control)
}

/// Public Boat choices shared by VM editors for this app lifetime. Only the
/// selected ID is persisted, on its thread.
#[derive(Debug, Default)]
pub enum ModelCatalogue {
    #[default]
    Unloaded,
    Loading(u64),
    Ready(ModelLists),
    Failed(String),
}

impl ModelCatalogue {
    pub fn find(&self, provider: Provider, id: &str) -> Option<&ProviderChoice> {
        let Self::Ready(lists) = self else {
            return None;
        };
        lists.get(&provider)?.iter().find(|m| m.id == id)
    }
}

pub(super) fn model_lists(mut catalogue: ProviderCatalogue) -> ModelLists {
    Provider::ALL
        .into_iter()
        .filter_map(|provider| {
            let harness = catalogue.remove(provider.as_str())?;
            let mut seen = HashSet::new();
            let models = harness
                .models
                .into_iter()
                .filter(|m| usable(&m.id) && seen.insert(m.id.clone()))
                .take(MAX_MODELS_PER_PROVIDER)
                .map(|m| ProviderChoice {
                    name: if usable(&m.label) {
                        m.label
                    } else {
                        m.id.clone()
                    },
                    // `/prompt` accepts Boat's ID, which can differ from the native sdkId.
                    id: m.id,
                })
                .collect();
            Some((provider, models))
        })
        .collect()
}

#[cfg(test)]
mod tests;
