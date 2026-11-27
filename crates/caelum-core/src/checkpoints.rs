//! Hidden Git working-tree checkpoints for safe per-turn diffing and file restoration across all providers.

use crate::application::presentation::now;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::{Command, Output},
};
use uuid::Uuid;

#[cfg(test)]
#[path = "checkpoints_tests.rs"]
mod tests;

pub const MAX_SESSION_CHECKPOINTS: usize = 32;
const DIFF_LIMIT: usize = 256 * 1024;
const REF_PREFIX: &str = "refs/caelum/checkpoints/";

/// A lightweight hidden Git working-tree snapshot captured before an agent turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnCheckpoint {
    pub id: String,
    pub tree_oid: String,
    pub ref_name: String,
    pub prompt_excerpt: String,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
}

/// Diff between a saved checkpoint tree and the current working tree.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointDiff {
    pub paths: Vec<String>,
    pub diff: String,
    pub truncated: bool,
}

/// Captures the current working tree (tracked + non-ignored untracked files) using a
/// temporary Git index so the user's real index, HEAD, and stash remain untouched.
pub fn capture(project: &str, prompt_excerpt: &str) -> Result<Option<TurnCheckpoint>, String> {
    if crate::remote::native::checkout_id(project).is_some() {
        return Ok(None);
    }
    let Some(root) = repo_root(Path::new(project))? else {
        return Ok(None);
    };
    let tree_oid = write_working_tree(&root)?;
    let id = Uuid::new_v4().simple().to_string();
    let ref_name = format!("{REF_PREFIX}{id}");
    let commit = run_git(
        &root,
        &["commit-tree", &tree_oid, "-m", "caelum-turn-checkpoint"],
        None,
    )?;
    if !commit.status.success() {
        return Err(stderr_or("create checkpoint object", &commit));
    }
    let commit_oid = stdout_trim(&commit);
    let pin = run_git(&root, &["update-ref", &ref_name, &commit_oid], None)?;
    if !pin.status.success() {
        return Err(stderr_or("pin checkpoint ref", &pin));
    }
    let excerpt: String = prompt_excerpt
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("Turn checkpoint")
        .chars()
        .take(140)
        .collect();
    Ok(Some(TurnCheckpoint {
        id,
        tree_oid,
        ref_name,
        prompt_excerpt: excerpt,
        created_at: now(),
        turn_id: None,
    }))
}

/// Computes changed paths and a bounded unified diff between `tree_oid` and the current working tree.
pub fn diff_since(project: &str, tree_oid: &str) -> Result<CheckpointDiff, String> {
    validate_oid(tree_oid)?;
    let root = require_repo_root(Path::new(project))?;
    verify_tree(&root, tree_oid)?;
    let current = write_working_tree(&root)?;
    if current == tree_oid {
        return Ok(CheckpointDiff::default());
    }
    let names = run_git(
        &root,
        &[
            "diff",
            "--name-only",
            "--no-renames",
            "-z",
            tree_oid,
            &current,
        ],
        None,
    )?;
    if !names.status.success() {
        return Err(stderr_or("list checkpoint changes", &names));
    }
    let mut paths: Vec<String> = names
        .stdout
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    paths.sort();

    let patch = run_git(
        &root,
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--color=never",
            tree_oid,
            &current,
        ],
        None,
    )?;
    if !patch.status.success() {
        return Err(stderr_or("read checkpoint diff", &patch));
    }
    let (diff, truncated) = truncate_utf8(&patch.stdout, DIFF_LIMIT);
    Ok(CheckpointDiff {
        paths,
        diff,
        truncated,
    })
}

