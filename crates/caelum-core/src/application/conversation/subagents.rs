//! Reconcile a background launch with its provider-tagged completion report.
use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use super::{Entry, EntryKind};
use crate::application::subagents::{child_id, valid_id};

pub(super) fn tool(entry: &mut Entry) {
    if entry.name != "subagent" || child_id(entry).is_none() {
        return;
    }
    // The tool can finish as soon as the child is launched. That is not the
    // subagent's completion; the later synthetic report finishes the task.
    if entry.error.is_empty() && entry.metadata["status"] == "running" {
        entry.complete = false;
        entry.status = "running".into();
        entry.background = true;
        entry.metadata.as_object_mut().unwrap().remove("ended");
        entry.output.clear();
    } else if entry.complete {
        let id = child_id(entry).unwrap();
        let header = format!("<subagent sessionID=\"{id}\" state=\"completed\">");
        entry.output = body(&entry.output, &header).into();
    }
}

pub(super) fn synthetic(data: &Value) -> Option<Entry> {
    if data["metadata"]["source"] != "subagent" {
        return None;
    }
    let child = data["metadata"]["childID"]
        .as_str()
        .filter(|id| valid_id(id))?;
    let id = data["id"].as_str().map(str::to_owned).or_else(|| {
        data["_event_id"]
            .as_str()
            .map(|id| id.replacen("evt_", "msg_", 1))
    })?;
    let description = data["description"].as_str().unwrap_or("Delegated task");
    let state = data["metadata"]["state"].as_str().unwrap_or("completed");
    let header =
        format!("<subagent sessionID=\"{child}\" state=\"{state}\" description=\"{description}\">");
    let mut entry = Entry::new(
        format!("opencode-{id}"),
        EntryKind::Tool,
        String::new(),
        true,
    );
    entry.name = "subagent".into();
    entry.background = true;
    entry.status = state.into();
    entry.input =
        json!({"agent": data["metadata"]["agent"], "description": description}).to_string();
    entry.output = body(data["text"].as_str().unwrap_or_default(), &header).into();
    entry.metadata = data["metadata"].clone();
    entry.metadata["sessionID"] = child.into();
    if let Some(at) = data["_created"]
        .as_u64()
        .or(data["time"]["created"].as_u64())
    {
        entry.metadata["ended"] = at.into();
    }
    Some(entry)
}

fn body<'a>(text: &'a str, header: &str) -> &'a str {
    text.strip_prefix(header)
        .and_then(|body| body.strip_suffix("</subagent>"))
        .map(|body| {
            let body = body.strip_prefix('\n').unwrap_or(body);
            body.strip_suffix('\n').unwrap_or(body)
        })
        .unwrap_or(text)
}

pub(super) fn reconcile(entries: &mut Vec<Entry>) {
    if !entries.iter().any(|entry| {
        entry.metadata["source"] == "subagent" || (child_id(entry).is_some() && !entry.complete)
    }) {
        return;
    }
    let mut calls: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        if entry.metadata["source"] != "subagent" {
            if let Some(child) = child_id(entry) {
                calls.entry(child.into()).or_default().push(index);
            }
        }
    }
    // Retain the report's identity on its task. An older history page can still
    // contain another handoff from that run after the synthetic row is consumed.
    let mut reports: Vec<_> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            entry.metadata["source"] == "subagent" || entry.metadata["subagentReport"].is_string()
        })
        .map(|(index, entry)| (index, entry.clone()))
        .collect();
    reports.sort_by_key(|(index, entry)| (entry.metadata["ended"].as_u64(), *index));
    let mut consumed = HashSet::new();
    for (result, report) in reports {
        let Some(child) = child_id(&report) else {
            continue;
        };
        let ended = report.metadata["ended"].as_u64();
        let Some(call) = calls.get(child).and_then(|calls| {
            calls
                .iter()
                .copied()
                .filter(|&i| {
                    ended
                        .zip(entries[i].started_at)
                        .map_or(i < result, |(end, start)| end >= start)
                })
                .max_by_key(|&i| (entries[i].started_at, i))
        }) else {
            continue;
        };
        let joined = calls[child].clone();
        let task = &mut entries[call];
        if task.metadata["subagentReport"].is_string()
            && task.metadata["ended"]
                .as_u64()
                .zip(ended)
                .is_some_and(|(known, at)| known > at)
        {
            if report.metadata["source"] == "subagent" {
                consumed.insert(result);
            }
            continue;
        }
        let report_id = report.metadata["subagentReport"]
            .as_str()
            .unwrap_or(&report.id);
        task.complete = true;
        task.status.clone_from(&report.status);
        task.output.clone_from(&report.output);
        task.history_output.clone_from(&task.output);
        task.stream_output.clear();
        task.metadata["status"] = task.status.clone().into();
        task.metadata["subagentReport"] = report_id.into();
        if let Some(ended) = ended {
            task.metadata["ended"] = ended.into();
        }
        // A follow-up to an already-running child joins that run. Finish the
        // earlier pending handoffs too, keeping the report on its latest task.
        for earlier in joined.into_iter().filter(|&i| i != call) {
            let task = &mut entries[earlier];
            if ended
                .zip(task.started_at)
                .is_none_or(|(end, start)| end < start)
            {
                continue;
            }
            if task.complete {
                // A late earlier report can refine a handoff previously joined
                // to a later run, but must not replace a task's own result.
                let newer_join = task.background
                    && task.error.is_empty()
                    && !task.metadata["subagentReport"].is_string()
                    && task.metadata["ended"]
                        .as_u64()
                        .zip(ended)
                        .is_some_and(|(known, at)| known > at);
                if !newer_join {
                    continue;
                }
            }
            task.complete = true;
            task.status.clone_from(&report.status);
            task.metadata["status"] = task.status.clone().into();
            task.metadata["ended"] = ended.into();
        }
        if report.metadata["source"] == "subagent" {
            consumed.insert(result);
        } else if result != call {
            let previous = &mut entries[result];
            previous.output.clear();
            previous.history_output.clear();
            previous.stream_output.clear();
            previous
                .metadata
                .as_object_mut()
                .unwrap()
                .remove("subagentReport");
        }
    }
    let mut index = 0;
    entries.retain(|_| {
        let keep = !consumed.contains(&index);
        index += 1;
        keep
    });
}
