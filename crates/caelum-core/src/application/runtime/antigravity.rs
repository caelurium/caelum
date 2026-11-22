use super::{Options, ProviderRuntime, Reply, SessionKey, Summary};
use crate::application::Scope;

impl ProviderRuntime {
    pub(super) fn list_antigravity(&self, scope: &Scope) -> Result<Vec<Summary>, String> {
        self.antigravity
            .list_sessions(&scope.project)
            .map_err(|e| e.to_string())
            .map(|list| antigravity_summaries(scope, list.sessions))
    }

    pub(super) fn list_archived_antigravity(&self, scope: &Scope) -> Result<Vec<Summary>, String> {
        self.antigravity
            .list_archived_sessions(&scope.project)
            .map_err(|e| e.to_string())
            .map(|list| antigravity_summaries(scope, list.sessions))
    }

    pub(super) fn start_antigravity(
        &self,
        scope: &Scope,
        options: &Options,
    ) -> Result<Summary, String> {
        self.antigravity
            .start_session_with_options(
                &scope.project,
                options.model.as_deref(),
                options.effort.as_deref(),
                options.agent.as_deref(),
            )
            .map(|s| Summary {
                key: SessionKey {
                    scope: scope.clone(),
                    id: s.session.id,
                },
                title: s.session.title,
                updated_at: s.session.updated_at,
                working: s.session.is_working,
                options: Options {
                    model: s.session.model,
                    effort: s.session.effort,
                    agent: s.session.agent,
                },
            })
            .map_err(|e| e.to_string())
    }

    pub(super) fn resume_antigravity(&self, key: &SessionKey) -> Result<Reply, String> {
        self.antigravity
            .resume_session(&key.scope.project, &key.id)
            .map(|(info, page)| {
                Reply::Resumed(
                    Summary {
                        key: key.clone(),
                        title: info.session.title,
                        updated_at: info.session.updated_at,
                        working: info.session.is_working,
                        options: Options {
                            model: info.session.model,
                            effort: info.session.effort,
                            agent: info.session.agent,
                        },
                    },
                    page,
                )
            })
            .map_err(|e| e.to_string())
    }
}

fn antigravity_summaries(
    scope: &Scope,
    sessions: Vec<caelum_protocol::AntigravitySession>,
) -> Vec<Summary> {
    sessions
        .into_iter()
        .map(|s| Summary {
            key: SessionKey {
                scope: scope.clone(),
                id: s.id,
            },
            title: s.title,
            updated_at: s.updated_at,
            working: s.is_working,
            options: Options {
                model: s.model,
                effort: s.effort,
                agent: s.agent,
            },
        })
        .collect()
}
