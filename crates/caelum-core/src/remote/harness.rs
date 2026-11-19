use super::Target;
use serde::{Deserialize, Serialize};

/// API keys and native provider sessions never enter this record. The cursor is
/// advanced atomically with its visible messages in the workspace snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    pub conversation: Option<String>,
    pub cursor: Option<String>,
    pub turn: Turn,
    pub lines: Vec<Line>,
    pub draft: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Turn {
    #[default]
    Idle,
    /// The non-idempotent POST may already have succeeded. Never replay on restore.
    Submitting,
    Following(Run),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub conversation: String,
    pub prompt: String,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Speaker {
    You,
    OpenCode,
    Activity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub speaker: Speaker,
    pub text: String,
    pub partial: bool,
}

impl Thread {
    pub(crate) fn append(&mut self, mut line: Line) {
        // Boat streams snapshots of a response; replace the provisional text until it is final.
        if self
            .lines
            .last()
            .is_some_and(|l| l.partial && l.speaker == line.speaker)
        {
            self.lines.pop();
        }
        line.text.truncate(line.text.floor_char_boundary(64 * 1024));
        self.lines.push(line);
        let mut bytes = 0;
        let count = self
            .lines
            .iter()
            .rev()
            .take(200)
            .take_while(|l| {
                bytes += l.text.len();
                bytes <= 512 * 1024
            })
            .count();
        self.lines.drain(..self.lines.len() - count);
    }
}

#[derive(Debug)]
pub struct Page {
    pub lines: Vec<Line>,
    pub cursor: Option<String>,
    pub more: bool,
    pub finished: Option<String>,
}

pub(super) trait Harness {
    async fn send(
        &self,
        target: &Target,
        conversation: Option<&str>,
        text: &str,
    ) -> Result<super::Outcome, String>;
    async fn follow(
        &self,
        target: &Target,
        run: &Run,
        cursor: Option<&str>,
    ) -> Result<Page, String>;
    async fn interrupt(&self, target: &Target, run: &Run) -> Result<(), String>;
}
