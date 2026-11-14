use super::{Filter, Owner, brand::Brand};
use crate::{Workspace, live::live, ui};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Group {
    Default,
    Favorites,
    Company(String),
    Unavailable,
}

impl Group {
    pub(super) fn label(&self) -> &str {
        match self {
            Self::Default => "",
            Self::Favorites => "Favourites",
            Self::Company(name) => name,
            Self::Unavailable => "Unavailable",
        }
    }
}

#[derive(Clone)]
pub(super) struct Model {
    pub id: String,
    pub name: String,
    pub connection_id: String,
    pub connection: String,
    pub brand: Option<Brand>,
    pub company: String,
    pub context: String,
    pub available: bool,
    pub favorite: bool,
    pub group: Group,
}

pub(super) fn all(s: &Workspace) -> Vec<Model> {
    let l = live(s);
    let Some(owner) = l.models.owner.as_ref() else {
        return Vec::new();
    };
    let scope = owner.scope();
    let favorites = l
        .app
        .preferences
        .favorite_models
        .get(scope.provider.as_str());
    let mut models: Vec<_> = l
        .app
        .choices
        .get(scope)
        .into_iter()
        .flat_map(|c| &c.models)
        .map(|m| {
            let connection_id = m
                .provider
                .as_ref()
                .map(|p| p.id.as_str())
                .unwrap_or(scope.provider.as_str())
                .to_owned();
            let connection = m
                .provider
                .as_ref()
                .map(|p| p.name.as_str())
                .unwrap_or(ui::provider_name(scope.provider))
                .to_owned();
            let brand = Brand::model(&m.id, &m.name, &connection_id);
            let company = brand.map_or_else(|| connection.clone(), |brand| brand.name().to_owned());
            let favorite = favorites.is_some_and(|f| f.contains(&m.id));
            Model {
                id: m.id.clone(),
                name: m.name.clone(),
                connection_id,
                connection,
                brand,
                group: if favorite {
                    Group::Favorites
                } else {
                    Group::Company(company.clone())
                },
                company,
                favorite,
                available: true,
                context: m.context_window.map(context).unwrap_or_default(),
            }
        })
        .collect();
    let home_options = l.app.home_options();
    let current = match owner {
        Owner::Home(_) => home_options.model.as_ref(),
        Owner::Session(index, _) => l.app.sessions[*index].options.model.as_ref(),
        Owner::Defaults(scope) => l
            .app
            .preferences
            .providers
            .get(scope.provider.as_str())
            .and_then(|o| o.model.as_ref()),
    };
    for id in favorites.into_iter().flatten().chain(current) {
        if !models.iter().any(|m| &m.id == id) {
            models.push(Model {
                id: id.clone(),
                name: id.clone(),
                connection_id: String::new(),
                connection: if Some(id) == current {
                    "Unavailable · current model"
                } else {
                    "Unavailable · saved favourite"
                }
                .into(),
                brand: None,
                company: String::new(),
                context: String::new(),
                available: false,
                favorite: favorites.is_some_and(|f| f.contains(id)),
                group: Group::Unavailable,
            });
        }
    }
    models.sort_by_cached_key(|m| {
        (
            if !m.available {
                2
            } else {
                usize::from(!m.favorite)
            },
            m.group.label().to_lowercase(),
            m.name.to_lowercase(),
            m.connection.to_lowercase(),
            m.id.clone(),
        )
    });
    models.insert(
        0,
        Model {
            id: String::new(),
            name: "Provider default".into(),
            connection: "Use the provider’s recommended model".into(),
            connection_id: String::new(),
            brand: None,
            company: String::new(),
            context: String::new(),
            available: true,
            favorite: false,
            group: Group::Default,
        },
    );
    models
}

pub(super) fn matches(s: &Workspace) -> Vec<Model> {
    let l = live(s);
    let query = l.models.query.text.to_lowercase();
    all(s)
        .into_iter()
        .filter(|m| {
            let filter = match &l.models.filter {
                Filter::All => true,
                Filter::Favorites => m.favorite,
                Filter::Connection(id) => &m.connection_id == id,
            };
            let text = format!(
                "{} {} {} {} {} {}",
                m.name, m.id, m.connection, m.connection_id, m.company, m.context
            )
            .to_lowercase();
            filter && query.split_whitespace().all(|word| text.contains(word))
        })
        .collect()
}

fn context(size: u64) -> String {
    if size >= 1_000_000 {
        let millions = size as f64 / 1_000_000.0;
        format!(
            "{}M context",
            format!("{millions:.1}").trim_end_matches(".0")
        )
    } else if size >= 1000 {
        format!("{}K context", size / 1000)
    } else {
        format!("{size} context")
    }
}
