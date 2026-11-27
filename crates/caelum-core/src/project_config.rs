//! Per-project `.caelum/config.toml` rules, defaults, worktree setup commands, and runner presets.

use crate::application::Provider;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

#[cfg(test)]
#[path = "project_config_tests.rs"]
mod tests;

const FILE_LIMIT: usize = 64 * 1024;
const SETUP_LIMIT: usize = 16;
const PRESET_LIMIT: usize = 24;
const COMMAND_LIMIT: usize = 1024;
const NAME_LIMIT: usize = 64;

/// Project-level defaults for new sessions in this repository.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProjectDefaults {
    pub provider: Option<Provider>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub agent: Option<String>,
}

/// Worktree lifecycle configuration from `.caelum/config.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WorktreeConfig {
    pub setup: Vec<String>,
}

/// A named quick-run command preset for the checkout runner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerPreset {
    pub name: String,
    pub command: String,
}

/// Parsed `.caelum/config.toml` for a project checkout.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProjectConfig {
    pub defaults: ProjectDefaults,
    pub worktree: WorktreeConfig,
    pub runner_presets: Vec<RunnerPreset>,
}

impl ProjectConfig {
    /// Parse a `.caelum/config.toml` document. Unknown sections and keys are ignored
    /// so newer config fields remain forward-compatible.
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > FILE_LIMIT {
            return Err("Project config exceeds 64 KiB.".into());
        }
        if text.contains('\0') {
            return Err("Project config contains a NUL byte.".into());
        }
        let mut config = Self::default();
        let mut section = Section::Root;
        let mut preset: Option<PendingPreset> = None;
        let lines: Vec<&str> = text.lines().collect();
        let mut index = 0;

        while index < lines.len() {
            let line_no = index + 1;
            let raw = strip_comment(lines[index]).trim();
            index += 1;
            if raw.is_empty() {
                continue;
            }
            if let Some(name) = raw
                .strip_prefix("[[")
                .and_then(|rest| rest.strip_suffix("]]"))
            {
                flush_preset(&mut config, &mut preset)?;
                section = match name.trim() {
                    "runner.presets" => {
                        preset = Some(PendingPreset::default());
                        Section::RunnerPreset
                    }
                    _ => Section::Other,
                };
                continue;
            }
            if let Some(name) = raw
                .strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
            {
                flush_preset(&mut config, &mut preset)?;
                section = match name.trim() {
                    "defaults" => Section::Defaults,
                    "worktree" => Section::Worktree,
                    _ => Section::Other,
                };
                continue;
            }
            let Some((key, value_start)) = raw.split_once('=') else {
                return Err(format!("Line {line_no}: expected `key = value`."));
            };
            let key = key.trim();
            let mut value = value_start.trim().to_owned();
            if value.starts_with('[') && !array_closed(&value) {
                while index < lines.len() && !array_closed(&value) {
                    let next = strip_comment(lines[index]).trim();
                    index += 1;
                    if !next.is_empty() {
                        value.push(' ');
                        value.push_str(next);
                    }
                }
                if !array_closed(&value) {
                    return Err(format!("Line {line_no}: unterminated array."));
                }
            }
            match section {
                Section::Defaults => apply_defaults(&mut config.defaults, key, &value, line_no)?,
                Section::Worktree => apply_worktree(&mut config.worktree, key, &value, line_no)?,
                Section::RunnerPreset => {
                    if let Some(current) = preset.as_mut() {
                        apply_preset(current, key, &value, line_no)?;
                    }
                }
                Section::Root | Section::Other => {}
            }
        }
        flush_preset(&mut config, &mut preset)?;
        Ok(config)
    }

    /// Deterministic SHA-256 hex digest of the configured worktree setup commands.
    /// Any edit to the setup list invalidates prior trust.
    pub fn setup_fingerprint(&self) -> Option<String> {
        if self.worktree.setup.is_empty() {
            return None;
        }
        let mut hasher = Sha256::new();
        hasher.update(b"caelum-worktree-setup-v1\n");
        for cmd in &self.worktree.setup {
            hasher.update((cmd.len() as u64).to_be_bytes());
            hasher.update(cmd.as_bytes());
            hasher.update(b"\n");
        }
        let digest = hasher.finalize();
        let mut hex = String::with_capacity(digest.len() * 2);
        for byte in digest {
            use std::fmt::Write as _;
            let _ = write!(&mut hex, "{byte:02x}");
        }
        Some(hex)
    }

    /// Combines the setup commands into a single shell command for the checkout runner.
    pub fn combined_setup_command(&self) -> Option<String> {
        if self.worktree.setup.is_empty() {
            return None;
        }
        Some(self.worktree.setup.join(" && "))
    }
}

