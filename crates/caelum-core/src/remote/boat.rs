mod checkout;
mod git;
mod harness;
mod setup_error;
mod stats;

use super::{
    Host, Inventory, Lifetime, Machine, MachineId, Machines, Outcome, Plan, Source, Target,
};
use boat_sdk::{
    sandbox::{
        requests::{ResumeRequest, StopRequest, UpdateSandboxRequest},
        resources::Sandbox,
        AutoStop, SandboxListQuery,
    },
    Client,
};

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(super) struct Boat {
    pub(super) client: Client,
    pub(super) account: String,
}

impl Boat {
    pub(super) async fn lifetime(
        &self,
        key: &MachineId,
        lifetime: Lifetime,
    ) -> Result<Machine, String> {
        self.owns(key)?;
        let result = self
            .client
            .update_sandbox(
                &key.id,
                &UpdateSandboxRequest {
                    ttl_seconds: Some(auto_stop(lifetime)),
                    ..Default::default()
                },
            )
            .await
            .map_err(api)?;
        if !result.ok || result.sandbox.id != key.id {
            return Err("Boat did not update the selected machine.".into());
        }
        Ok(self.machine(result.sandbox))
    }

    fn machine(&self, s: Sandbox) -> Machine {
        Machine {
            key: MachineId {
                host: Host::Boat,
                account: self.account.clone(),
                id: s.id,
            },
            name: s.name,
            state: s.state,
            auto_stop_unix: s
                .archive_after
                .as_deref()
                .and_then(|value| {
                    time::OffsetDateTime::parse(
                        value,
                        &time::format_description::well_known::Rfc3339,
                    )
                    .ok()
                })
                .map(|time| time.unix_timestamp()),
        }
    }
    fn owns(&self, key: &MachineId) -> Result<(), String> {
        if key.account == self.account && key.host == Host::Boat {
            Ok(())
        } else {
            Err("Connect the Boat account that owns this machine".into())
        }
    }
    pub(super) async fn authenticate(client: Client) -> Result<Self, String> {
        let account = client
            .me()
            .await
            .map_err(|e| setup_error::Step::ReadAccount.error(e))?
            .user
            .id
            .filter(|id| !id.is_empty())
            .ok_or("Boat returned no account ID")?;
        Ok(Self { client, account })
    }
}

impl Machines for Boat {
    async fn inventory(&self) -> Result<Inventory, String> {
        let mut machines = Vec::new();
        let mut query = SandboxListQuery {
            limit: Some(100),
            ..Default::default()
        };
        loop {
            let page = self
                .client
                .list_sandboxes(&query)
                .await
                .map_err(|e| setup_error::Step::ListMachines.error(e))?;
            machines.extend(page.sandboxes.into_iter().map(|s| self.machine(s)));
            let Some(info) = page.page_info.filter(|p| p.has_more) else {
                break;
            };
            let cursor = info
                .next_cursor
                .filter(|c| Some(c) != query.cursor.as_ref())
                .ok_or("Boat returned an invalid machine list cursor")?;
            query.cursor = Some(cursor);
        }
        Ok(Inventory {
            account: self.account.clone(),
            machines,
        })
    }

    async fn create(&self, source: &Source, plan: &Plan) -> Result<Machine, String> {
        self.create_checkout(source, plan).await
    }
    async fn power(&self, machine: &MachineId, resume: bool) -> Result<Machine, String> {
        self.owns(machine)?;
        let response = if resume {
            self.client
                .resume_sandbox(&machine.id, &ResumeRequest::default())
                .await
        } else {
            self.client
                .stop_sandbox(&machine.id, &StopRequest::default())
                .await
        }
        .map_err(api)?;
        if response.id != machine.id {
            return Err("Boat returned a different machine".into());
        }
        self.inspect(machine).await.map(|s| self.machine(s))
    }
    async fn prepare(
        &self,
        target: &Target,
        base: &str,
        branch: Option<&str>,
    ) -> Result<Outcome, String> {
        self.prepare_checkout(target, base, branch).await
    }
}

pub(super) fn api(error: boat_sdk::Error) -> String {
    if matches!(&error, boat_sdk::Error::Api {code, ..} if code.as_deref() == Some("trial_auto_stop_required"))
    {
        return "Boat trials require an auto-stop of 2 hours or less.".into();
    }
    // SDK Display omits response bodies and secret-bearing URLs.
    error.to_string()
}

fn auto_stop(lifetime: Lifetime) -> AutoStop {
    Option::<u32>::from(lifetime).map_or(AutoStop::Disabled, |m| AutoStop::AfterSeconds(m * 60))
}
