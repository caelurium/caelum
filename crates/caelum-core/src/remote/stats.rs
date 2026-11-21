use super::Machine;
use std::time::Instant;

#[derive(Debug)]
pub struct Process {
    pub name: String,
    pub cpu_percent: f64,
    pub memory_mb: Option<f64>,
}

#[derive(Debug)]
pub enum Activity {
    Live(Option<Process>),
    LastSample(Option<Process>),
    Unavailable,
}

#[derive(Debug)]
pub struct Sample {
    pub machine: Machine,
    pub vcpu: Option<i64>,
    pub memory_gb: Option<i64>,
    pub activity: Activity,
}

#[derive(Debug, Default)]
pub enum Monitor {
    #[default]
    Off,
    Loading {
        owner: String,
        request: u64,
        previous: Option<Sample>,
    },
    Showing {
        owner: String,
        sample: Sample,
        next: Instant,
    },
    Failed {
        owner: String,
        error: String,
    },
}

impl Monitor {
    pub(crate) fn owns(&self, id: &str) -> bool {
        matches!(self, Self::Loading { owner, .. } | Self::Showing {owner,..} | Self::Failed{owner,..} if owner == id)
    }
    pub fn sample(&self, id: &str) -> Option<&Sample> {
        match self {
            Self::Loading {
                owner,
                previous: Some(s),
                ..
            }
            | Self::Showing {
                owner, sample: s, ..
            } if owner == id => Some(s),
            _ => None,
        }
    }
}