/// Restores working-tree files to match `tree_oid` without modifying the real Git index or HEAD.
/// Files added since the checkpoint are removed; modified or deleted files are restored.
pub fn restore(project: &str, tree_oid: &str) -> Result<Vec<String>, String> {
    validate_oid(tree_oid)?;
    let root = require_repo_root(Path::new(project))?;
    verify_tree(&root, tree_oid)?;
    let current = write_working_tree(&root)?;
    if current == tree_oid {
        return Ok(Vec::new());
    }
    let status = run_git(
        &root,
        &[
            "diff",
            "--name-status",
            "--no-renames",
            "-z",
            tree_oid,
            &current,
        ],
        None,
    )?;
    if !status.status.success() {
        return Err(stderr_or("inspect checkpoint delta", &status));
    }

    let mut added = Vec::new();
    let mut to_restore = Vec::new();
    let mut tokens = status.stdout.split(|&b| b == 0).filter(|s| !s.is_empty());
    while let Some(code) = tokens.next() {
        let Some(path_bytes) = tokens.next() else {
            break;
        };
        let path = String::from_utf8_lossy(path_bytes).into_owned();
        if !safe_relative(&path) {
            continue;
        }
        if code.starts_with(b"A") {
            added.push(path);
        } else {
            to_restore.push(path);
        }
    }

    for rel in &added {
        let target = root.join(rel);
        if target.is_dir() {
            let _ = fs::remove_dir_all(&target);
        } else if target.exists() || target.symlink_metadata().is_ok() {
            fs::remove_file(&target)
                .map_err(|e| format!("Failed to remove added file {rel}: {e}"))?;
        }
        let mut parent = target.parent();
        while let Some(dir) = parent {
            if dir == root {
                break;
            }
            if fs::remove_dir(dir).is_err() {
                break;
            }
            parent = dir.parent();
        }
    }

    if !to_restore.is_empty() {
        let mut args: Vec<String> = vec![
            "restore".into(),
            format!("--source={tree_oid}"),
            "--worktree".into(),
            "--".into(),
        ];
        args.extend(to_restore.iter().cloned());
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = run_git(&root, &refs, None)?;
        if !out.status.success() {
            return Err(stderr_or("restore checkpoint files", &out));
        }
    }

    let mut changed = added;
    changed.extend(to_restore);
    changed.sort();
    Ok(changed)
}

/// Removes a hidden checkpoint ref so `git gc` can reclaim unreferenced objects.
pub fn prune_ref(project: &str, ref_name: &str) {
    if !ref_name.starts_with(REF_PREFIX) {
        return;
    }
    if let Ok(Some(root)) = repo_root(Path::new(project)) {
        let _ = run_git(&root, &["update-ref", "-d", ref_name], None);
    }
}

fn write_working_tree(root: &Path) -> Result<String, String> {
    let temp = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    let index_path = temp.path().to_owned();
    drop(temp);
    let _cleanup = IndexGuard(index_path.clone());

    // Seed from HEAD if present (unborn branches start with an empty index).
    let _ = run_git(root, &["read-tree", "HEAD"], Some(&index_path));
    let add = run_git(root, &["add", "-A", "--", "."], Some(&index_path))?;
    if !add.status.success() {
        return Err(stderr_or("stage working tree into temporary index", &add));
    }
    let write = run_git(root, &["write-tree"], Some(&index_path))?;
    if !write.status.success() {
        return Err(stderr_or("write checkpoint tree", &write));
    }
    Ok(stdout_trim(&write))
}

struct IndexGuard(PathBuf);

impl Drop for IndexGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn repo_root(path: &Path) -> Result<Option<PathBuf>, String> {
    let out = run_git(path, &["rev-parse", "--show-toplevel"], None)?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        if err.contains("not a git repository") {
            return Ok(None);
        }
        return Err(stderr_or("detect git repository", &out));
    }
    let root = PathBuf::from(stdout_trim(&out));
    root.canonicalize()
        .map(Some)
        .map_err(|e| format!("resolve repository root: {e}"))
}

fn require_repo_root(path: &Path) -> Result<PathBuf, String> {
    repo_root(path)?.ok_or_else(|| "This project is not a Git repository.".into())
}

fn verify_tree(root: &Path, tree_oid: &str) -> Result<(), String> {
    let out = run_git(root, &["cat-file", "-t", tree_oid], None)?;
    if !out.status.success() || stdout_trim(&out) != "tree" {
        return Err("Checkpoint snapshot is no longer available in this Git repository.".into());
    }
    Ok(())
}

fn validate_oid(oid: &str) -> Result<(), String> {
    let valid = (7..=64).contains(&oid.len()) && oid.bytes().all(|b| b.is_ascii_hexdigit());
    if !valid {
        return Err("Invalid checkpoint tree OID.".into());
    }
    Ok(())
}

fn safe_relative(path: &str) -> bool {
    let p = Path::new(path);
    !path.is_empty()
        && !p.is_absolute()
        && !p.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        && p.components()
            .next()
            .is_none_or(|c| c.as_os_str() != ".git")
}

fn run_git(dir: &Path, args: &[&str], index: Option<&Path>) -> Result<Output, String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir).args(args);
    if let Some(index) = index {
        cmd.env("GIT_INDEX_FILE", index);
    }
    cmd.output().map_err(|e| format!("git {}: {e}", args[0]))
}

fn stdout_trim(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn stderr_or(action: &str, output: &Output) -> String {
    let err = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if err.is_empty() {
        format!("Failed to {action}.")
    } else {
        format!("Failed to {action}: {err}")
    }
}

fn truncate_utf8(bytes: &[u8], limit: usize) -> (String, bool) {
    if bytes.len() <= limit {
        return (String::from_utf8_lossy(bytes).into_owned(), false);
    }
    let text = String::from_utf8_lossy(&bytes[..limit]).into_owned();
    (text, true)
}
