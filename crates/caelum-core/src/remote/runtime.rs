use super::harness::Harness;
use super::{
    boat::{api, Boat},
    credentials, Credential, Inventory, Machines, Operation, Outcome,
};
use std::{sync::OnceLock, time::Duration};
use tokio::{
    runtime::{Builder, Runtime},
    sync::{mpsc, oneshot},
};

enum ConnectionMsg {
    Connected(oneshot::Sender<Result<Boat, String>>),
    Connect {
        key: Credential,
        reply: oneshot::Sender<Result<Inventory, String>>,
    },
    Disconnect(oneshot::Sender<Result<(), String>>),
}

struct BackendRuntime {
    runtime: Runtime,
    connection: mpsc::Sender<ConnectionMsg>,
}

#[derive(Default)]
pub(crate) struct Backend {
    runtime: OnceLock<Result<BackendRuntime, String>>,
    closed: tokio::sync::watch::Sender<bool>,
}

impl Backend {
    pub(crate) fn shutdown(&self) {
        self.closed.send_replace(true);
    }
    pub(crate) fn execute(&self, op: &Operation) -> Result<Outcome, String> {
        let rt = self.runtime.get_or_init(|| {
            let runtime = Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            let (connection, mailbox) = mpsc::channel(32);
            runtime.spawn(run_connection(mailbox, self.closed.subscribe()));
            Ok(BackendRuntime {
                runtime,
                connection,
            })
        });
        let backend = rt.as_ref().map_err(Clone::clone)?;
        backend.runtime.block_on(async {
            let mut closed = self.closed.subscribe();
            let timeout = if matches!(op, Operation::Prepare(_, _, Some(_))) {
                390
            } else {
                90
            };
            tokio::select! {
                biased;
                _ = closed.wait_for(|v| *v) => Err("Remote connection closed".into()),
                result = tokio::time::timeout(Duration::from_secs(timeout), self.perform(&backend.connection, op)) =>
                    result.map_err(|_| "Boat did not respond in time".to_owned())?,
            }
        })
    }

    async fn authenticate(key: &Credential) -> Result<Boat, String> {
        Boat::authenticate(boat_sdk::Client::new(key.0.trim()).map_err(api)?).await
    }

    async fn connected(connection: &mpsc::Sender<ConnectionMsg>) -> Result<Boat, String> {
        let (reply, result) = oneshot::channel();
        connection
            .send(ConnectionMsg::Connected(reply))
            .await
            .map_err(|_| "Remote connection closed".to_owned())?;
        result
            .await
            .map_err(|_| "Remote connection closed".to_owned())?
    }

