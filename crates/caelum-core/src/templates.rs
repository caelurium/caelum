//! Plain-text prompt templates, with checkout definitions taking precedence over user definitions.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Read, path::Path};

mod catalog;
mod words;
pub use catalog::Catalog;
pub use catalog::Target;

const FILE_LIMIT: usize = 64 * 1024;
const COUNT_LIMIT: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Template {
    pub name: String,
    pub description: String,
    pub argument_hint: String,
    body: String,
}

fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
        && name != "ask"
        && !crate::application::commands::COMMANDS
            .iter()
            .any(|(n, _, _)| *n == name)
}

impl Template {
    /// Parse one Markdown command. Reserved names and invalid or empty definitions are omitted.
    pub fn parse(name: String, text: String) -> Option<Self> {
        if !valid_name(&name) || text.len() > FILE_LIMIT || text.contains('\0') {
            return None;
        }
        let text = text.replace("\r\n", "\n");
        let mut description = String::new();
        let mut argument_hint = String::new();
        let mut lines = text.as_str();
        if let Some(mut rest) = lines.strip_prefix("---\n") {
            loop {
                let (line, next) = rest.split_once('\n')?;
                rest = next;
                if line.trim_end() == "---" {
                    break;
                }
                if let Some((key, value)) = line.split_once(':') {
                    let value = value.trim();
                    let value = value
                        .strip_prefix('"')
                        .and_then(|v| v.strip_suffix('"'))
                        .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                        .unwrap_or(value);
                    match key.trim() {
                        "description" => description = value.chars().take(240).collect(),
                        "argument_hint" => argument_hint = value.chars().take(96).collect(),
                        _ => {}
                    }
                }
            }
            lines = rest;
        }
        let body = lines.trim_end().to_owned();
        (!body.trim().is_empty()).then_some(Self {
            name,
            description,
            argument_hint,
            body,
        })
    }

