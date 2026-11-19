use super::{
    boat::{api, Boat},
    credentials, Credential, Machines, Operation, Outcome,
};
use std::{sync::OnceLock, time::Duration};
use tokio::{
    runtime::{Builder, Runtime},
    sync::Mutex,
};

#[derive(Default)]
pub(crate) struct Backend {
    runtime: OnceLock<Result<Runtime, String>>,
    connection: Mutex<Option<Boat>>,
}

impl Backend {
    pub(crate) fn execute(&self, op: &Operation) -> Result<Outcome, String> {
        let rt = self.runtime.get_or_init(|| {
            Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())
        });
        rt.as_ref().map_err(Clone::clone)?.block_on(async {
            tokio::time::timeout(Duration::from_secs(90), self.perform(op))
                .await
                .map_err(|_| "Boat did not respond in time".to_owned())?
        })
    }

    async fn authenticate(key: &Credential) -> Result<Boat, String> {
        Boat::authenticate(boat_sdk::Client::new(key.0.trim()).map_err(api)?).await
    }

    async fn connected(&self) -> Result<Boat, String> {
        let mut slot = self.connection.lock().await;
        if slot.is_none() {
            *slot = Some(Self::authenticate(&credentials::read().await?).await?);
        }
        Ok(slot.as_ref().expect("authenticated").clone())
    }

    async fn perform(&self, op: &Operation) -> Result<Outcome, String> {
        match op {
            Operation::Discover(project) => self
                .connected()
                .await?
                .discover(project)
                .await
                .map(Outcome::Source),
            Operation::Create(source, plan) => self
                .connected()
                .await?
                .create(source, plan)
                .await
                .map(Outcome::Machine),
            Operation::Prepare(target, base, branch) => {
                self.connected()
                    .await?
                    .prepare(target, base, branch.as_deref())
                    .await
            }
            Operation::Power(key, resume) => self
                .connected()
                .await?
                .power(key, *resume)
                .await
                .map(Outcome::Machine),
            Operation::Connect(key) => {
                let mut slot = self.connection.lock().await;
                let boat = Self::authenticate(key).await?;
                let inventory = boat.inventory().await?;
                credentials::store(Some(key)).await?;
                *slot = Some(boat);
                Ok(Outcome::Inventory(inventory))
            }
            Operation::Inventory => self
                .connected()
                .await?
                .inventory()
                .await
                .map(Outcome::Inventory),
            Operation::Disconnect => {
                let mut slot = self.connection.lock().await;
                credentials::store(None).await?;
                *slot = None;
                Ok(Outcome::Disconnected)
            }
        }
    }
}
