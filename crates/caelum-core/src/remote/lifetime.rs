use serde::{Deserialize, Serialize};

/// Saved create attempts used numeric minutes. `null` is an explicit manual stop.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Option<u32>", into = "Option<u32>")]
pub enum Lifetime {
    HalfHour,
    #[default]
    Hour,
    TwoHours,
    EightHours,
    Day,
    UntilStopped,
}

impl Lifetime {
    pub const CHOICES: [Self; 6] = [
        Self::HalfHour,
        Self::Hour,
        Self::TwoHours,
        Self::EightHours,
        Self::Day,
        Self::UntilStopped,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::HalfHour => "30 min",
            Self::Hour => "1 h",
            Self::TwoHours => "2 h",
            Self::EightHours => "8 h",
            Self::Day => "24 h",
            Self::UntilStopped => "Until stopped",
        }
    }
}

impl From<Lifetime> for Option<u32> {
    fn from(value: Lifetime) -> Self {
        match value {
            Lifetime::HalfHour => Some(30),
            Lifetime::Hour => Some(60),
            Lifetime::TwoHours => Some(120),
            Lifetime::EightHours => Some(480),
            Lifetime::Day => Some(1440),
            Lifetime::UntilStopped => None,
        }
    }
}

impl TryFrom<Option<u32>> for Lifetime {
    type Error = &'static str;
    fn try_from(minutes: Option<u32>) -> Result<Self, Self::Error> {
        Self::CHOICES
            .into_iter()
            .find(|v| Option::from(*v) == minutes)
            .ok_or("Unsupported VM lifetime")
    }
}
