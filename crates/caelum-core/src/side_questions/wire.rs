use crate::processes::{ProcessId, ProcessRegistry};
use serde_json::{json, Value};
use std::process::{Command, Stdio};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout},
};

struct Child<'a>(&'a ProcessRegistry, ProcessId);
impl Drop for Child<'_> {
    fn drop(&mut self) {
        let _ = self.0.terminate(self.1);
    }
}
pub(super) struct Wire<'a> {
    _child: Child<'a>,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}
impl<'a> Wire<'a> {
    pub(super) fn start(cmd: &mut Command, registry: &'a ProcessRegistry) -> Result<Self, String> {
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let child = Child(
            registry,
            registry.register(child).map_err(|e| e.to_string())?,
        );
        Ok(Self {
            _child: child,
            input: ChildStdin::from_std(input).map_err(|e| e.to_string())?,
            output: BufReader::new(ChildStdout::from_std(output).map_err(|e| e.to_string())?),
        })
    }
    pub(super) async fn send(&mut self, value: Value) -> Result<(), String> {
        let mut bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        self.input
            .write_all(&bytes)
            .await
            .map_err(|e| e.to_string())
    }
    pub(super) async fn next(&mut self) -> Result<Value, String> {
        loop {
            let mut bytes = Vec::new();
            loop {
                let buf = self.output.fill_buf().await.map_err(|e| e.to_string())?;
                if buf.is_empty() {
                    return Err("Side-question provider disconnected.".into());
                }
                let end = buf.iter().position(|&b| b == b'\n');
                let n = end.map_or(buf.len(), |i| i + 1);
                if bytes.len() + n > 1024 * 1024 {
                    return Err("Oversized side-question response.".into());
                }
                bytes.extend_from_slice(&buf[..n]);
                self.output.consume(n);
                if end.is_some() {
                    break;
                }
            }
            let msg: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if let (Some(method), Some(id)) = (msg["method"].as_str(), msg.get("id")) {
                let reply = if method == "session/request_permission" {
                    json!({"jsonrpc":"2.0","id":id,"result":{"outcome":{"outcome":"cancelled"}}})
                } else {
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Unavailable during a read-only side question"}})
                };
                self.send(reply).await?;
            } else {
                return Ok(msg);
            }
        }
    }
    pub(super) async fn request(
        &mut self,
        id: u64,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        loop {
            if let Some(result) = reply(&self.next().await?, id)? {
                return Ok(result);
            }
        }
    }
}
pub(super) fn reply(msg: &Value, id: u64) -> Result<Option<Value>, String> {
    if msg["id"].as_u64() != Some(id) {
        return Ok(None);
    }
    if let Some(v) = msg.get("error") {
        return Err(v["message"]
            .as_str()
            .unwrap_or("Provider rejected the side question.")
            .chars()
            .take(512)
            .collect());
    }
    msg.get("result")
        .cloned()
        .map(Some)
        .ok_or_else(|| "Missing side-question response.".into())
}
