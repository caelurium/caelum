use std::path::Path;

use serde_json::Value;

use super::{args_str, clean_path, extract_path};

pub(super) fn build_patch(
    raw: &Value,
    args: &Value,
    title: &str,
    project_path: Option<&Path>,
) -> (String, String) {
    let primary_path = extract_path(raw, args, title, project_path).unwrap_or_default();

    if let Some(items) = raw["content"].as_array() {
        let mut sections = Vec::new();
        for item in items {
            if item["type"] != "diff" {
                continue;
            }
            let item_path = item["path"]
                .as_str()
                .map(|p| clean_path(p, project_path))
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| {
                    if primary_path.is_empty() {
                        "file".into()
                    } else {
                        primary_path.clone()
                    }
                });
            let old = item["oldText"].as_str().unwrap_or_default();
            let new = item["newText"].as_str().unwrap_or_default();
            let diff = unified_diff(&item_path, old, new);
            if !diff.is_empty() {
                sections.push(diff);
            }
        }
        if !sections.is_empty() {
            let path = if primary_path.is_empty() {
                items
                    .iter()
                    .find(|i| i["type"] == "diff")
                    .and_then(|i| i["path"].as_str())
                    .map(|p| clean_path(p, project_path))
                    .unwrap_or_default()
            } else {
                primary_path
            };
            return (path, sections.join("\n"));
        }
    }

    if let Some(patch) = args_str(args, &["patch", "patchText", "diff"]) {
        return (primary_path, patch);
    }

    let path_label = if primary_path.is_empty() {
        "file"
    } else {
        primary_path.as_str()
    };

    if let Some(old) = args_str(
        args,
        &[
            "Target",
            "oldString",
            "old_string",
            "OldString",
            "TargetContent",
        ],
    ) {
        let new = args_str(
            args,
            &[
                "Replacement",
                "newString",
                "new_string",
                "NewString",
                "ReplacementContent",
            ],
        )
        .unwrap_or_default();
        let diff = unified_diff(path_label, &old, &new);
        return (primary_path, diff);
    }

    if let Some(chunks) = args["ReplacementChunks"].as_array() {
        let mut sections = Vec::new();
        for chunk in chunks {
            let old = chunk["TargetContent"].as_str().unwrap_or_default();
            let new = chunk["ReplacementContent"].as_str().unwrap_or_default();
            let diff = unified_diff(path_label, old, new);
            if !diff.is_empty() {
                sections.push(diff);
            }
        }
        if !sections.is_empty() {
            return (primary_path, sections.join("\n"));
        }
    }

    if let Some(content) = args_str(args, &["Content", "content"]) {
        let diff = unified_diff(path_label, "", &content);
        return (primary_path, diff);
    }

    (primary_path, String::new())
}

fn unified_diff(path: &str, old: &str, new: &str) -> String {
    let old_lines: Vec<&str> = split_diff_lines(old);
    let new_lines: Vec<&str> = split_diff_lines(new);
    if old_lines.is_empty() && new_lines.is_empty() {
        return String::new();
    }

    let ops = myers_or_lcs_ops(&old_lines, &new_lines);
    let mut body = String::new();
    for op in &ops {
        match op {
            DiffOp::Context(line) => {
                body.push(' ');
                body.push_str(line);
                body.push('\n');
            }
            DiffOp::Remove(line) => {
                body.push('-');
                body.push_str(line);
                body.push('\n');
            }
            DiffOp::Add(line) => {
                body.push('+');
                body.push_str(line);
                body.push('\n');
            }
        }
    }

    let old_header = if old_lines.is_empty() {
        "/dev/null".to_owned()
    } else {
        serde_json::to_string(&format!("a/{path}")).expect("string")
    };
    let new_header = serde_json::to_string(&format!("b/{path}")).expect("string");
    let old_start = usize::from(!old_lines.is_empty());
    let new_start = usize::from(!new_lines.is_empty());
    format!(
        "--- {old_header}\n+++ {new_header}\n@@ -{old_start},{} +{new_start},{} @@\n{body}",
        old_lines.len(),
        new_lines.len()
    )
}

fn split_diff_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.lines().collect()
    }
}

enum DiffOp<'a> {
    Context(&'a str),
    Remove(&'a str),
    Add(&'a str),
}

fn myers_or_lcs_ops<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<DiffOp<'a>> {
    let mut prefix = 0usize;
    while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
        prefix += 1;
    }
    let mut suffix = 0usize;
    while suffix + prefix < old.len()
        && suffix + prefix < new.len()
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let mid_old = &old[prefix..old.len() - suffix];
    let mid_new = &new[prefix..new.len() - suffix];

    let mut ops = Vec::with_capacity(old.len() + new.len());
    for line in &old[..prefix] {
        ops.push(DiffOp::Context(line));
    }

    if mid_old.len() * mid_new.len() <= 40_000 {
        let rows = mid_old.len();
        let cols = mid_new.len();
        let mut dp = vec![0usize; (rows + 1) * (cols + 1)];
        let stride = cols + 1;
        for i in (0..rows).rev() {
            for j in (0..cols).rev() {
                dp[i * stride + j] = if mid_old[i] == mid_new[j] {
                    dp[(i + 1) * stride + (j + 1)] + 1
                } else {
                    dp[(i + 1) * stride + j].max(dp[i * stride + (j + 1)])
                };
            }
        }
        let (mut i, mut j) = (0usize, 0usize);
        while i < rows && j < cols {
            if mid_old[i] == mid_new[j] {
                ops.push(DiffOp::Context(mid_old[i]));
                i += 1;
                j += 1;
            } else if dp[(i + 1) * stride + j] >= dp[i * stride + (j + 1)] {
                ops.push(DiffOp::Remove(mid_old[i]));
                i += 1;
            } else {
                ops.push(DiffOp::Add(mid_new[j]));
                j += 1;
            }
        }
        while i < rows {
            ops.push(DiffOp::Remove(mid_old[i]));
            i += 1;
        }
        while j < cols {
            ops.push(DiffOp::Add(mid_new[j]));
            j += 1;
        }
    } else {
        for line in mid_old {
            ops.push(DiffOp::Remove(line));
        }
        for line in mid_new {
            ops.push(DiffOp::Add(line));
        }
    }

    for line in &old[old.len() - suffix..] {
        ops.push(DiffOp::Context(line));
    }
    ops
}
