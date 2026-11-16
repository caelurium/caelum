use super::{Options, ProviderRuntime, Reply, SessionKey, Summary};
use crate::opencode::OpenCodeError;

impl ProviderRuntime {
    pub(super) fn resume_opencode(&self, key: &SessionKey) -> Result<Reply, String> {
        match self
            .opencode
            .resume_with_history(&key.scope.project, &key.id)
        {
            Ok((info, page)) => Ok(Reply::Resumed(
                Summary {
                    key: key.clone(),
                    title: info.session.title.unwrap_or_default(),
                    updated_at: info.session.updated_at,
                    working: info.session.is_working,
                    options: Options {
                        model: info.session.model,
                        effort: info.session.effort,
                        agent: info.session.agent,
                    },
                },
                page,
            )),
            Err(OpenCodeError::SessionNotInProject) => {
                let project = self
                    .opencode
                    .session_location(&key.scope.project, &key.id)
                    .map_err(|e| e.to_string())?;
                // Git, rather than an ID cached by either client, establishes
                // that this is another checkout of the same scoped project.
                let listing = self
                    .worktrees
                    .list(&key.scope.project, "")
                    .map_err(|e| e.to_string())?;
                if project.path == key.scope.project
                    || !listing.is_some_and(|listing| {
                        listing
                            .worktrees
                            .iter()
                            .any(|tree| tree.available && tree.project.path == project.path)
                    })
                {
                    return Err(format!(
                        "This OpenCode session is now in '{}'. Open that directory to reconnect.",
                        project.path
                    ));
                }
                // No history or mutation is sent against the stale scope. The
                // workspace first adopts the new checkout, then resumes there.
                Ok(Reply::SessionMoved {
                    from: key.clone(),
                    project,
                })
            }
            Err(error) => Err(error.to_string()),
        }
    }
}
