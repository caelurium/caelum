use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatsProvider {
    Codex,
    OpenCode,
    Antigravity,
    Boat,
}

impl StatsProvider {
    pub fn name(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::OpenCode => "OpenCode",
            Self::Antigravity => "Antigravity",
            Self::Boat => "Boat",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeStatsFilter {
    /// Inclusive Unix milliseconds. Omit for all recorded history.
    pub from: Option<u64>,
    /// Exclusive Unix milliseconds.
    pub to: Option<u64>,
    pub project: Option<String>,
    pub timezone: Option<String>,
    pub provider: Option<StatsProvider>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeStatsProject {
    pub id: String,
    pub name: Option<String>,
    pub canonical: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatsRange {
    pub from: f64,
    pub to: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatsCacheTokens {
    pub read: f64,
    pub write: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatsTokens {
    pub input: f64,
    pub output: f64,
    pub reasoning: f64,
    pub cache: StatsCacheTokens,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsModelRef {
    pub id: String,
    #[serde(rename = "providerID")]
    pub provider_id: String,
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsModelUsage {
    pub model: StatsModelRef,
    pub steps: u64,
    pub tokens: StatsTokens,
    pub cost: f64,
    #[serde(default)]
    pub unpriced: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsActivity {
    pub date: String,
    pub steps: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeStats {
    pub range: StatsRange,
    pub sessions: u64,
    pub subagents: u64,
    pub prompts: u64,
    pub steps: u64,
    pub tokens: StatsTokens,
    pub cost: f64,
    pub active_days: u64,
    pub streak: u64,
    pub activity: Vec<StatsActivity>,
    pub models: Vec<StatsModelUsage>,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub estimated_turns: u64,
    #[serde(default)]
    pub unpriced: u64,
    #[serde(default)]
    pub recorded_only: bool,
    #[serde(default)]
    pub dropped: u64,
}
