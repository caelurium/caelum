mod checkout;
mod harness;

use super::{Host, Inventory, Machine, MachineId, Machines, Outcome, Plan, Source, Target};
use boat_sdk::{
    sandbox::{
        requests::{ResumeRequest, StopRequest},
        resources::Sandbox,
        SandboxListQuery,
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
    fn machine(&self, s: Sandbox) -> Machine {
        Machine {
            key: MachineId {
                host: Host::Boat,
                account: self.account.clone(),
                id: s.id,
            },
            name: s.name,
            state: s.state,
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
            .map_err(api)?
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
            let page = self.client.list_sandboxes(&query).await.map_err(api)?;
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
    // SDK Display omits response bodies and secret-bearing URLs.
    error.to_string()
}