/// Safely read and parse `<project>/.caelum/config.toml` without following symlinks.
pub fn load(project: &str) -> Result<Option<ProjectConfig>, String> {
    use rustix::fs::{open, openat, Mode, OFlags};
    use rustix::io::Errno;

    let dir_flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
    let root = open(Path::new(project), dir_flags, Mode::empty()).map_err(|e| e.to_string())?;
    let caelum = match openat(
        &root,
        ".caelum",
        dir_flags | OFlags::NOFOLLOW,
        Mode::empty(),
    ) {
        Ok(dir) => dir,
        Err(Errno::NOENT | Errno::NOTDIR | Errno::LOOP) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let fd = match openat(
        &caelum,
        "config.toml",
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(Errno::NOENT | Errno::NOTDIR | Errno::LOOP) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let file = std::fs::File::from(fd);
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Ok(None);
    }
    if meta.len() > FILE_LIMIT as u64 {
        return Err(".caelum/config.toml exceeds 64 KiB.".into());
    }
    let mut text = String::new();
    file.take(FILE_LIMIT as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    ProjectConfig::parse(&text)
        .map(Some)
        .map_err(|e| format!(".caelum/config.toml: {e}"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Root,
    Defaults,
    Worktree,
    RunnerPreset,
    Other,
}

#[derive(Default)]
struct PendingPreset {
    name: Option<String>,
    command: Option<String>,
}

fn flush_preset(
    config: &mut ProjectConfig,
    pending: &mut Option<PendingPreset>,
) -> Result<(), String> {
    let Some(item) = pending.take() else {
        return Ok(());
    };
    let name = item.name.unwrap_or_default().trim().to_owned();
    let command = item.command.unwrap_or_default().trim().to_owned();
    if name.is_empty() || command.is_empty() {
        return Err("Each [[runner.presets]] entry requires both `name` and `command`.".into());
    }
    if name.chars().count() > NAME_LIMIT {
        return Err(format!(
            "Preset name `{name}` exceeds {NAME_LIMIT} characters."
        ));
    }
    if command.len() > COMMAND_LIMIT {
        return Err(format!(
            "Preset `{name}` command exceeds {COMMAND_LIMIT} bytes."
        ));
    }
    if config.runner_presets.len() >= PRESET_LIMIT {
        return Err(format!(
            "At most {PRESET_LIMIT} runner presets are supported."
        ));
    }
    config.runner_presets.push(RunnerPreset { name, command });
    Ok(())
}

fn apply_defaults(
    defaults: &mut ProjectDefaults,
    key: &str,
    value: &str,
    line_no: usize,
) -> Result<(), String> {
    let parsed = parse_string(value, line_no)?;
    let trimmed = parsed.trim();
    match key {
        "provider" => {
            if trimmed.is_empty() {
                defaults.provider = None;
            } else {
                defaults.provider = Some(match trimmed.to_ascii_lowercase().as_str() {
                    "codex" => Provider::Codex,
                    "opencode" => Provider::OpenCode,
                    "antigravity" => Provider::Antigravity,
                    other => {
                        return Err(format!(
                            "Line {line_no}: unknown provider `{other}` (expected codex, opencode, or antigravity)."
                        ));
                    }
                });
            }
        }
        "model" => defaults.model = (!trimmed.is_empty()).then(|| trimmed.to_owned()),
        "effort" => defaults.effort = (!trimmed.is_empty()).then(|| trimmed.to_owned()),
        "agent" => defaults.agent = (!trimmed.is_empty()).then(|| trimmed.to_owned()),
        _ => {}
    }
    Ok(())
}

fn apply_worktree(
    worktree: &mut WorktreeConfig,
    key: &str,
    value: &str,
    line_no: usize,
) -> Result<(), String> {
    if key != "setup" {
        return Ok(());
    }
    let items = parse_string_array(value, line_no)?;
    if items.len() > SETUP_LIMIT {
        return Err(format!(
            "Line {line_no}: at most {SETUP_LIMIT} worktree setup commands are supported."
        ));
    }
    let mut cleaned = Vec::with_capacity(items.len());
    for item in items {
        let cmd = item.trim().to_owned();
        if cmd.is_empty() {
            continue;
        }
        if cmd.len() > COMMAND_LIMIT {
            return Err(format!(
                "Line {line_no}: worktree setup command exceeds {COMMAND_LIMIT} bytes."
            ));
        }
        cleaned.push(cmd);
    }
    worktree.setup = cleaned;
    Ok(())
}

fn apply_preset(
    preset: &mut PendingPreset,
    key: &str,
    value: &str,
    line_no: usize,
) -> Result<(), String> {
    match key {
        "name" => preset.name = Some(parse_string(value, line_no)?),
        "command" => preset.command = Some(parse_string(value, line_no)?),
        _ => {}
    }
    Ok(())
}

fn strip_comment(line: &str) -> &str {
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        if in_double {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_double = false;
            }
        } else if in_single {
            if ch == '\'' {
                in_single = false;
            }
        } else {
            match ch {
                '"' => in_double = true,
                '\'' => in_single = true,
                '#' => return &line[..idx],
                _ => {}
            }
        }
    }
    line
}

fn array_closed(text: &str) -> bool {
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;
    let mut depth = 0usize;
    for ch in text.chars() {
        if in_double {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_double = false;
            }
        } else if in_single {
            if ch == '\'' {
                in_single = false;
            }
        } else {
            match ch {
                '"' => in_double = true,
                '\'' => in_single = true,
                '[' => depth += 1,
                ']' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return true;
                    }
                }
                _ => {}
            }
        }
    }
    false
}

