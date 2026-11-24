//! Safe, bounded local-host resource snapshots for the desktop Overview screen.
use std::collections::VecDeque;

const MAX_HISTORY: usize = 16;

#[derive(Debug, Clone, PartialEq)]
pub struct HostSample {
    pub name: String,
    pub used_gb: f32,
    pub total_gb: f32,
    pub cpu_percent: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HostHistory {
    pub name: String,
    pub used_gb: f32,
    pub total_gb: f32,
    pub cpu_percent: f32,
    pub cpu_history: VecDeque<f32>,
}

impl HostHistory {
    pub fn from_sample(sample: HostSample) -> Self {
        let mut cpu_history = VecDeque::with_capacity(MAX_HISTORY);
        cpu_history.push_back(sample.cpu_percent.clamp(0.0, 100.0));
        Self {
            name: sample.name,
            used_gb: sample.used_gb,
            total_gb: sample.total_gb,
            cpu_percent: sample.cpu_percent.clamp(0.0, 100.0),
            cpu_history,
        }
    }

    pub fn push(&mut self, sample: HostSample) {
        self.name = sample.name;
        self.used_gb = sample.used_gb;
        self.total_gb = sample.total_gb;
        self.cpu_percent = sample.cpu_percent.clamp(0.0, 100.0);
        if self.cpu_history.len() >= MAX_HISTORY {
            self.cpu_history.pop_front();
        }
        self.cpu_history.push_back(self.cpu_percent);
    }
}

pub fn sample() -> Result<HostSample, String> {
    #[cfg(target_os = "macos")]
    {
        sample_macos()
    }
    #[cfg(target_os = "linux")]
    {
        sample_linux()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err("Local host stats are unsupported on this platform".into())
    }
}

#[cfg(target_os = "macos")]
fn command_stdout(program: &str, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    if !output.status.success() {
        return Err(format!("{program} exited with {}", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(target_os = "macos")]
fn sample_macos() -> Result<HostSample, String> {
    let name = command_stdout("scutil", &["--get", "ComputerName"])
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Mac".into());
    let total_bytes: u64 = command_stdout("sysctl", &["-n", "hw.memsize"])?
        .trim()
        .parse()
        .map_err(|e| format!("hw.memsize: {e}"))?;
    let total_gb = total_bytes as f32 / (1024.0 * 1024.0 * 1024.0);
    let vm_stat = command_stdout("vm_stat", &[])?;
    let used_gb = parse_vm_stat_used_gb(&vm_stat).unwrap_or(0.0).min(total_gb);
    let cpu_percent = command_stdout("top", &["-l", "1", "-n", "0", "-stats", "cpu"])
        .ok()
        .and_then(|out| parse_top_cpu_percent(&out))
        .unwrap_or(0.0);
    Ok(HostSample {
        name,
        used_gb,
        total_gb,
        cpu_percent,
    })
}

#[cfg(target_os = "linux")]
fn sample_linux() -> Result<HostSample, String> {
    let name = std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Linux".into());
    let meminfo = std::fs::read_to_string("/proc/meminfo").map_err(|e| e.to_string())?;
    let (used_gb, total_gb) =
        parse_proc_meminfo(&meminfo).ok_or_else(|| "Unable to parse /proc/meminfo".to_owned())?;
    let loadavg = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let cores = std::thread::available_parallelism()
        .map(|n| n.get() as f32)
        .unwrap_or(1.0);
    let cpu_percent = parse_proc_loadavg(&loadavg, cores).unwrap_or(0.0);
    Ok(HostSample {
        name,
        used_gb,
        total_gb,
        cpu_percent,
    })
}

#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
pub(crate) fn parse_vm_stat_used_gb(output: &str) -> Option<f32> {
    let mut page_size: u64 = 16_384;
    let mut active: u64 = 0;
    let mut wired: u64 = 0;
    let mut compressed: u64 = 0;
    for line in output.lines() {
        if let Some(rest) = line.split("page size of ").nth(1) {
            if let Some(bytes) = rest.split_whitespace().next().and_then(|v| v.parse().ok()) {
                page_size = bytes;
            }
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let pages: u64 = value
            .trim()
            .trim_end_matches('.')
            .parse()
            .unwrap_or_default();
        match key.trim() {
            "Pages active" => active = pages,
            "Pages wired down" => wired = pages,
            "Pages occupied by compressor" => compressed = pages,
            _ => {}
        }
    }
    let used_bytes = active
        .saturating_add(wired)
        .saturating_add(compressed)
        .saturating_mul(page_size);
    (used_bytes > 0).then_some(used_bytes as f32 / (1024.0 * 1024.0 * 1024.0))
}

#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
pub(crate) fn parse_top_cpu_percent(output: &str) -> Option<f32> {
    for line in output.lines() {
        let Some(rest) = line.strip_prefix("CPU usage:") else {
            continue;
        };
        let mut user = None;
        let mut sys = None;
        for part in rest.split(',') {
            let part = part.trim();
            if let Some(v) = part.strip_suffix("% user") {
                user = v.trim().parse::<f32>().ok();
            } else if let Some(v) = part.strip_suffix("% sys") {
                sys = v.trim().parse::<f32>().ok();
            }
        }
        if let (Some(u), Some(s)) = (user, sys) {
            return Some((u + s).clamp(0.0, 100.0));
        }
    }
    None
}

#[cfg_attr(not(any(test, target_os = "linux")), allow(dead_code))]
pub(crate) fn parse_proc_meminfo(output: &str) -> Option<(f32, f32)> {
    let mut total_kb = None;
    let mut avail_kb = None;
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total_kb = rest.split_whitespace().next()?.parse::<u64>().ok();
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            avail_kb = rest.split_whitespace().next()?.parse::<u64>().ok();
        }
    }
    let total = total_kb?;
    let avail = avail_kb?;
    let used = total.saturating_sub(avail);
    let denom = 1024.0 * 1024.0;
    Some((used as f32 / denom, total as f32 / denom))
}

#[cfg_attr(not(any(test, target_os = "linux")), allow(dead_code))]
pub(crate) fn parse_proc_loadavg(output: &str, cores: f32) -> Option<f32> {
    let one_min: f32 = output.split_whitespace().next()?.parse().ok()?;
    Some((one_min / cores.max(1.0) * 100.0).clamp(0.0, 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_macos_vm_stat_and_top_cpu() {
        let vm = "\
Mach Virtual Memory Statistics: (page size of 16384 bytes)
Pages free:                               10000.
Pages active:                            700000.
Pages inactive:                          300000.
Pages wired down:                        200000.
Pages occupied by compressor:            100000.
";
        let used = parse_vm_stat_used_gb(vm).unwrap();
        let expected = (1_000_000.0 * 16_384.0) / (1024.0 * 1024.0 * 1024.0);
        assert!((used - expected).abs() < 0.01);

        let top =
            "Processes: 512 total, 3 running\nCPU usage: 18.42% user, 8.58% sys, 73.00% idle \n";
        assert!((parse_top_cpu_percent(top).unwrap() - 27.0).abs() < 0.01);
    }

    #[test]
    fn parses_linux_meminfo_and_loadavg_and_bounds_history() {
        let mem = "MemTotal:       33554432 kB\nMemFree:         2000000 kB\nMemAvailable:   16777216 kB\n";
        let (used, total) = parse_proc_meminfo(mem).unwrap();
        assert!((total - 32.0).abs() < 0.01);
        assert!((used - 16.0).abs() < 0.01);
        assert!((parse_proc_loadavg("2.00 1.50 1.00 1/200 123", 8.0).unwrap() - 25.0).abs() < 0.01);

        let mut history = HostHistory::from_sample(HostSample {
            name: "MacBook Pro".into(),
            used_gb: 20.0,
            total_gb: 48.0,
            cpu_percent: 10.0,
        });
        for i in 1..=25 {
            history.push(HostSample {
                name: "MacBook Pro".into(),
                used_gb: 21.0,
                total_gb: 48.0,
                cpu_percent: i as f32,
            });
        }
        assert_eq!(history.cpu_history.len(), MAX_HISTORY);
        assert_eq!(*history.cpu_history.front().unwrap(), 10.0);
        assert_eq!(*history.cpu_history.back().unwrap(), 25.0);
    }
}