    async fn perform(
        &self,
        connection: &mpsc::Sender<ConnectionMsg>,
        op: &Operation,
    ) -> Result<Outcome, String> {
        match op {
            Operation::Models => boat_sdk::agents::provider_models()
                .await
                .map(super::models::model_lists)
                .map(Outcome::Models)
                .map_err(api),
            Operation::Delete(key) => {
                let confirmed = Self::connected(connection).await?.delete(key).await?;
                if let Outcome::Deleted(id) = &confirmed {
                    if let Err(e) =
                        super::native::retire(&crate::application::Store::default_path(), id)
                    {
                        eprintln!("Could not retire the deleted VM's native workspace: {e}");
                    }
                }
                Ok(confirmed)
            }
            Operation::Stats(key) => Self::connected(connection)
                .await?
                .stats(key)
                .await
                .map(Outcome::Stats),
            Operation::Git(target, action) => Self::connected(connection)
                .await?
                .git(target, action)
                .await
                .map(Outcome::Review),
            Operation::Prompt(target, prompt) => match Self::connected(connection).await {
                Ok(boat) => boat.send(target, prompt).await,
                Err(error) => Ok(Outcome::Rejected(error)),
            },
            Operation::Follow(target, run, cursor) => Self::connected(connection)
                .await?
                .follow(target, run, cursor.as_deref())
                .await
                .map(Outcome::Page),
            Operation::InspectTool(key, source) => Self::connected(connection)
                .await?
                .tool_output(key, source)
                .await
                .map(Outcome::ToolOutput),
            Operation::Interrupt(target, run) => Self::connected(connection)
                .await?
                .interrupt(target, run)
                .await
                .map(|_| Outcome::Interrupted),
            Operation::Recover(target, provider, conv, prompt) => Self::connected(connection)
                .await?
                .recover(target, *provider, conv.as_deref(), prompt)
                .await
                .map(Outcome::Accepted),
            Operation::Discover(project) => Self::connected(connection)
                .await?
                .discover(project, false)
                .await
                .map(Outcome::Source),
            Operation::DiscoverHandoff(project) => Self::connected(connection)
                .await?
                .discover(project, true)
                .await
                .map(Outcome::Source),
            Operation::MatchCheckout(project, target, publication) => {
                let git = crate::git::GitClient::new();
                if !git
                    .github_origin(project)?
                    .eq_ignore_ascii_case(&target.repository)
                {
                    return Err("The VM checkout has a different GitHub origin.".into());
                }
                let (base, boat) = tokio::try_join!(
                    git.destination_base(project, publication.as_ref()),
                    Self::connected(connection)
                )?;
                let mut target = target.clone();
                target.required_base = Some(base.clone());
                boat.prepare(&target, &base.branch, None).await
            }
            Operation::Lifetime(key, lifetime) => Self::connected(connection)
                .await?
                .lifetime(key, *lifetime)
                .await
                .map(Outcome::Lifetime),
            Operation::Create(source, plan) => {
                let boat = Self::connected(connection).await?;
                if boat.account != source.account {
                    return Err("Connect the Boat account selected for this checkout".into());
                }
                if source.publish.is_some()
                    && (source.base != plan.branch || uuid::Uuid::parse_str(&plan.token).is_err())
                {
                    return Err("Invalid branch publication plan. Reopen VM setup.".into());
                }
                if source.published_tip.is_some() || source.publish.is_some() {
                    crate::git::GitClient::new()
                        .ensure_vm_branch(source, &plan.token, crate::github::create_vm_branch)
                        .await?;
                }
                boat.create(source, plan).await.map(Outcome::Machine)
            }
            Operation::Prepare(target, base, branch) => {
                Self::connected(connection)
                    .await?
                    .prepare(target, base, branch.as_deref())
                    .await
            }
            Operation::Power(key, resume) => Self::connected(connection)
                .await?
                .power(key, *resume)
                .await
                .map(Outcome::Machine),
            Operation::Connect(key) => {
                let (reply, result) = oneshot::channel();
                connection
                    .send(ConnectionMsg::Connect {
                        key: key.clone(),
                        reply,
                    })
                    .await
                    .map_err(|_| "Remote connection closed".to_owned())?;
                result
                    .await
                    .map_err(|_| "Remote connection closed".to_owned())?
                    .map(Outcome::Inventory)
            }
            Operation::Inventory => Self::connected(connection)
                .await?
                .inventory()
                .await
                .map(Outcome::Inventory),
            Operation::Disconnect => {
                let (reply, result) = oneshot::channel();
                connection
                    .send(ConnectionMsg::Disconnect(reply))
                    .await
                    .map_err(|_| "Remote connection closed".to_owned())?;
                result
                    .await
                    .map_err(|_| "Remote connection closed".to_owned())??;
                Ok(Outcome::Disconnected)
            }
        }
    }
}

async fn run_connection(
    mut mailbox: mpsc::Receiver<ConnectionMsg>,
    mut closed: tokio::sync::watch::Receiver<bool>,
) {
    let mut slot: Option<Boat> = None;
    loop {
        if *closed.borrow() {
            break;
        }
        tokio::select! {
            biased;
            _ = closed.changed() => continue,
            message = mailbox.recv() => match message {
                Some(ConnectionMsg::Connected(reply)) => {
                    let result = if let Some(boat) = &slot {
                        Ok(boat.clone())
                    } else {
                        match credentials::read().await {
                            Ok(key) => match Backend::authenticate(&key).await {
                                Ok(boat) => {
                                    slot = Some(boat.clone());
                                    Ok(boat)
                                }
                                Err(error) => Err(error),
                            },
                            Err(error) => Err(error),
                        }
                    };
                    let _ = reply.send(result);
                }
                Some(ConnectionMsg::Connect { key, reply }) => {
                    let result = async {
                        let boat = Backend::authenticate(&key).await?;
                        let inventory = boat.inventory().await?;
                        credentials::store(Some(&key)).await?;
                        slot = Some(boat);
                        Ok(inventory)
                    }
                    .await;
                    let _ = reply.send(result);
                }
                Some(ConnectionMsg::Disconnect(reply)) => {
                    let result = match credentials::store(None).await {
                        Ok(()) => {
                            slot = None;
                            Ok(())
                        }
                        Err(error) => Err(error),
                    };
                    let _ = reply.send(result);
                }
                None => break,
            }
        }
    }
}
