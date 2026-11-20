use super::ToolState;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Enough to re-read the exact published call without saving arguments or output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSource {
    pub conversation: String,
    pub prompt: String,
    pub event: String,
    pub before: Option<String>,
    pub call: ToolCall,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolCall {
    Id(String),
    Slot(u16, String),
}

impl ToolSource {
    pub(super) fn bytes(&self) -> usize {
        self.conversation.len()
            + self.prompt.len()
            + self.event.len()
            + self.before.as_ref().map_or(0, String::len)
            + match &self.call {
                ToolCall::Id(id) | ToolCall::Slot(_, id) => id.len(),
            }
    }
}

/// Memory-only, bounded display of an explicitly opened tool result.
pub struct ToolOutput {
    pub command: String,
    pub output: String,
    pub state: ToolState,
    pub clipped: bool,
}

impl fmt::Debug for ToolOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolOutput")
            .field("state", &self.state)
            .field("clipped", &self.clipped)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub enum ToolLoad {
    Loading(u64),
    Ready(ToolOutput),
    Failed(String),
}

#[derive(Debug)]
pub struct ToolInspection {
    pub(crate) checkout: String,
    pub(crate) thread: String,
    pub(crate) line: String,
    pub(crate) source: ToolSource,
    pub(crate) state: ToolState,
    pub load: ToolLoad,
}

impl ToolInspection {
    pub fn owns(&self, checkout: &str, thread: &str, line: &str) -> bool {
        self.checkout == checkout && self.thread == thread && self.line == line
    }
}