    /// Substitutions are literal, single-pass, and bounded to the VM message limit.
    pub fn render(&self, arguments: &str) -> Result<String, String> {
        let arguments = arguments.trim();
        let too_long = || "Expanded prompt exceeds 64 KiB.".to_owned();
        if arguments.len() > FILE_LIMIT {
            return Err(too_long());
        }
        let mut words = None;
        let mut rest = self.body.as_str();
        let mut out = String::new();
        while let Some((prefix, token)) = rest.split_once('$') {
            out.push_str(prefix);
            let digits = token.bytes().take_while(u8::is_ascii_digit).count();
            let (value, next) = if let Some(next) = token
                .strip_prefix("ARGUMENTS")
                .filter(|s| !s.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_'))
            {
                (arguments, next)
            } else if digits > 0 && !token.starts_with('0') {
                let args = words
                    .get_or_insert_with(|| words::split(arguments))
                    .as_ref()
                    .ok_or("Unclosed quote or escape in template arguments.")?;
                let value = token[..digits]
                    .parse::<usize>()
                    .ok()
                    .and_then(|n| args.get(n - 1))
                    .map_or("", String::as_str);
                (value, &token[digits..])
            } else {
                ("$", token)
            };
            if out.len() + value.len() > FILE_LIMIT {
                return Err(too_long());
            }
            out.push_str(value);
            rest = next;
        }
        if out.len() + rest.len() > FILE_LIMIT {
            return Err(too_long());
        }
        out.push_str(rest);
        if out.trim().is_empty() {
            return Err(format!("/{} expanded to an empty prompt.", self.name));
        }
        Ok(out)
    }
}

pub fn invocation(input: &str) -> Option<(&str, &str)> {
    let input = input.trim().strip_prefix('/')?;
    let (name, args) = input.split_once(char::is_whitespace).unwrap_or((input, ""));
    valid_name(name).then_some((name, args))
}

fn read_dir(root: &Path) -> Result<Vec<(String, String)>, String> {
    use rustix::fs::{open, openat, Dir, Mode, OFlags};
    use rustix::io::Errno;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
    let mut dir = open(root, flags, Mode::empty()).map_err(|e| e.to_string())?;
    for component in [".caelum", "commands"] {
        dir = match openat(&dir, component, flags | OFlags::NOFOLLOW, Mode::empty()) {
            Ok(dir) => dir,
            Err(Errno::NOENT | Errno::NOTDIR | Errno::LOOP) => return Ok(vec![]),
            Err(e) => return Err(e.to_string()),
        };
    }
    let mut names = Vec::new();
    for (i, entry) in Dir::read_from(&dir).map_err(|e| e.to_string())?.enumerate() {
        if i >= 4096 {
            return Err("Too many entries in .caelum/commands.".into());
        }
        let entry = entry.map_err(|e| e.to_string())?;
        if let Some(name) = entry
            .file_name()
            .to_str()
            .ok()
            .and_then(|s| s.strip_suffix(".md"))
            .filter(|n| valid_name(n))
        {
            names.push(name.to_owned());
        }
    }
    names.sort();
    let mut files = Vec::new();
    for name in names {
        let Ok(fd) = openat(
            &dir,
            format!("{name}.md"),
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        ) else {
            continue;
        };
        let f = std::fs::File::from(fd);
        if !f
            .metadata()
            .is_ok_and(|m| m.is_file() && m.len() <= FILE_LIMIT as u64)
        {
            continue;
        }
        let mut text = String::new();
        if f.take(FILE_LIMIT as u64 + 1)
            .read_to_string(&mut text)
            .is_err()
            || text.len() > FILE_LIMIT
        {
            continue;
        }
        files.push((name, text));
        if files.len() == COUNT_LIMIT {
            break;
        }
    }
    Ok(files)
}

fn merge(project: Vec<(String, String)>, user: Vec<(String, String)>) -> Vec<Template> {
    let mut entries = BTreeMap::new();
    for (name, text) in project.into_iter().chain(user) {
        if entries.len() == COUNT_LIMIT {
            break;
        }
        if entries.contains_key(&name) {
            continue;
        }
        if let Some(t) = Template::parse(name, text) {
            entries.insert(t.name.clone(), t);
        }
    }
    entries.into_values().collect()
}

fn with_user(project: Vec<(String, String)>) -> Result<Vec<Template>, String> {
    let user = std::env::var_os("HOME")
        .map(|home| read_dir(Path::new(&home)))
        .transpose()?
        .unwrap_or_default();
    Ok(merge(project, user))
}

pub(crate) fn load(project: &str) -> Result<Vec<Template>, String> {
    with_user(read_dir(Path::new(project))?)
}

pub(crate) const REMOTE_READ: &str = concat!(
    "python3 -I - <<'CAELUM_TEMPLATES'\n",
    include_str!("templates/read.py"),
    "\nCAELUM_TEMPLATES\n"
);
pub(crate) const REMOTE_LIMIT: usize = COUNT_LIMIT * (FILE_LIMIT.div_ceil(3) * 4 + 128) + 4;

fn remote_files(output: &[u8]) -> Result<Vec<(String, String)>, String> {
    if output.len() > REMOTE_LIMIT {
        return Err("VM template catalogue exceeds the size limit.".into());
    }
    let project: Vec<(&str, &str)> = serde_json::from_slice(output).map_err(|e| e.to_string())?;
    if project.len() > COUNT_LIMIT {
        return Err("Too many VM templates.".into());
    }
    Ok(project
        .into_iter()
        .filter_map(|(name, data)| {
            if !valid_name(name) || data.len() > FILE_LIMIT.div_ceil(3) * 4 {
                return None;
            }
            let text = String::from_utf8(STANDARD.decode(data).ok()?).ok()?;
            (text.len() <= FILE_LIMIT).then(|| (name.into(), text))
        })
        .collect())
}

pub(crate) fn from_remote(output: &[u8]) -> Result<Vec<Template>, String> {
    with_user(remote_files(output)?)
}

#[cfg(test)]
mod tests;
