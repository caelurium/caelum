use super::{Host, Inventory, Machine, MachineId, Machines};
use boat_sdk::{sandbox::SandboxListQuery, Client};

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(super) struct Boat {
    pub(super) client: Client,
    pub(super) account: String,
}

impl Boat {
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
            machines.extend(page.sandboxes.into_iter().map(|s| Machine {
                key: MachineId {
                    host: Host::Boat,
                    account: self.account.clone(),
                    id: s.id,
                },
                name: s.name,
                state: s.state,
            }));
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
}

pub(super) fn api(error: boat_sdk::Error) -> String {
    // SDK Display omits response bodies and secret-bearing URLs.
    error.to_string()
}
