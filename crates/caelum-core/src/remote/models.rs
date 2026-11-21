use crate::application::Provider;
use boat_sdk::agents::{ProviderCatalogue, ProviderModel};
use caelum_protocol::{ProviderChoice, ProviderModelChoice};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub type ModelLists = HashMap<Provider, Arc<[ProviderModelChoice]>>;

const MAX_MODELS_PER_PROVIDER: usize = 512;
const MAX_FIELD_BYTES: usize = 512;
const MAX_EFFORTS: usize = 16;
const MAX_EFFORT_BYTES: usize = 64;

fn usable(value: &str) -> bool {
    value.len() <= MAX_FIELD_BYTES
        && !value.trim().is_empty()
        && !value.chars().any(char::is_control)
}

/// Public Boat choices shared by VM editors for this app lifetime. Only the
/// selected model and effort IDs are persisted, on their thread.
#[derive(Debug, Default)]
pub enum ModelCatalogue {
    #[default]
    Unloaded,
    Loading(u64),
    Ready(ModelLists),
    Failed(String),
}

impl ModelCatalogue {
    pub fn find(&self, provider: Provider, id: &str) -> Option<&ProviderModelChoice> {
        let Self::Ready(lists) = self else {
            return None;
        };
        lists.get(&provider)?.iter().find(|m| m.id == id)
    }

    pub(crate) fn supports_effort(&self, provider: Provider, model: &str, level: &str) -> bool {
        self.find(provider, model)
            .is_some_and(|m| m.efforts.iter().any(|e| e.id == level))
    }
}

fn model_choice(m: ProviderModel) -> ProviderModelChoice {
    let mut efforts = Vec::<ProviderChoice>::new();
    let mut default_effort = None;
    if let Some(spec) = m.reasoning_effort {
        for id in spec.supported {
            if id.len() <= MAX_EFFORT_BYTES && usable(&id) && !efforts.iter().any(|e| e.id == id) {
                efforts.push(ProviderChoice {
                    name: id.clone(),
                    id,
                });
                if efforts.len() == MAX_EFFORTS {
                    break;
                }
            }
        }
        default_effort = spec
            .default
            .filter(|id| efforts.iter().any(|e| e.id == *id));
    }
    ProviderModelChoice {
        name: if usable(&m.label) {
            m.label
        } else {
            m.id.clone()
        },
        // `/prompt` accepts Boat's ID, which can differ from the native sdkId.
        id: m.id,
        provider: None,
        context_window: None,
        efforts,
        default_effort,
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
                .map(model_choice)
                .collect();
            Some((provider, models))
        })
        .collect()
}

#[cfg(test)]
mod tests;
