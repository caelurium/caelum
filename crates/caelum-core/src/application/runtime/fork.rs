use super::{Options, Provider, ProviderRuntime, Reply, SessionKey, Summary};
use crate::application::ForkBoundary;

impl ProviderRuntime {
    pub(super) fn fork(
        &self,
        source: &SessionKey,
        boundary: &ForkBoundary,
    ) -> Result<Reply, String> {
        let scope = &source.scope;
        let (id, title, updated_at, options) = match (scope.provider, boundary) {
            (Provider::Codex, ForkBoundary::CodexTurn(turn)) => {
                let info = self
                    .codex
                    .fork_session(&scope.project, &source.id, turn)
                    .map_err(|e| e.to_string())?;
                (
                    info.thread.id,
                    info.thread.name.unwrap_or(info.thread.preview),
                    info.thread.updated_at.map(|t| t.saturating_mul(1000)),
                    Options {
                        model: info.model,
                        effort: info.effort,
                        agent: None,
                    },
                )
            }
            (
                Provider::OpenCode,
                ForkBoundary::OpenCodeBefore(msg) | ForkBoundary::OpenCodeTip(msg),
            ) => {
                let s = self
                    .opencode
                    .fork_session(
                        &scope.project,
                        &source.id,
                        msg,
                        matches!(boundary, ForkBoundary::OpenCodeBefore(_)),
                    )
                    .map_err(|e| e.to_string())?
                    .session;
                (
                    s.id,
                    s.title.unwrap_or_default(),
                    s.updated_at,
                    Options {
                        model: s.model,
                        effort: s.effort,
                        agent: s.agent,
                    },
                )
            }
            _ => return Err("Fork boundary belongs to a different provider".into()),
        };
        Ok(Reply::Forked {
            source: source.clone(),
            child: Summary {
                key: SessionKey {
                    scope: scope.clone(),
                    id,
                },
                title,
                updated_at,
                options,
                working: false,
            },
        })
    }
}
