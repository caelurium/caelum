use std::{io::Write, process::ChildStdin};

use serde_json::Value;

use crate::blocking_actor::BlockingActor;

#[derive(Clone)]
pub(crate) struct JsonLines(BlockingActor<ChildStdin>);

impl JsonLines {
    pub(crate) fn new(stdin: ChildStdin) -> Self {
        Self(BlockingActor::spawn("caelum-json-writer", stdin))
    }

    pub(crate) fn send(&self, value: &Value) -> Result<(), String> {
        let mut line = serde_json::to_vec(value).map_err(|error| error.to_string())?;
        line.push(b'\n');
        self.0
            .call(move |stdin| stdin.write_all(&line).and_then(|()| stdin.flush()))
            .ok_or("Provider input writer has stopped.")?
            .map_err(|error| error.to_string())
    }
}
