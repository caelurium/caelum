use std::{fmt::Write, path::Path};

use serde_json::Value;

use super::{args_str, clean_path, extract_path};

const CONTEXT_LINES: usize = 3;

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

    let mut prefix = 0usize;
    while prefix < old_lines.len()
        && prefix < new_lines.len()
        && old_lines[prefix] == new_lines[prefix]
    {
        prefix += 1;
    }
    let mut suffix = 0usize;
    while suffix + prefix < old_lines.len()
        && suffix + prefix < new_lines.len()
        && old_lines[old_lines.len() - 1 - suffix] == new_lines[new_lines.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let mid_old = &old_lines[prefix..old_lines.len() - suffix];
    let mid_new = &new_lines[prefix..new_lines.len() - suffix];
    if mid_old.is_empty() && mid_new.is_empty() {
        return String::new();
    }

    let lead = prefix.min(CONTEXT_LINES);
    let trail = suffix.min(CONTEXT_LINES);
    let base_old = prefix - lead;
    let base_new = prefix - lead;

    let mut ops = Vec::with_capacity(lead + mid_old.len() + mid_new.len() + trail);
    for line in &old_lines[base_old..prefix] {
        ops.push(DiffOp::Context(line));
    }
    diff_middle(mid_old, mid_new, &mut ops);
    let suffix_start = old_lines.len() - suffix;
    for line in &old_lines[suffix_start..suffix_start + trail] {
        ops.push(DiffOp::Context(line));
    }

    let mut change_ranges: Vec<(usize, usize)> = Vec::new();
    for (idx, op) in ops.iter().enumerate() {
        if !matches!(op, DiffOp::Context(_)) {
            let start = idx.saturating_sub(CONTEXT_LINES);
            let end = (idx + CONTEXT_LINES + 1).min(ops.len());
            if let Some(last) = change_ranges.last_mut() {
                if start <= last.1 {
                    last.1 = last.1.max(end);
                    continue;
                }
            }
            change_ranges.push((start, end));
        }
    }
    if change_ranges.is_empty() {
        return String::new();
    }

    let mut old_pos = Vec::with_capacity(ops.len() + 1);
    let mut new_pos = Vec::with_capacity(ops.len() + 1);
    let (mut o, mut n) = (base_old, base_new);
    for op in &ops {
        old_pos.push(o);
        new_pos.push(n);
        match op {
            DiffOp::Context(_) => {
                o += 1;
                n += 1;
            }
            DiffOp::Remove(_) => o += 1,
            DiffOp::Add(_) => n += 1,
        }
    }
    old_pos.push(o);
    new_pos.push(n);

    let old_header = if old_lines.is_empty() {
        "/dev/null".to_owned()
    } else {
        serde_json::to_string(&format!("a/{path}")).expect("string")
    };
    let new_header = serde_json::to_string(&format!("b/{path}")).expect("string");

    let mut out = format!("--- {old_header}\n+++ {new_header}\n");
    for (start, end) in change_ranges {
        let old_count = old_pos[end] - old_pos[start];
        let new_count = new_pos[end] - new_pos[start];
        let old_start = if old_count == 0 {
            old_pos[start]
        } else {
            old_pos[start] + 1
        };
        let new_start = if new_count == 0 {
            new_pos[start]
        } else {
            new_pos[start] + 1
        };
        let _ = writeln!(
            out,
            "@@ -{old_start},{old_count} +{new_start},{new_count} @@"
        );
        for op in &ops[start..end] {
            match op {
                DiffOp::Context(line) => {
                    out.push(' ');
                    out.push_str(line);
                    out.push('\n');
                }
                DiffOp::Remove(line) => {
                    out.push('-');
                    out.push_str(line);
                    out.push('\n');
                }
                DiffOp::Add(line) => {
                    out.push('+');
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
    }
    out
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

fn diff_middle<'a>(mid_old: &[&'a str], mid_new: &[&'a str], ops: &mut Vec<DiffOp<'a>>) {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_file_diff_emits_compact_hunk_around_changed_lines() {
        let mut old: Vec<String> = (1..=120).map(|i| format!("line {i}")).collect();
        let mut new = old.clone();
        new[79] = "line 80 updated".into();
        let diff = unified_diff("src/git.rs", &old.join("\n"), &new.join("\n"));

        assert!(diff.contains("@@ -77,7 +77,7 @@\n"), "{diff}");
        assert!(!diff.contains(" line 1\n"), "{diff}");
        assert!(diff.contains(" line 77\n"), "{diff}");
        assert!(diff.contains("-line 80\n+line 80 updated\n"), "{diff}");
        assert!(diff.contains(" line 83\n"), "{diff}");
        assert!(!diff.contains(" line 84\n"), "{diff}");

        // Identical files produce no patch.
        old[79] = "line 80 updated".into();
        assert!(unified_diff("src/git.rs", &old.join("\n"), &new.join("\n")).is_empty());
    }

    #[test]
    fn distant_edits_split_into_separate_hunks_and_new_file_starts_at_zero() {
        let old: Vec<String> = (1..=60).map(|i| format!("line {i}")).collect();
        let mut new = old.clone();
        new[9] = "line 10 changed".into();
        new[49] = "line 50 changed".into();
        let diff = unified_diff("src/lib.rs", &old.join("\n"), &new.join("\n"));

        assert!(diff.contains("@@ -7,7 +7,7 @@\n"), "{diff}");
        assert!(diff.contains("@@ -47,7 +47,7 @@\n"), "{diff}");
        assert!(!diff.contains(" line 25\n"), "{diff}");

        let added = unified_diff("src/new.rs", "", "a\nb\n");
        assert!(added.contains("--- /dev/null\n+++ \"b/src/new.rs\"\n@@ -0,0 +1,2 @@\n+a\n+b\n"));
    }
}
