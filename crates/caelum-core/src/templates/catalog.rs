use super::{invocation, Template};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Checkout(String),
    Boat(String),
}

#[derive(Default)]
pub enum Catalog {
    #[default]
    Closed,
    Loading {
        target: Target,
        request: u64,
        invalidated: bool,
    },
    Ready {
        target: Target,
        items: Vec<Template>,
    },
    Failed {
        target: Target,
        error: String,
    },
}

impl Catalog {
    pub fn target(&self) -> Option<&Target> {
        match self {
            Self::Closed => None,
            Self::Loading { target, .. }
            | Self::Ready { target, .. }
            | Self::Failed { target, .. } => Some(target),
        }
    }

    pub fn items(&self, owner: &Target) -> &[Template] {
        match self {
            Self::Ready { target, items } if target == owner => items,
            _ => &[],
        }
    }

    pub fn expand(&self, owner: &Target, prompt: &str) -> Result<Option<String>, String> {
        let Some((name, args)) = invocation(prompt) else {
            return Ok(None);
        };
        match self {
            Self::Loading { target, .. } if target == owner => {
                Err("Prompt templates are loading. Try again in a moment.".into())
            }
            Self::Failed { target, error } if target == owner => {
                Err(format!("Could not load prompt templates: {error}"))
            }
            _ => self
                .items(owner)
                .iter()
                .find(|t| t.name == name)
                .map(|t| t.render(args))
                .transpose(),
        }
    }
}
