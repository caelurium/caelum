use super::{api, Boat};
use crate::remote::{
    checkout::repo_name, github_slug, valid_branch, Machine, MachineId, Outcome, Plan, Size,
    Source, Target,
};
use boat_sdk::{
    commands::CommandRequest,
    environments::models::*,
    sandbox::{requests::CreateSandboxRequest, resources::Sandbox, AutoStop, MachineSize},
};

fn suitable(e: &SandboxEnvironment, repo: &str) -> bool {
    !e.safe_for_third_parties
        && e.pass_github
        && e.pass_agents_credentials
        && matches!(e.selected_repositories.as_deref(), Some([r]) if r.full_name.as_ref().is_some_and(|name| name.eq_ignore_ascii_case(repo)))
}

pub(super) fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

impl Boat {
    pub(in crate::remote) async fn discover(&self, project: &str) -> Result<Source, String> {
        let (repository, base) = crate::git::GitClient::new().published_source(project)?;
        let environment = self
            .client
            .environments(None)
            .await
            .map_err(api)?
            .environments
            .into_iter()
            .find(|e| suitable(e, &repository))
            .map(|e| e.name);
        Ok(Source {
            project: project.to_owned(),
            account: self.account.clone(),
            repository,
            base,
            environment,
        })
    }

    async fn environment(&self, source: &Source, plan: &Plan) -> Result<String, String> {
        let dedicated = format!("caelum-{}", plan.token);
        let name = source.environment.as_deref().unwrap_or(&dedicated);
        let envs = self
            .client
            .environments(None)
            .await
            .map_err(api)?
            .environments;
        let env = envs.into_iter().find(|e| e.name == name);
        if env
            .as_ref()
            .is_some_and(|e| suitable(e, &source.repository))
        {
            return Ok(name.into());
        }
        // Only the private name minted for this create attempt may be configured.
        if source.environment.is_some() {
            return Err("The selected Boat environment changed. Choose the project again.".into());
        }
        let repo = self
            .client
            .repos(&ReposQuery {
                q: Some(source.repository.clone()),
                sync: Some(true),
                ..Default::default()
            })
            .await
            .map_err(api)?
            .installations
            .into_iter()
            .flat_map(|i| i.repositories.unwrap_or_default())
            .find(|r| {
                r.full_name
                    .as_ref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(&source.repository))
            })
            .and_then(|r| r.database_id)
            .ok_or("Connect this GitHub repository to Boat before creating a machine")?;
        let env = match env {
            Some(env) => env,
            None => self
                .client
                .create_environment(None, &CreateSandboxEnvironmentRequest { name: name.into() })
                .await
                .map_err(api)?
                .environments
                .into_iter()
                .find(|e| e.name == name)
                .ok_or("Boat did not return the new environment")?,
        };
        if env.is_default
            || env
                .selected_repositories
                .as_ref()
                .is_some_and(|r| !r.is_empty())
        {
            return Err("The checkout environment is already in use".into());
        }
        let response = self
            .client
            .update_environment(
                &env.id,
                &UpdateSandboxEnvironmentRequest {
                    pass_github: Some(true),
                    pass_agents_credentials: Some(true),
                    pass_secrets: Some(false),
                    pass_sandbox_credentials: Some(false),
                    safe_for_third_parties: Some(false),
                    repositories: Some(vec![EnvironmentRepository {
                        repository_id: repo,
                        base_branch: Some(source.base.clone()),
                        setup_script: None,
                        setup_blocking: None,
                    }]),
                    ..Default::default()
                },
            )
            .await
            .map_err(api)?;
        if !response.success {
            return Err("Could not configure the new checkout environment".into());
        }
        Ok(name.into())
    }

    pub(super) async fn create_checkout(
        &self,
        source: &Source,
        plan: &Plan,
    ) -> Result<Machine, String> {
        if source.account != self.account {
            return Err("Connect the Boat account selected for this checkout".into());
        }
        let name = repo_name(&source.repository)?;
        let token =
            uuid::Uuid::parse_str(&plan.token).map_err(|_| "Invalid machine create request")?;
        if !valid_branch(&plan.branch)
            || !valid_branch(&source.base)
            || plan.branch == source.base
            || !matches!(plan.minutes, 30 | 60 | 120)
        {
            return Err("Choose a published base and a different, valid new branch name".into());
        }
        let environment = self.environment(source, plan).await?;
        let result = self
            .client
            .create_sandbox(
                &CreateSandboxRequest {
                    name: Some(format!(
                        "{}-{}",
                        &name[..name.len().min(32)],
                        &token.simple().to_string()[..8]
                    )),
                    machine_type: Some(match plan.size {
                        Size::Small => MachineSize::Small,
                        Size::Standard => MachineSize::Default,
                        Size::Large => MachineSize::Large,
                        Size::Xlarge => MachineSize::Xlarge,
                    }),
                    ttl_seconds: Some(AutoStop::AfterSeconds(plan.minutes * 60)),
                    environment: Some(environment),
                    ..Default::default()
                },
                Some(&plan.token),
            )
            .await
            .map_err(api)?;
        Ok(self.machine(result.sandbox))
    }

    pub(super) async fn inspect(&self, key: &MachineId) -> Result<Sandbox, String> {
        self.owns(key)?;
        let sandbox = self.client.get_sandbox(&key.id).await.map_err(api)?.sandbox;
        if sandbox.id != key.id {
            return Err("Boat returned a different machine".into());
        }
        Ok(sandbox)
    }

    pub(super) async fn command(&self, target: &Target, command: &str) -> Result<String, String> {
        self.owns(&target.machine)?;
        let dir = format!("/home/user/{}", repo_name(&target.repository)?);
        let command = format!("cd {} && {{ {}; }}", quote(&dir), command);
        let result = self
            .client
            .run_command(&target.machine.id, &CommandRequest::new(command))
            .await
            .map_err(api)?;
        if !result.success || result.timed_out || result.exit_code != Some(0) {
            // Git diagnostics may contain credential-bearing remote URLs.
            return Err(format!("Remote Git command failed (exit {}). Check the checkout, branch and GitHub access on the machine.",
                result.exit_code.map_or_else(|| "unknown".to_owned(), |n| n.to_string())));
        }
        if result.stdout_truncated == Some(true) {
            return Err("The remote command output was truncated".into());
        }
        Ok(result.stdout)
    }

    pub(super) async fn verify_origin(&self, target: &Target) -> Result<(), String> {
        let origin = self
            .command(
                target,
                "test \"$(git rev-parse --show-toplevel)\" = \"$PWD\" && git remote get-url origin",
            )
            .await?;
        if !github_slug(&origin).is_some_and(|s| s.eq_ignore_ascii_case(&target.repository)) {
            return Err("The remote checkout does not match the selected GitHub repository".into());
        }
        Ok(())
    }

    pub(super) async fn prepare_checkout(
        &self,
        target: &Target,
        base: &str,
        branch: Option<&str>,
    ) -> Result<Outcome, String> {
        let sandbox = self.inspect(&target.machine).await?;
        if matches!(
            sandbox.state.as_str(),
            "archived" | "stopped" | "error" | "failed" | "cancelled"
        ) {
            return Ok(Outcome::Machine(self.machine(sandbox)));
        }
        if matches!(
            sandbox.state.as_str(),
            "provisioning" | "provisioned" | "cloning" | "archiving"
        ) || sandbox.hydrated == Some(false)
            || matches!(sandbox.setup_status.as_deref(), Some("pending" | "running"))
        {
            return Ok(Outcome::Waiting(self.machine(sandbox)));
        }
        if !matches!(sandbox.state.as_str(), "idle" | "ready" | "running") {
            return Err(format!("Unsupported Boat machine state: {}", sandbox.state));
        }
        if sandbox.setup_status.as_deref() == Some("failed") {
            return Err("Boat repository setup failed".into());
        }
        let envs = self.client.environments(None).await.map_err(api)?;
        if !envs.environments.iter().any(|e| {
            Some(&e.name) == sandbox.environment.as_ref()
                && e.latest_version_number == sandbox.environment_version
                && suitable(e, &target.repository)
        }) {
            return Err("This machine needs a current single-repository environment for the selected GitHub repository".into());
        }
        self.verify_origin(target).await?;
        if let Some(branch) = branch {
            if !valid_branch(branch) || !valid_branch(base) || branch == base {
                return Err("Invalid checkout branch".into());
            }
            // A lost response after switch is safe to retry; never reset an existing branch.
            self.command(target, &format!("if test \"$(git branch --show-current)\" != {branch}; then git fetch --no-tags origin {base} && git switch --no-track -c {branch} FETCH_HEAD; fi",
                branch = quote(branch), base = quote(&format!("refs/heads/{base}")))).await?;
        }
        let branch = self.command(target, "git branch --show-current").await?;
        if !valid_branch(branch.trim()) {
            return Err("Select a named branch on the remote checkout".into());
        }
        Ok(Outcome::Prepared(
            self.machine(sandbox),
            branch.trim().to_owned(),
        ))
    }
}
