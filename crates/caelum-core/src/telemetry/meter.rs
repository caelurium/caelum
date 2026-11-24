use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Meter {
    pub input: u64,
    pub output: u64,
    pub read: u64,
    pub write: u64,
    pub reasoning: u64,
    pub cost: Option<f64>,
}

impl Meter {
    pub(super) fn visible(input: usize, output: usize) -> Self {
        Self {
            input: input.div_ceil(4) as u64,
            output: output.div_ceil(4) as u64,
            ..Default::default()
        }
    }
    /// Provider totals include cached input and reasoning output; the chart uses disjoint buckets.
    pub(super) fn inclusive(v: &Value) -> Option<Self> {
        let input = v["inputTokens"].as_u64()?;
        let output = v["outputTokens"].as_u64()?;
        let read = v["cachedInputTokens"]
            .as_u64()
            .or(v["cachedReadTokens"].as_u64())
            .unwrap_or(0);
        let write = v["cachedWriteTokens"].as_u64().unwrap_or(0);
        let reasoning = v["reasoningOutputTokens"]
            .as_u64()
            .or(v["thoughtTokens"].as_u64())
            .unwrap_or(0);
        Some(Self {
            input: input.saturating_sub(read).saturating_sub(write),
            output: output.saturating_sub(reasoning),
            read,
            write,
            reasoning,
            cost: usd(&v["cost"]),
        })
    }
    pub(super) fn step(v: &Value) -> Option<Self> {
        let t = &v["tokens"];
        Some(Self {
            input: t["input"].as_u64()?,
            output: t["output"].as_u64()?,
            read: t["cache"]["read"].as_u64().unwrap_or(0),
            write: t["cache"]["write"].as_u64().unwrap_or(0),
            reasoning: t["reasoning"].as_u64().unwrap_or(0),
            cost: usd(&v["cost"]),
        })
    }
    pub(super) fn add(&mut self, m: &Self) {
        self.input = self.input.saturating_add(m.input);
        self.output = self.output.saturating_add(m.output);
        self.read = self.read.saturating_add(m.read);
        self.write = self.write.saturating_add(m.write);
        self.reasoning = self.reasoning.saturating_add(m.reasoning);
        self.cost = self.cost.zip(m.cost).map(|(a, b)| a + b);
    }
    pub(super) fn price(&self, model: &str) -> Option<f64> {
        let (input, cached, write, output) = match model.rsplit('/').next()? {
            "gpt-5" | "gpt-5-codex" | "gpt-5.1-codex" | "gpt-5.1-codex-max" => {
                (1.25, 0.125, 1.25, 10.0)
            }
            "gpt-5.2" | "gpt-5.2-codex" | "gpt-5.3-codex" => (1.75, 0.175, 1.75, 14.0),
            "claude-sonnet-4-6" => (3.0, 0.30, 3.75, 15.0),
            _ => return None,
        };
        Some(
            (self.input as f64 * input
                + self.read as f64 * cached
                + self.write as f64 * write
                + (self.output as f64 + self.reasoning as f64) * output)
                / 1_000_000.0,
        )
    }
}

fn usd(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| {
            (v["currency"] == "USD")
                .then(|| v["amount"].as_f64())
                .flatten()
        })
        .filter(|n| n.is_finite() && *n >= 0.)
}
