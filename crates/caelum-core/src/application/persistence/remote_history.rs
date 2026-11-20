//! Read compatibility for workspaces saved by the direct-OpenCode VM preview.
//! Its session/message IDs cannot be followed or interrupted through Boat.
use serde_json::{json, Value};

pub(super) fn retire_vm_sessions(snapshot: &mut Value) {
    let Some(checkouts) = snapshot
        .pointer_mut("/remote/checkouts")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    for checkout in checkouts.values_mut() {
        if let Some(t) = checkout.get_mut("thread") {
            retire(t);
        }
        if let Some(parked) = checkout
            .get_mut("other_threads")
            .and_then(Value::as_array_mut)
        {
            parked.iter_mut().for_each(retire);
        }
    }
}

fn retire(thread: &mut Value) {
    if !thread["native_session"].is_string() && thread["turn"].get("NativeSubmitting").is_none() {
        return;
    }
    thread.as_object_mut().unwrap().remove("native_session");
    thread["turn"] = "Idle".into();
    thread["conversation"] = Value::Null;
    thread["cursor"] = Value::Null;
    if let Some(lines) = thread.get_mut("lines").and_then(Value::as_array_mut) {
        for line in lines.iter_mut() {
            if line["partial"] == true {
                line["partial"] = false.into();
            }
            if line["speaker"]["Tool"] == "Running" {
                line["speaker"]["Tool"] = "Reported".into();
            }
        }
        lines.push(json!({
            "speaker":"Activity","partial":false,
            "text":"Previous VM chat saved for reference. Your next send starts a new Boat conversation."
        }));
    }
}

#[cfg(test)]
mod tests;