fn parse_string(raw: &str, line_no: usize) -> Result<String, String> {
    let raw = raw.trim();
    if let Some(inner) = raw.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        return Ok(inner.to_owned());
    }
    let Some(inner) = raw.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return Err(format!("Line {line_no}: expected a quoted string."));
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(other) => {
                return Err(format!("Line {line_no}: unsupported escape `\\{other}`."));
            }
            None => return Err(format!("Line {line_no}: trailing backslash in string.")),
        }
    }
    Ok(out)
}

fn parse_string_array(raw: &str, line_no: usize) -> Result<Vec<String>, String> {
    let raw = raw.trim();
    let Some(inner) = raw.strip_prefix('[').and_then(|s| s.strip_suffix(']')) else {
        return Err(format!("Line {line_no}: expected an array of strings."));
    };
    let mut items = Vec::new();
    let mut start = 0usize;
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;
    for (idx, ch) in inner.char_indices() {
        if in_double {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_double = false;
            }
        } else if in_single {
            if ch == '\'' {
                in_single = false;
            }
        } else {
            match ch {
                '"' => in_double = true,
                '\'' => in_single = true,
                ',' => {
                    let piece = inner[start..idx].trim();
                    if !piece.is_empty() {
                        items.push(parse_string(piece, line_no)?);
                    }
                    start = idx + 1;
                }
                _ => {}
            }
        }
    }
    if in_double || in_single {
        return Err(format!("Line {line_no}: unterminated string in array."));
    }
    let tail = inner[start..].trim();
    if !tail.is_empty() {
        items.push(parse_string(tail, line_no)?);
    }
    Ok(items)
}
