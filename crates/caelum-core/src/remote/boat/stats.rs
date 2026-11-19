use super::Boat;
use crate::remote::{Activity, MachineId, Process, Sample};

impl Boat {
    pub(in crate::remote) async fn stats(&self, key: &MachineId) -> Result<Sample, String> {
        let sandbox = self.inspect(key).await?;
        let activity = if sandbox.monitoring == Some(false)
            || !matches!(sandbox.state.as_str(), "ready" | "running" | "idle")
        {
            Activity::Unavailable
        } else if let Ok(list) = self.client.processes(&key.id).await {
            let busiest = list
                .processes
                .into_iter()
                .filter_map(|p| {
                    let cpu = p.cpu_percent.filter(|n| n.is_finite() && *n >= 0.)?;
                    // Keep only the executable label; command-line arguments can carry tokens.
                    let name = p.command.split_whitespace().next()?.rsplit('/').next()?;
                    let name = if name.len() <= 64
                        && name
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
                    {
                        name
                    } else {
                        "process"
                    };
                    Some(Process {
                        name: name.into(),
                        cpu_percent: cpu,
                        memory_mb: p.memory_mb.filter(|n| n.is_finite() && *n >= 0.),
                    })
                })
                .max_by(|a, b| a.cpu_percent.total_cmp(&b.cpu_percent));
            if list.live {
                Activity::Live(busiest)
            } else {
                Activity::LastSample(busiest)
            }
        } else {
            Activity::Unavailable
        };
        Ok(Sample {
            vcpu: sandbox.vcpu,
            memory_gb: sandbox.memory_gb,
            machine: self.machine(sandbox),
            activity,
        })
    }
}
