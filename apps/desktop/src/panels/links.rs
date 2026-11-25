use rune::Tx;

use crate::{
    Workspace,
    live::{live, live_mut},
};

use super::Screen;

pub fn open_link(s: &mut Workspace, index: usize, url: &str, tx: &mut Tx) {
    if url.starts_with("https://") || url.starts_with("http://") {
        let _ = tx.open_url(url);
        return;
    }
    let project = live(s).app.sessions[index].scope.project.clone();
    let (path, line) = url
        .split_once("#L")
        .or_else(|| {
            url.rsplit_once(':')
                .filter(|(_, n)| n.parse::<usize>().is_ok())
        })
        .map_or((url, None), |(path, line)| {
            (
                path,
                line.split('-').next().and_then(|v| v.parse::<usize>().ok()),
            )
        });
    let decoded = if let Some(rest) = path.strip_prefix("file://") {
        let rest = rest
            .strip_prefix("localhost")
            .filter(|path| path.starts_with('/'))
            .unwrap_or(rest);
        percent_decode(rest)
    } else {
        path.to_owned()
    };
    let path = std::path::Path::new(&decoded);
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return;
    }
    let target = if path.is_absolute() {
        if let Ok(relative) = path.strip_prefix(&project) {
            Some(relative.to_string_lossy().into_owned())
        } else if caelum_core::remote::native::checkout_id(&project).is_none() {
            Some(decoded)
        } else {
            None
        }
    } else {
        Some(decoded)
    };
    if let Some(path) = target {
        let live = live_mut(s);
        live.app.select(index);
        live.panels.pending_line = line.map(|line| (project.clone(), path.clone(), line));
        live.app.open_file(project, path);
        live.panels.inspector = Screen::Files;
        live.panels.preview_page = line.unwrap_or(1).saturating_sub(1) / 200;
        s.inspector.set_open(true);
    }
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_owned())
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
