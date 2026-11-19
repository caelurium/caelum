//! Machine identities and operations are independent of local project paths and coding providers.
mod boat;
mod credentials;
mod runtime;

pub(crate) use runtime::Backend;

use serde::{Deserialize, Serialize};
use std::{fmt, sync::Arc};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Credential(Arc<Zeroizing<String>>);

impl Credential {
    pub fn new(key: String) -> Self {
        Self(Arc::new(Zeroizing::new(key)))
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Host {
    Boat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineId {
    pub host: Host,
    pub account: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Machine {
    pub key: MachineId,
    pub name: String,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inventory {
    pub account: String,
    pub machines: Vec<Machine>,
}

trait Machines {
    async fn inventory(&self) -> Result<Inventory, String>;
}

#[derive(Clone, Debug)]
pub enum Operation {
    Connect(Credential),
    Inventory,
    Disconnect,
}

#[derive(Debug)]
pub enum Outcome {
    Inventory(Inventory),
    Disconnected,
}

#[derive(Debug, Default)]
pub struct State {
    pub inventory: Option<Inventory>,
    pub connection: Connection,
}

#[derive(Debug, Default)]
pub enum Connection {
    #[default]
    Idle,
    Loading(u64),
    Failed(String),
}
